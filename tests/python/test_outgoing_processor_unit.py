import pytest

from webmentions._model import ContentTextFormat, Webmention, WebmentionDirection
from webmentions.handlers._outgoing import OutgoingWebmentionsProcessor
from webmentions.storage._base import WebmentionsStorage


class _FakeStorage(WebmentionsStorage):
    def __init__(
        self,
        *,
        existing: list[Webmention] | None = None,
        retrieve_exc: Exception | None = None,
    ):
        self._existing = existing or []
        self._retrieve_exc = retrieve_exc
        self.sent: list[tuple[str, str]] = []
        self.deleted: list[tuple[str, str, WebmentionDirection]] = []

    def store_webmention(self, mention: Webmention):
        self.sent.append((mention.source, mention.target))

    def mark_sent(self, source: str, target: str) -> None:
        self.store_webmention(
            Webmention(source=source, target=target, direction=WebmentionDirection.OUT)
        )

    def delete_webmention(
        self, source: str, target: str, direction: WebmentionDirection
    ):
        self.deleted.append((source, target, direction))

    def retrieve_webmentions(self, resource: str, direction: WebmentionDirection):
        if self._retrieve_exc is not None:
            raise self._retrieve_exc
        assert direction == WebmentionDirection.OUT
        return [
            m
            for m in self._existing
            if m.source == resource and m.direction == direction
        ]


class _FakeResponse:
    def __init__(
        self,
        *,
        url: str,
        status_code: int = 200,
        text: str = "",
        body: bytes | None = None,
        headers: dict[str, str] | None = None,
        exc: Exception | None = None,
    ):
        self.url = url
        self.status_code = status_code
        self.text = text
        self.body = body
        self.headers = headers or {}
        self.encoding = "utf-8"
        self._exc = exc
        self.closed = False
        has_location = any(k.lower() == "location" for k in self.headers)
        self.is_redirect = has_location and status_code in (301, 302, 303, 307, 308)
        self.is_permanent_redirect = has_location and status_code in (301, 308)

    def raise_for_status(self):
        if self._exc is not None:
            raise self._exc

    def iter_content(self, chunk_size=1, decode_unicode=False):
        body = self.body if self.body is not None else self.text.encode(self.encoding)
        for i in range(0, len(body), chunk_size):
            yield body[i : i + chunk_size]

    def close(self):
        self.closed = True


class _SyncExecutor:
    def __init__(self, *_, **__):
        self.submitted: list[tuple[object, tuple, dict]] = []

    def submit(self, fn, /, *args, **kwargs):
        self.submitted.append((fn, args, kwargs))
        fn(*args, **kwargs)

    def shutdown(self, *_, **__):
        return None


def test_discover_endpoint_from_link_header(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)

    def _get(url, **_):
        assert url == "https://target.example/post"
        return _FakeResponse(
            url="https://target.example/post",
            headers={"Link": '</webmentions>; rel="webmention"'},
            text="",
        )

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.get", _get)

    assert (
        processor._discover_webmention_endpoint("https://target.example/post")
        == "https://target.example/webmentions"
    )


def test_discover_endpoint_from_html_tag(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)

    html = "<html><head><link rel='webmention' href='/wm'></head></html>"

    def _get(url, **_):
        assert url == "https://target.example/post"
        return _FakeResponse(url="https://target.example/post", headers={}, text=html)

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.get", _get)

    assert (
        processor._discover_webmention_endpoint("https://target.example/post")
        == "https://target.example/wm"
    )


def test_discover_endpoint_fetches_target_as_stream(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)

    fetched = {}

    def _get(url, **kwargs):
        fetched["url"] = url
        fetched["stream"] = kwargs.get("stream")
        return _FakeResponse(
            url="https://target.example/post",
            headers={"Content-Type": "text/html"},
            text="<html></html>",
        )

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.get", _get)

    assert (
        processor._discover_webmention_endpoint("https://target.example/post") is None
    )
    assert fetched == {"url": "https://target.example/post", "stream": True}


def test_discover_endpoint_from_link_header_on_non_html(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)

    def _get(url, **_):
        assert url == "https://target.example/audio.mp3"
        return _FakeResponse(
            url="https://target.example/audio.mp3",
            headers={
                "Content-Type": "audio/mpeg",
                "Link": '</webmentions>; rel="webmention"',
            },
            body=b"ID3binary",
        )

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.get", _get)

    assert (
        processor._discover_webmention_endpoint("https://target.example/audio.mp3")
        == "https://target.example/webmentions"
    )


def test_discover_endpoint_skips_non_html_response(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)
    response = _FakeResponse(
        url="https://target.example/model.zip",
        headers={"Content-Type": "application/zip", "Content-Length": "123"},
        body=b"PK\x03\x04binary zip payload",
    )

    def _get(url, **_):
        assert url == "https://target.example/model.zip"
        return response

    def _raise_bs4(*_, **__):
        raise AssertionError("BeautifulSoup should not parse non-HTML responses")

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.get", _get)
    monkeypatch.setattr("webmentions.handlers._outgoing.BeautifulSoup", _raise_bs4)

    assert (
        processor._discover_webmention_endpoint("https://target.example/model.zip")
        is None
    )
    assert response.closed


def test_discover_endpoint_skips_oversized_content_length(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage, max_discovery_response_bytes=16)

    def _get(url, **_):
        assert url == "https://target.example/huge"
        return _FakeResponse(
            url="https://target.example/huge",
            headers={"Content-Type": "text/html", "Content-Length": "17"},
            body=b"<link rel='webmention' href='/wm'>",
        )

    def _raise_bs4(*_, **__):
        raise AssertionError("BeautifulSoup should not parse oversized responses")

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.get", _get)
    monkeypatch.setattr("webmentions.handlers._outgoing.BeautifulSoup", _raise_bs4)

    assert (
        processor._discover_webmention_endpoint("https://target.example/huge") is None
    )


def test_discover_endpoint_skips_streaming_response_over_limit(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage, max_discovery_response_bytes=16)

    def _get(url, **_):
        assert url == "https://target.example/chunked"
        return _FakeResponse(
            url="https://target.example/chunked",
            headers={"Content-Type": "text/html"},
            body=b"x" * 17,
        )

    def _raise_bs4(*_, **__):
        raise AssertionError("BeautifulSoup should not parse oversized responses")

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.get", _get)
    monkeypatch.setattr("webmentions.handlers._outgoing.BeautifulSoup", _raise_bs4)

    assert (
        processor._discover_webmention_endpoint("https://target.example/chunked")
        is None
    )


def test_notify_target_no_endpoint_does_not_post(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)

    monkeypatch.setattr(processor, "_discover_webmention_endpoint", lambda *_: None)

    def _post(*_, **__):
        raise AssertionError(
            "requests.post should not be called if no endpoint is found"
        )

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.post", _post)

    processor._notify_target("https://source.example/s", "https://target.example/t")


def test_notify_target_raises_on_http_error(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)

    monkeypatch.setattr(
        processor,
        "_discover_webmention_endpoint",
        lambda *_: "https://target.example/wm",
    )

    def _post(url, **_):
        assert url == "https://target.example/wm"
        return _FakeResponse(url=url, status_code=400, exc=RuntimeError("bad"))

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.post", _post)

    with pytest.raises(RuntimeError, match="bad"):
        processor._notify_target("https://source.example/s", "https://target.example/t")


def test_notify_added_marks_sent_on_success_and_swallows_failures(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)

    monkeypatch.setattr(processor, "_notify_target", lambda *_: None)
    processor._notify_added("https://source.example/s", "https://target.example/t")

    assert storage.sent == [("https://source.example/s", "https://target.example/t")]

    storage2 = _FakeStorage()
    processor2 = OutgoingWebmentionsProcessor(storage2)

    def _boom(*_, **__):
        raise RuntimeError("boom")

    monkeypatch.setattr(processor2, "_notify_target", _boom)
    processor2._notify_added("https://source.example/s", "https://target.example/t")

    assert storage2.sent == []


def test_extract_targets_from_html_includes_media_src():
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage)

    html = """
    <html><body>
      <a href="https://target.example/linked">link</a>
      <audio controls src="https://target.example/song.mp3"></audio>
      <video src="https://target.example/clip.mp4"></video>
      <img src="/relative/image.png" />
      <source src="https://target.example/inner.ogg" />
    </body></html>
    """

    targets = processor._extract_targets(html, ContentTextFormat.HTML)

    assert "https://target.example/linked" in targets
    assert "https://target.example/song.mp3" in targets
    assert "https://target.example/clip.mp4" in targets
    assert "https://target.example/inner.ogg" in targets
    assert "/relative/image.png" not in targets


def test_process_outgoing_webmentions_computes_added_and_removed(monkeypatch):
    source = "https://source.example/post"

    existing = [
        Webmention(
            source=source,
            target="https://target.example/old",
            direction=WebmentionDirection.OUT,
        ),
        Webmention(
            source=source,
            target="https://target.example/keep",
            direction=WebmentionDirection.OUT,
        ),
    ]

    storage = _FakeStorage(existing=existing)
    processor = OutgoingWebmentionsProcessor(storage)

    monkeypatch.setattr(
        "webmentions.handlers._outgoing.ThreadPoolExecutor", _SyncExecutor
    )

    added: list[tuple[str, str]] = []
    removed: list[tuple[str, str]] = []

    monkeypatch.setattr(processor, "_notify_added", lambda s, t: added.append((s, t)))
    monkeypatch.setattr(
        processor, "_notify_removed", lambda s, t: removed.append((s, t))
    )

    text = "See https://target.example/keep and https://target.example/new"
    processor.process_outgoing_webmentions(
        source, text=text, text_format=ContentTextFormat.TEXT
    )

    assert added == [
        (source, "https://target.example/keep"),
        (source, "https://target.example/new"),
    ]
    assert removed == [(source, "https://target.example/old")]


def test_process_outgoing_webmentions_ignores_storage_errors(monkeypatch):
    source = "https://source.example/post"

    storage = _FakeStorage(retrieve_exc=RuntimeError("db down"))
    processor = OutgoingWebmentionsProcessor(storage)

    monkeypatch.setattr(
        "webmentions.handlers._outgoing.ThreadPoolExecutor", _SyncExecutor
    )

    added: list[tuple[str, str]] = []
    monkeypatch.setattr(processor, "_notify_added", lambda s, t: added.append((s, t)))

    processor.process_outgoing_webmentions(
        source,
        text="See https://target.example/a",
        text_format=ContentTextFormat.TEXT,
    )

    assert added == [(source, "https://target.example/a")]


def test_process_outgoing_webmentions_fetches_source_when_text_is_none(monkeypatch):
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage, user_agent="UA", http_timeout=3.0)

    monkeypatch.setattr(
        "webmentions.handlers._outgoing.ThreadPoolExecutor", _SyncExecutor
    )

    fetched = {}

    def _get(url, *, timeout, headers, allow_redirects, stream=False):
        fetched["url"] = url
        fetched["timeout"] = timeout
        fetched["headers"] = headers
        fetched["allow_redirects"] = allow_redirects
        fetched["stream"] = stream
        return _FakeResponse(
            url=url,
            text="<html><body><a href='https://target.example/t'>t</a></body></html>",
        )

    monkeypatch.setattr("webmentions.handlers._outgoing.requests.get", _get)

    calls: list[tuple[str, str]] = []
    monkeypatch.setattr(processor, "_notify_added", lambda s, t: calls.append((s, t)))

    processor.process_outgoing_webmentions("https://source.example/post")

    assert fetched == {
        "url": "https://source.example/post",
        "timeout": 3.0,
        "headers": {"User-Agent": "UA"},
        "allow_redirects": False,
        "stream": True,
    }
    assert calls == [("https://source.example/post", "https://target.example/t")]


def test_extract_targets_excludes_local_targets_when_enabled():
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(
        storage,
        base_urls=["https://me.example", "http://me.example"],
        exclude_local_targets=True,
    )

    text = (
        "See https://me.example/self and https://other.example/post "
        "and http://me.example/another"
    )
    targets = processor._extract_targets(text, ContentTextFormat.TEXT)

    assert targets == {"https://other.example/post"}


def test_extract_targets_keeps_local_targets_by_default():
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(
        storage,
        base_urls=["https://me.example"],
    )

    text = "See https://me.example/self and https://other.example/post"
    targets = processor._extract_targets(text, ContentTextFormat.TEXT)

    assert targets == {
        "https://me.example/self",
        "https://other.example/post",
    }


def test_extract_targets_exclude_local_without_base_urls_is_noop():
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(storage, exclude_local_targets=True)

    text = "See https://me.example/self and https://other.example/post"
    targets = processor._extract_targets(text, ContentTextFormat.TEXT)

    assert targets == {
        "https://me.example/self",
        "https://other.example/post",
    }


def test_extract_targets_exclude_local_matches_netloc_including_port():
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(
        storage,
        base_url="http://localhost:8080",
        exclude_local_targets=True,
    )

    text = "See http://localhost:8080/local and http://localhost:9000/other"
    targets = processor._extract_targets(text, ContentTextFormat.TEXT)

    assert targets == {"http://localhost:9000/other"}


def test_process_outgoing_webmentions_skips_local_targets(monkeypatch):
    source = "https://me.example/post"
    storage = _FakeStorage()
    processor = OutgoingWebmentionsProcessor(
        storage,
        base_urls=["https://me.example"],
        exclude_local_targets=True,
    )

    monkeypatch.setattr(
        "webmentions.handlers._outgoing.ThreadPoolExecutor", _SyncExecutor
    )

    added: list[tuple[str, str]] = []
    monkeypatch.setattr(processor, "_notify_added", lambda s, t: added.append((s, t)))

    text = "See https://me.example/self and https://other.example/post"
    processor.process_outgoing_webmentions(
        source, text=text, text_format=ContentTextFormat.TEXT
    )

    assert added == [(source, "https://other.example/post")]
