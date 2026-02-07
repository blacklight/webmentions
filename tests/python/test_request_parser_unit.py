import pytest

from webmentions._exceptions import WebmentionGone
from webmentions._model import WebmentionDirection, WebmentionType
from webmentions.handlers._parser import WebmentionsRequestParser


class _FakeResponse:
    def __init__(self, *, status_code: int = 200, text: str = "", exc: Exception | None = None):
        self.status_code = status_code
        self.text = text
        self._exc = exc

    def raise_for_status(self):
        if self._exc:
            raise self._exc


def test_parse_requires_source_and_target():
    parser = WebmentionsRequestParser()

    with pytest.raises(ValueError):
        parser.parse(None, "https://example.com/target")

    with pytest.raises(ValueError):
        parser.parse("https://example.com/source", None)


def test_parse_validates_target_domain_against_base_url(monkeypatch):
    def _get(*_, **__):
        raise AssertionError("requests.get should not be called when domain validation fails")

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    parser = WebmentionsRequestParser(base_url="https://example.com")
    with pytest.raises(ValueError, match="Target URL domain does not match server domain"):
        parser.parse("https://src.example.net/post", "https://evil.example.net/page")


@pytest.mark.parametrize("status_code", [404, 410])
def test_parse_404_410_raises_webmention_gone(monkeypatch, status_code):
    def _get(*_, **__):
        return _FakeResponse(status_code=status_code, text="")

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    parser = WebmentionsRequestParser()
    with pytest.raises(WebmentionGone, match="Source URL not found"):
        parser.parse("https://example.com/source", "https://example.com/target")


def test_parse_raises_for_status_propagates(monkeypatch):
    def _get(*_, **__):
        return _FakeResponse(status_code=500, text="", exc=RuntimeError("boom"))

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    parser = WebmentionsRequestParser()
    with pytest.raises(RuntimeError, match="boom"):
        parser.parse("https://example.com/source", "https://example.com/target")


def test_parse_requires_target_in_source_content(monkeypatch):
    target = "https://example.com/target"

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text="<html>no link</html>")

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    parser = WebmentionsRequestParser()
    with pytest.raises(WebmentionGone, match="Target URL not found in source content"):
        parser.parse("https://example.com/source", target)


def test_parse_populates_webmention_from_html_fallbacks(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = (
        "<html><head>"
        "<meta property=\"og:title\" content=\"Hello\"/>"
        "<meta name=\"author\" content=\"Ada\"/>"
        "<meta property=\"article:published_time\" content=\"2026-02-07T00:00:00+00:00\"/>"
        "<meta property=\"og:description\" content=\"   Some   description   \"/>"
        "</head><body>"
        f"<a href=\"{target}\">t</a>"
        "</body></html>"
    )

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    def _mf2_parse(*_, **__):
        return {"items": []}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)

    assert mention.source == source
    assert mention.target == target
    assert mention.direction == WebmentionDirection.IN
    assert mention.title == "Hello"
    assert mention.author_name == "Ada"
    assert mention.published is not None
    assert mention.published.isoformat() == "2026-02-07T00:00:00+00:00"
    assert mention.content == "   Some   description   "
    assert mention.excerpt == "Some description"


def test_parse_extracts_h_entry_and_sets_fields_and_type_like(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f"<html><body><a href=\"{target}\">t</a></body></html>"

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    h_entry = {
        "type": ["h-entry"],
        "properties": {
            "name": ["Entry title"],
            "published": ["2026-02-07T01:02:03+00:00"],
            "summary": ["Entry summary"],
            "content": [{"value": "Hello\n\nworld"}],
            "like-of": [target],
            "author": [
                {
                    "type": ["h-card"],
                    "properties": {
                        "name": ["Alice"],
                        "url": ["https://example.com/alice"],
                        "photo": ["https://example.com/alice.jpg"],
                    },
                }
            ],
        },
    }

    def _mf2_parse(*_, **__):
        return {"items": [h_entry]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)

    assert mention.title == "Entry title"
    assert mention.excerpt == "Entry summary"
    assert mention.content == "Hello\n\nworld"
    assert mention.author_name == "Alice"
    assert mention.author_url == "https://example.com/alice"
    assert mention.author_photo == "https://example.com/alice.jpg"
    assert mention.published is not None
    assert mention.published.isoformat() == "2026-02-07T01:02:03+00:00"
    assert mention.mention_type == WebmentionType.LIKE
    assert mention.mention_type_raw == "like-of"


def test_extract_h_entry_finds_child_entry(monkeypatch):
    child = {"type": ["h-entry"], "properties": {"name": ["Child"]}}

    def _mf2_parse(*_, **__):
        return {"items": [{"type": ["h-feed"], "children": [child]}]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    entry = WebmentionsRequestParser._extract_h_entry("<html></html>", "https://example.com/")
    assert entry == child


def test_extract_h_entry_returns_none_on_mf2_parse_error(monkeypatch):
    def _mf2_parse(*_, **__):
        raise RuntimeError("mf2 broken")

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    entry = WebmentionsRequestParser._extract_h_entry("<html></html>", "https://example.com/")
    assert entry is None


def test_first_str_handles_str_dict_list():
    assert WebmentionsRequestParser._first_str(None) is None
    assert WebmentionsRequestParser._first_str("x") == "x"
    assert WebmentionsRequestParser._first_str({"value": "v"}) == "v"
    assert WebmentionsRequestParser._first_str({"url": "u"}) == "u"
    assert WebmentionsRequestParser._first_str(["y"]) == "y"
    assert WebmentionsRequestParser._first_str([{"value": "z"}]) == "z"


def test_parse_generates_excerpt_from_content_when_missing(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f"<html><body><a href=\"{target}\">t</a></body></html>"

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    h_entry = {
        "type": ["h-entry"],
        "properties": {
            "content": [{"value": "   a\n\n   b   \n c   "}],
        },
    }

    def _mf2_parse(*_, **__):
        return {"items": [h_entry]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)

    assert mention.content == "   a\n\n   b   \n c   "
    assert mention.excerpt == "a b c"
