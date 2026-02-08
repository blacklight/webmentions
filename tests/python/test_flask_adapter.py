import pytest


pytest.importorskip("flask")

from webmentions._exceptions import WebmentionException
from webmentions.server.adapters.flask import (
    Flask,
    _join_url_prefix,
    bind_webmentions,
    bind_webmentions_blueprint,
)


class _Handler:
    def __init__(self, *, exc: Exception | None = None):
        self.exc = exc
        self.calls: list[tuple[str | None, str | None]] = []

    def process_incoming_webmention(self, source: str | None, target: str | None):
        self.calls.append((source, target))
        if self.exc is not None:
            raise self.exc


def test_join_url_prefix():
    assert _join_url_prefix(None, "/webmentions") == "/webmentions"
    assert _join_url_prefix("", "/webmentions") == "/webmentions"

    assert _join_url_prefix("/api", "/webmentions") == "/api/webmentions"
    assert _join_url_prefix("/api/", "/webmentions") == "/api/webmentions"

    assert _join_url_prefix("/api", "webmentions") == "/api/webmentions"
    assert _join_url_prefix("/api/", "webmentions") == "/api/webmentions"


def test_bind_webmentions_processes_post_ok():
    handler = _Handler()
    app = Flask("test")
    bind_webmentions(app, handler, route="/webmentions")

    client = app.test_client()
    resp = client.post(
        "/webmentions",
        data={"source": "https://example.com/s", "target": "https://example.com/t"},
    )

    assert resp.status_code == 200
    assert resp.get_json() == {"status": "ok"}
    assert handler.calls == [("https://example.com/s", "https://example.com/t")]


def test_bind_webmentions_processes_post_error():
    exc = WebmentionException("s", "t", "bad")
    handler = _Handler(exc=exc)
    app = Flask("test")
    bind_webmentions(app, handler, route="/webmentions")

    client = app.test_client()
    resp = client.post("/webmentions", data={"source": "s", "target": "t"})

    assert resp.status_code == 400
    body = resp.get_json()
    assert isinstance(body, dict)
    assert "error" in body


def test_after_request_appends_webmention_link_header_for_text_responses():
    app = Flask("test")
    bind_webmentions(app, _Handler(), route="/webmentions")

    @app.get("/page")
    def _page():
        return "hello", 200, {"Content-Type": "text/plain; charset=utf-8"}

    client = app.test_client()
    resp = client.get("/page")

    link = resp.headers.get("Link")
    assert link is not None
    assert 'rel="webmention"' in link
    assert "</webmentions>" in link


def test_after_request_does_not_touch_non_text_responses():
    app = Flask("test")
    bind_webmentions(app, _Handler(), route="/webmentions")

    @app.get("/data")
    def _data():
        return app.response_class("{}", mimetype="application/json")

    client = app.test_client()
    resp = client.get("/data")

    assert resp.headers.get("Link") is None


def test_after_request_deduplicates_existing_link_header():
    app = Flask("test")
    bind_webmentions(app, _Handler(), route="/webmentions")

    @app.get("/page")
    def _page():
        return (
            "hello",
            200,
            {"Content-Type": "text/plain", "Link": '</webmentions>; rel="webmention"'},
        )

    client = app.test_client()
    resp = client.get("/page")

    assert resp.headers.get("Link") == '</webmentions>; rel="webmention"'


def test_bind_webmentions_blueprint_respects_url_prefix_and_updates_extensions_and_headers():
    handler = _Handler()
    bp = bind_webmentions_blueprint(handler, route="/webmentions")

    app = Flask("test")

    @app.get("/page")
    def _page():
        return "hello", 200, {"Content-Type": "text/plain"}

    app.register_blueprint(bp, url_prefix="/api")

    assert "/api/webmentions" in app.extensions.get("webmentions_endpoints", set())

    client = app.test_client()
    resp = client.get("/page")
    link = resp.headers.get("Link")
    assert link is not None
    assert "</api/webmentions>" in link

    resp2 = client.post(
        "/api/webmentions",
        data={"source": "https://example.com/s", "target": "https://example.com/t"},
    )
    assert resp2.status_code == 200
    assert handler.calls == [("https://example.com/s", "https://example.com/t")]
