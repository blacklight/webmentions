import logging
import datetime
import json
import os
import re
from pathlib import Path
from typing import Callable, Collection, Union
from urllib.parse import urlparse

from jinja2 import Environment, PackageLoader, Template, select_autoescape
from markupsafe import Markup

from .._model import Webmention

# HTML sanitisation: tags and attributes allowed in content.
_ALLOWED_TAGS = frozenset(
    {
        "a",
        "p",
        "br",
        "span",
        "strong",
        "em",
        "b",
        "i",
        "u",
        "s",
        "del",
        "blockquote",
        "pre",
        "code",
        "ul",
        "ol",
        "li",
    }
)
_ALLOWED_ATTRS = frozenset(
    {
        "href",
        "class",
        "rel",
        "translate",
        "title",
        "lang",
        "dir",
    }
)
_TAG_RE = re.compile(r"<(/?)(\w+)([^>]*)>", re.DOTALL)
_ATTR_RE = re.compile(r'(\w[\w-]*)=["\']([^"\']*)["\']')


def _sanitize_html(html: str) -> Markup:
    """
    Strip disallowed tags and attributes from *html*, returning a
    :class:`Markup` instance safe for rendering.
    """

    def _replace_tag(m: re.Match) -> str:
        slash, tag, attrs_str = m.group(1), m.group(2).lower(), m.group(3)
        if tag not in _ALLOWED_TAGS:
            return ""
        if slash:
            return f"</{tag}>"
        # Filter attributes
        safe_attrs = []
        for am in _ATTR_RE.finditer(attrs_str):
            attr_name = am.group(1).lower()
            if attr_name in _ALLOWED_ATTRS:
                # Extra check: only allow safe href schemes
                if attr_name == "href":
                    val = am.group(2).strip()
                    parsed = urlparse(val)
                    if parsed.scheme.lower() not in ("http", "https", ""):
                        continue
                safe_attrs.append(f'{am.group(1)}="{am.group(2)}"')
        attr_str = (" " + " ".join(safe_attrs)) if safe_attrs else ""
        return f"<{tag}{attr_str}>"

    return Markup(_TAG_RE.sub(_replace_tag, html))


TemplateLike = Union[str, Path, Template]

logger = logging.getLogger(__name__)


class TemplateUtils:
    """
    Collection of Jinja2 template helper functions.
    """

    @staticmethod
    def format_date(d: object) -> str:
        if not d:
            return ""
        if isinstance(d, datetime.datetime):
            return d.strftime("%b %d, %Y")
        if isinstance(d, str):
            return datetime.datetime.fromisoformat(d).strftime("%b %d, %Y")
        return str(d)

    @staticmethod
    def format_datetime(dt: object) -> str:
        if not dt:
            return ""
        if isinstance(dt, datetime.datetime):
            return dt.strftime("%b %d, %Y at %H:%M")
        if isinstance(dt, str):
            return datetime.datetime.fromisoformat(dt).strftime("%b %d, %Y at %H:%M")
        return str(dt)

    @staticmethod
    def as_url(v: object) -> str:
        if isinstance(v, str):
            return v
        if isinstance(v, dict):
            return str(v.get("url") or v.get("value") or "")
        if isinstance(v, (list, tuple)):
            return str(v[0]) if v else ""
        return ""

    @classmethod
    def hostname(cls, url: str) -> str:
        if not url:
            return ""
        return urlparse(cls.as_url(url)).hostname or ""

    @classmethod
    def safe_url(cls, url: object) -> str:
        u = cls.as_url(url).strip()
        if not u:
            return ""

        parsed = urlparse(u)
        if parsed.scheme.lower() not in {"http", "https"}:
            return ""
        if not parsed.netloc:
            return ""

        return u

    @staticmethod
    def fromjson(v: object) -> object:
        return (
            json.loads(v)
            if isinstance(v, str) and v and v.strip() and v.strip()[0] in '[{"'
            else ({} if v is None else v)
        )

    @staticmethod
    def sanitize_html(html: object) -> Markup:
        """Sanitize HTML content, returning a safe :class:`Markup` instance."""
        if not html:
            return Markup("")
        return _sanitize_html(str(html))

    @classmethod
    def to_dict(cls) -> dict[str, Callable]:
        helpers: dict[str, Callable] = {}
        for name in dir(cls):
            if name.startswith("_"):
                continue
            value = getattr(cls, name)
            if callable(value):
                helpers[name] = value
        return helpers


class WebmentionsRenderer:
    """
    Webmentions renderer.

    A utility class for rendering Webmentions into HTML through Jinja2 templates.
    """

    def _get_template(self, template: TemplateLike | None, *, default: str) -> Template:
        if Environment is None:
            raise RuntimeError("Jinja2 is required for rendering Webmentions")

        env = Environment(
            loader=PackageLoader("webmentions", "templates"),
            autoescape=select_autoescape(enabled_extensions=("html", "xml")),
        )

        template_obj = None
        if template is None:
            template_obj = env.get_template(default)
        elif isinstance(template, Path) or (
            isinstance(template, str) and os.path.isfile(template)
        ):
            template_path = Path(template)
            template_obj = env.from_string(template_path.read_text(encoding="utf-8"))
        elif isinstance(template, str):
            template_obj = env.from_string(template)
        elif isinstance(template, Template):
            template_obj = template

        if not template_obj:
            raise ValueError(f"Invalid template: {template}")

        return template_obj

    def _get_markup(self, template: TemplateLike | None, *, default: str, **kwargs):
        template_obj = self._get_template(template, default=default)
        return Markup(template_obj.render(**kwargs, **TemplateUtils.to_dict()))

    def render_webmention(
        self, webmention: Webmention, template: TemplateLike | None = None
    ) -> Markup:
        return self._get_markup(template, default="webmention.html", mention=webmention)

    def render_webmentions(
        self, webmentions: Collection[Webmention], template: TemplateLike | None = None
    ) -> Markup:
        def _sort_key(wm: Webmention):
            return (
                wm.created_at
                or wm.published
                or datetime.datetime.min.replace(tzinfo=datetime.timezone.utc)
            )

        sorted_mentions = sorted(
            webmentions,
            key=_sort_key,
            reverse=True,
        )
        rendered_mentions = [
            self.render_webmention(mention) for mention in sorted_mentions
        ]
        counts = {"likes": 0, "reposts": 0, "replies": 0, "mentions": 0}
        for wm in sorted_mentions:
            mt = getattr(wm, "mention_type", None)
            if mt is not None:
                type_val = mt.value if hasattr(mt, "value") else str(mt)
                if type_val == "like":
                    counts["likes"] += 1
                elif type_val == "repost":
                    counts["reposts"] += 1
                elif type_val == "reply":
                    counts["replies"] += 1
                else:
                    counts["mentions"] += 1
        return self._get_markup(
            template,
            default="webmentions.html",
            mentions=rendered_mentions,
            counts=counts,
        )
