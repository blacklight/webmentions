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
        headers: dict[str, str] | None = None,
        exc: Exception | None = None,
    ):
        self.url = url
        self.status_code = status_code
        self.text = text
        self.headers = headers or {}
        self._exc = exc

    def raise_for_status(self):
        if self._exc is not None:
            raise self._exc


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

    def _get(url, *, timeout, headers, allow_redirects):
        fetched["url"] = url
        fetched["timeout"] = timeout
        fetched["headers"] = headers
        fetched["allow_redirects"] = allow_redirects
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
        "allow_redirects": True,
    }
    assert calls == [("https://source.example/post", "https://target.example/t")]
