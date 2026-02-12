import logging
import datetime
import json
import os
from pathlib import Path
from typing import Callable, Collection, Union
from urllib.parse import urlparse

from jinja2 import Environment, PackageLoader, Template, select_autoescape
from markupsafe import Markup

from .._model import Webmention

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

    def render_webmention(
        self, webmention: Webmention, template: TemplateLike | None = None
    ) -> str:
        if (
            Environment is None
            or PackageLoader is None
            or Template is None
            or select_autoescape is None
        ):
            raise RuntimeError(
                "Jinja2 is required to render templates. Please install 'jinja2'."
            )

        template_obj: Template | None = None

        env = Environment(
            loader=PackageLoader("webmentions", "templates"),
            autoescape=select_autoescape(enabled_extensions=("html", "xml")),
        )

        if template is None:
            template_obj = env.get_template("webmention.html")
        elif isinstance(template, Path) or (
            isinstance(template, str) and os.path.isfile(template)
        ):
            template_path = Path(template)
            template_obj = env.from_string(template_path.read_text(encoding="utf-8"))
        elif isinstance(template, str):
            template_obj = env.from_string(template)
        elif isinstance(template, Template):
            template_obj = template

        assert template_obj, "Template not found"
        return template_obj.render(
            mention=webmention.to_dict(), **TemplateUtils.to_dict()
        )

    def render_webmentions(
        self, webmentions: Collection[Webmention], template: TemplateLike | None = None
    ) -> list[Markup]:
        return [
            Markup(self.render_webmention(wm, template=template)) for wm in webmentions
        ]
