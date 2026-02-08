import pytest

from webmentions import (
    Webmention,
    WebmentionDirection,
    WebmentionGone,
    WebmentionType,
)
from webmentions.handlers._parser import WebmentionsRequestParser


class _FakeResponse:
    def __init__(
        self, *, status_code: int = 200, text: str = "", exc: Exception | None = None
    ):
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
        raise AssertionError(
            "requests.get should not be called when domain validation fails"
        )

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    parser = WebmentionsRequestParser(base_url="https://example.com")
    with pytest.raises(
        ValueError, match="Target URL domain does not match server domain"
    ):
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


def test_parse_accepts_exact_target_url_in_href(monkeypatch):
    target = "https://example.com/target"

    def _get(*_, **__):
        return _FakeResponse(
            status_code=200,
            text=f'<html><body><a href="{target}">t</a></body></html>',
        )

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    def _mf2_parse(*_, **__):
        return {"items": []}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse("https://example.com/source", target)
    assert mention.target == target


def test_parse_accepts_exact_target_url_in_src(monkeypatch):
    target = "https://example.com/target"

    def _get(*_, **__):
        return _FakeResponse(
            status_code=200,
            text=f'<html><body><img src="{target}"/></body></html>',
        )

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    def _mf2_parse(*_, **__):
        return {"items": []}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse("https://example.com/source", target)
    assert mention.target == target


def test_parse_populates_webmention_from_html_fallbacks(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = (
        "<html><head>"
        '<meta property="og:title" content="Hello"/>'
        '<meta name="author" content="Ada"/>'
        '<meta property="article:published_time" content="2026-02-07T00:00:00+00:00"/>'
        '<meta property="og:description" content="   Some   description   "/>'
        "</head><body>"
        f'<a href="{target}">t</a>'
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
    html = f'<html><body><a href="{target}">t</a></body></html>'

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


@pytest.mark.parametrize(
    ("props", "expected_type", "expected_raw"),
    [
        (
            {"bookmark-of": ["https://example.com/target"]},
            WebmentionType.BOOKMARK,
            "bookmark-of",
        ),
        (
            {"follow-of": ["https://example.com/target"]},
            WebmentionType.FOLLOW,
            "follow-of",
        ),
        ({"rsvp": ["yes"]}, WebmentionType.RSVP, "rsvp"),
    ],
)
def test_parse_sets_additional_response_types(
    monkeypatch, props, expected_type, expected_raw
):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f'<html><body><a href="{target}">t</a></body></html>'

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    h_entry = {
        "type": ["h-entry"],
        "properties": {
            **props,
        },
    }

    def _mf2_parse(*_, **__):
        return {"items": [h_entry]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)

    assert mention.mention_type == expected_type
    assert mention.mention_type_raw == expected_raw


def test_parse_does_not_infer_mention_type_if_already_set(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f'<html><body><a href="{target}">t</a></body></html>'

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    h_entry = {"type": ["h-entry"], "properties": {"like-of": [target]}}

    def _mf2_parse(*_, **__):
        return {"items": [h_entry]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)
    mention.mention_type = WebmentionType.LIKE
    mention.mention_type_raw = "like-of"

    WebmentionsRequestParser._fill_from_h_entry(mention, h_entry, target)
    assert mention.mention_type == WebmentionType.LIKE
    assert mention.mention_type_raw == "like-of"


def test_parse_extracts_mf2_comments_into_metadata(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f'<html><body><a href="{target}">t</a></body></html>'

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    comment = {
        "type": ["h-cite"],
        "properties": {
            "url": ["https://commenter.example/c12"],
            "published": ["2026-02-07T01:02:03+00:00"],
            "content": [{"value": "Nice post"}],
            "author": [
                {
                    "type": ["h-card"],
                    "properties": {
                        "name": ["Jane"],
                        "url": ["https://commenter.example"],
                        "photo": ["https://commenter.example/jane.jpg"],
                    },
                }
            ],
        },
    }

    h_entry = {
        "type": ["h-entry"],
        "properties": {
            "comment": [comment],
        },
    }

    def _mf2_parse(*_, **__):
        return {"items": [h_entry]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)

    assert "comments" in mention.metadata
    assert len(mention.metadata["comments"]) == 1
    assert mention.metadata["comments"][0]["url"] == "https://commenter.example/c12"
    assert mention.metadata["comments"][0]["content"] == "Nice post"
    assert mention.metadata["comments"][0]["author"]["name"] == "Jane"


def test_parse_extracts_mf2_media_and_location_fields(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f'<html><body><a href="{target}">t</a></body></html>'

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    location = {
        "type": ["h-geo"],
        "properties": {
            "name": ["Somewhere"],
            "latitude": ["12.34"],
            "longitude": ["56.78"],
            "url": ["https://example.com/places/1"],
        },
    }

    h_entry = {
        "type": ["h-entry"],
        "properties": {
            "photo": ["https://example.com/p.jpg"],
            "featured": [{"value": "https://example.com/featured.jpg"}],
            "video": ["https://example.com/v.mp4"],
            "audio": ["https://example.com/a.mp3"],
            "location": [location],
        },
    }

    def _mf2_parse(*_, **__):
        return {"items": [h_entry]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)

    mf2 = mention.metadata["mf2"]
    assert mf2["photo"] == ["https://example.com/p.jpg"]
    assert mf2["featured"] == [{"value": "https://example.com/featured.jpg"}]
    assert mf2["video"] == ["https://example.com/v.mp4"]
    assert mf2["audio"] == ["https://example.com/a.mp3"]
    assert mf2["photo_url"] == "https://example.com/p.jpg"
    assert mf2["featured_url"] == "https://example.com/featured.jpg"
    assert mf2["video_url"] == "https://example.com/v.mp4"
    assert mf2["audio_url"] == "https://example.com/a.mp3"
    assert mf2["location"] == [location]
    assert mf2["location_normalized"] == {
        "type": ["h-geo"],
        "name": "Somewhere",
        "url": "https://example.com/places/1",
        "latitude": "12.34",
        "longitude": "56.78",
    }


def test_extract_h_entry_finds_child_entry(monkeypatch):
    child = {"type": ["h-entry"], "properties": {"name": ["Child"]}}

    def _mf2_parse(*_, **__):
        return {"items": [{"type": ["h-feed"], "children": [child]}]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    entry = WebmentionsRequestParser._extract_h_entry(
        "<html></html>", "https://example.com/"
    )
    assert entry == child


def test_extract_h_entry_returns_none_on_mf2_parse_error(monkeypatch):
    def _mf2_parse(*_, **__):
        raise RuntimeError("mf2 broken")

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    entry = WebmentionsRequestParser._extract_h_entry(
        "<html></html>", "https://example.com/"
    )
    assert entry is None


def test_first_str_handles_str_dict_list():
    assert WebmentionsRequestParser._first_str(None) is None
    assert WebmentionsRequestParser._first_str("x") == "x"
    assert WebmentionsRequestParser._first_str({"value": "v"}) == "v"
    assert WebmentionsRequestParser._first_str({"url": "u"}) == "u"
    assert WebmentionsRequestParser._first_str(["y"]) == "y"
    assert WebmentionsRequestParser._first_str([{"value": "z"}]) == "z"


def test_first_str_handles_unexpected_shapes():
    assert WebmentionsRequestParser._first_str({"nope": "x"}) is None
    assert WebmentionsRequestParser._first_str([123]) is None
    assert WebmentionsRequestParser._first_str([{"nope": "x"}]) is None


def test_extract_author_handles_string_and_unknown(monkeypatch):
    assert WebmentionsRequestParser._extract_author(
        {"author": "https://example.com"}
    ) == (
        None,
        "https://example.com",
        None,
    )
    assert WebmentionsRequestParser._extract_author({"author": [123]}) == (
        None,
        None,
        None,
    )


def test_parse_generates_excerpt_from_content_when_missing(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f'<html><body><a href="{target}">t</a></body></html>'

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


def test_parse_excerpt_generation_can_result_in_none(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f'<html><body><a href="{target}">t</a></body></html>'

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    h_entry = {
        "type": ["h-entry"],
        "properties": {
            "content": [{"value": "    \n\n   \n"}],
        },
    }

    def _mf2_parse(*_, **__):
        return {"items": [h_entry]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)
    assert mention.content is not None
    assert mention.excerpt is None


def test_source_mentions_target_falls_back_when_bs4_raises(monkeypatch):
    target = "https://example.com/target"

    def _raise_bs4(*_, **__):
        raise RuntimeError("bs4 broken")

    monkeypatch.setattr("webmentions.handlers._parser.BeautifulSoup", _raise_bs4)
    assert WebmentionsRequestParser._source_mentions_target(
        f"prefix {target} suffix", target
    )


def test_source_mentions_target_returns_false_when_no_exact_match_in_href_or_src():
    target = "https://example.com/target"
    html = (
        "<html><body>"
        '<a href="https://example.com/other">x</a>'
        '<img src="https://example.com/other2"/>'
        "</body></html>"
    )
    assert WebmentionsRequestParser._source_mentions_target(html, target) is False


@pytest.mark.parametrize(
    ("props", "expected_type", "expected_raw"),
    [
        (
            {"repost-of": ["https://example.com/target"]},
            WebmentionType.REPOST,
            "repost-of",
        ),
        (
            {"in-reply-to": ["https://example.com/target"]},
            WebmentionType.REPLY,
            "in-reply-to",
        ),
        ({}, WebmentionType.MENTION, "mention"),
    ],
)
def test_parse_sets_repost_reply_and_mention_types(
    monkeypatch, props, expected_type, expected_raw
):
    source = "https://example.com/source"
    target = "https://example.com/target"
    html = f'<html><body><a href="{target}">t</a></body></html>'

    def _get(*_, **__):
        return _FakeResponse(status_code=200, text=html)

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    h_entry = {"type": ["h-entry"], "properties": {**props}}

    def _mf2_parse(*_, **__):
        return {"items": [h_entry]}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)
    assert mention.mention_type == expected_type
    assert mention.mention_type_raw == expected_raw


def test_fill_from_h_entry_does_not_overwrite_existing_author_fields():
    mention = Webmention(
        source="https://example.com/source",
        target="https://example.com/target",
        direction=WebmentionDirection.IN,
        author_name="Already",
    )

    h_entry = {
        "type": ["h-entry"],
        "properties": {
            "author": [
                {
                    "type": ["h-card"],
                    "properties": {"name": ["New"], "url": ["https://new.example"]},
                }
            ]
        },
    }

    WebmentionsRequestParser._fill_from_h_entry(mention, h_entry, mention.target)
    assert mention.author_name == "Already"


def test_fill_from_h_entry_content_and_published_edge_cases():
    mention = Webmention(
        source="https://example.com/source",
        target="https://example.com/target",
        direction=WebmentionDirection.IN,
    )

    h_entry = {
        "type": ["h-entry"],
        "properties": {
            "published": [""],
            "content": ["Hello"],
        },
    }

    WebmentionsRequestParser._fill_from_h_entry(mention, h_entry, mention.target)
    assert mention.published is None
    assert mention.content == "Hello"


def test_extract_comments_handles_non_list_and_mixed_shapes():
    assert WebmentionsRequestParser._extract_comments({"x": 1}) == []

    extracted = WebmentionsRequestParser._extract_comments(
        [
            "https://example.com/c1",
            123,
            {
                "type": ["h-cite"],
                "properties": {"content": ["Hello"]},
            },
            {
                "type": ["h-cite"],
                "properties": {"content": []},
            },
        ]
    )
    assert extracted[0]["url"] == "https://example.com/c1"
    assert extracted[1]["content"] == "Hello"
    assert extracted[2]["content"] is None


def test_extract_comments_supports_string_content_item():
    extracted = WebmentionsRequestParser._extract_comments(
        [
            {
                "type": ["h-cite"],
                "properties": {"content": ["Hello from str"]},
            }
        ]
    )
    assert extracted[0]["content"] == "Hello from str"


def test_parse_html_title_fallbacks_twitter_then_title_tag(monkeypatch):
    source = "https://example.com/source"
    target = "https://example.com/target"

    def _get(*_, **__):
        return _FakeResponse(
            status_code=200,
            text=(
                "<html><head>"
                '<meta name="twitter:title" content="Tw"/>'
                "</head><body>"
                f'<a href="{target}">t</a>'
                "</body></html>"
            ),
        )

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get)

    def _mf2_parse(*_, **__):
        return {"items": []}

    monkeypatch.setattr("webmentions.handlers._parser.mf2py.parse", _mf2_parse)

    parser = WebmentionsRequestParser()
    mention = parser.parse(source, target)
    assert mention.title == "Tw"

    def _get2(*_, **__):
        return _FakeResponse(
            status_code=200,
            text=(
                "<html><head><title> Plain </title></head><body>"
                f'<a href="{target}">t</a>'
                "</body></html>"
            ),
        )

    monkeypatch.setattr("webmentions.handlers._parser.requests.get", _get2)

    mention2 = parser.parse(source, target)
    assert mention2.title == "Plain"
