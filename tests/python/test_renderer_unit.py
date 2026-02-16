import datetime
import json

import pytest
from jinja2 import Environment

from webmentions import Webmention, WebmentionDirection
from webmentions.render import _renderer
from webmentions.render._renderer import TemplateUtils, WebmentionsRenderer


def test_template_utils_format_date_accepts_datetime_and_iso_string():
    dt = datetime.datetime(2024, 1, 2, 3, 4, tzinfo=datetime.timezone.utc)
    assert TemplateUtils.format_date(dt) == "Jan 02, 2024"
    assert TemplateUtils.format_date(dt.isoformat()) == "Jan 02, 2024"


def test_template_utils_format_date_none_and_fallback_to_str():
    assert TemplateUtils.format_date(None) == ""
    assert TemplateUtils.format_date(123) == "123"


def test_template_utils_format_datetime_accepts_datetime_and_iso_string():
    dt = datetime.datetime(2024, 1, 2, 3, 4, tzinfo=datetime.timezone.utc)
    assert TemplateUtils.format_datetime(dt) == "Jan 02, 2024 at 03:04"
    assert TemplateUtils.format_datetime(dt.isoformat()) == "Jan 02, 2024 at 03:04"


def test_template_utils_format_datetime_none_and_fallback_to_str():
    assert TemplateUtils.format_datetime(None) == ""
    assert TemplateUtils.format_datetime(123) == "123"


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("https://example.com/a", "https://example.com/a"),
        ({"url": "https://example.com/b"}, "https://example.com/b"),
        ({"value": "https://example.com/c"}, "https://example.com/c"),
        (["https://example.com/d"], "https://example.com/d"),
        ([], ""),
        (None, ""),
    ],
)
def test_template_utils_as_url(value, expected):
    assert TemplateUtils.as_url(value) == expected


def test_template_utils_hostname():
    assert TemplateUtils.hostname("https://sub.example.com/a") == "sub.example.com"
    assert TemplateUtils.hostname("") == ""


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("https://example.com/a", "https://example.com/a"),
        ("http://example.com/a", "http://example.com/a"),
        ("https:///nohost", ""),
        ("javascript:alert(1)", ""),
        ("data:text/html;base64,PGgxPjwvaDE+", ""),
        ("//example.com/relative", ""),
        ("/relative", ""),
        ("", ""),
        (None, ""),
        ({"url": "https://example.com/b"}, "https://example.com/b"),
    ],
)
def test_template_utils_safe_url(value, expected):
    assert TemplateUtils.safe_url(value) == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (None, {}),
        ("", ""),
        ("notjson", "notjson"),
        ("{}", {}),
        ("[]", []),
        (json.dumps({"a": 1}), {"a": 1}),
    ],
)
def test_template_utils_fromjson(value, expected):
    assert TemplateUtils.fromjson(value) == expected


def _build_minimal_webmention(**kwargs):
    return Webmention(
        source=kwargs.get("source", "https://source.example/post"),
        target=kwargs.get("target", "https://target.example/article"),
        direction=kwargs.get("direction", WebmentionDirection.IN),
        title=kwargs.get("title", "A title"),
        author_name=kwargs.get("author_name", "Alice"),
        author_url=kwargs.get("author_url", "https://source.example/"),
        created_at=kwargs.get(
            "created_at",
            datetime.datetime(2024, 1, 2, 3, 4, tzinfo=datetime.timezone.utc),
        ),
        **{
            k: v
            for k, v in kwargs.items()
            if k
            not in {
                "source",
                "target",
                "direction",
                "title",
                "author_name",
                "author_url",
                "created_at",
            }
        },
    )


def test_renderer_renders_default_packaged_template():
    renderer = WebmentionsRenderer()
    wm = _build_minimal_webmention()

    html = renderer.render_webmention(wm)

    assert isinstance(html, str)
    assert '<div class="mention">' in html
    assert "A title" in html
    assert "source.example" in html
    assert "Jan 02, 2024 at 03:04" in html


def test_renderer_renders_template_string():
    renderer = WebmentionsRenderer()
    wm = _build_minimal_webmention(title="Hello")

    html = renderer.render_webmention(wm, template="<p>{{ mention.title }}</p>")

    assert html.strip() == "<p>Hello</p>"


def test_renderer_renders_template_object():
    renderer = WebmentionsRenderer()
    wm = _build_minimal_webmention(title="Hello")

    env = Environment(autoescape=False)
    template_obj = env.from_string("<span>{{ mention.title }}</span>")

    html = renderer.render_webmention(wm, template=template_obj)

    assert html.strip() == "<span>Hello</span>"


def test_renderer_renders_template_from_file_path(tmp_path):
    renderer = WebmentionsRenderer()
    wm = _build_minimal_webmention(title="From file")

    p = tmp_path / "t.html"
    p.write_text("<h1>{{ mention.title }}</h1>", encoding="utf-8")

    html = renderer.render_webmention(wm, template=p)

    assert html.strip() == "<h1>From file</h1>"


def test_renderer_renders_metadata_mf2_fields_from_json():
    renderer = WebmentionsRenderer()

    metadata = {
        "mf2": {
            "like_of": ["https://example.com/liked"],
        }
    }

    wm = _build_minimal_webmention(metadata=json.dumps(metadata))

    html = renderer.render_webmention(wm)

    assert "https://example.com/liked" in html
    assert "example.com" in html


def test_renderer_sanitizes_unsafe_urls_in_template():
    renderer = WebmentionsRenderer()

    wm = _build_minimal_webmention(
        source="javascript:alert(1)",
        author_url="javascript:alert(2)",
        author_photo="javascript:alert(3)",
        metadata=json.dumps(
            {
                "mf2": {
                    "like_of": ["javascript:alert(4)", "https://example.com/ok"],
                    "photo": ["javascript:alert(5)", "https://example.com/p.jpg"],
                }
            }
        ),
    )

    html = renderer.render_webmention(wm)

    assert "javascript:alert" not in html
    assert "https://example.com/ok" in html
    assert "https://example.com/p.jpg" in html


def test_renderer_raises_on_unknown_template_type():
    renderer = WebmentionsRenderer()
    wm = _build_minimal_webmention()
    bad_template = object()

    with pytest.raises(ValueError, match="Invalid template"):
        renderer.render_webmention(wm, template=bad_template)


def test_renderer_raises_if_jinja2_is_missing(monkeypatch):
    renderer = WebmentionsRenderer()
    wm = _build_minimal_webmention()

    monkeypatch.setattr(_renderer, "Environment", None)

    with pytest.raises(RuntimeError, match="Jinja2 is required"):
        renderer.render_webmention(wm)
