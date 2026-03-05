import json

import pytest

pytest.importorskip("tornado")

# isort: off
from tornado.testing import AsyncHTTPTestCase  # noqa: E402
from tornado.web import Application, RequestHandler  # noqa: E402

from webmentions._exceptions import WebmentionException  # noqa: E402
from webmentions.server.adapters.tornado import (  # noqa: E402
    bind_webmentions,
    make_webmention_link_header_handler,
)

# isort: on


class _Handler:
    def __init__(self, *, exc: Exception | None = None, stored: list | None = None):
        self.exc = exc
        self.calls: list[tuple[str | None, str | None]] = []
        self.storage = _Storage(stored or [])

    def process_incoming_webmention(self, source: str | None, target: str | None):
        self.calls.append((source, target))
        if self.exc is not None:
            raise self.exc


class _Storage:
    def __init__(self, stored: list):
        self._stored = stored

    def retrieve_webmentions(self, resource, direction):
        return self._stored


class _MockWebmention:
    def __init__(self, source: str, target: str):
        self.source = source
        self.target = target

    def to_dict(self):
        return {"source": self.source, "target": self.target}


class TestBindWebmentionsPost(AsyncHTTPTestCase):
    def get_app(self):
        self.handler = _Handler()
        app = Application()
        bind_webmentions(app, self.handler, route="/webmentions")
        return app

    def test_processes_post_ok(self):
        response = self.fetch(
            "/webmentions",
            method="POST",
            body="source=https://example.com/s&target=https://example.com/t",
        )

        assert response.code == 200
        body = json.loads(response.body)
        assert body == {"status": "ok"}
        assert self.handler.calls == [
            ("https://example.com/s", "https://example.com/t")
        ]

    def test_processes_post_error(self):
        self.handler.exc = WebmentionException("s", "t", "bad")

        response = self.fetch(
            "/webmentions",
            method="POST",
            body="source=s&target=t",
        )

        assert response.code == 400
        body = json.loads(response.body)
        assert "error" in body


class TestBindWebmentionsGet(AsyncHTTPTestCase):
    def get_app(self):
        stored = [
            _MockWebmention("https://a.example/1", "https://b.example/page"),
        ]
        self.handler = _Handler(stored=stored)
        app = Application()
        bind_webmentions(app, self.handler, route="/webmentions")
        return app

    def test_get_requires_resource(self):
        response = self.fetch("/webmentions?direction=in", method="GET")
        assert response.code == 400
        body = json.loads(response.body)
        assert body == {"error": "resource is required"}

    def test_get_requires_direction(self):
        response = self.fetch("/webmentions?resource=https://example.com", method="GET")
        assert response.code == 400
        body = json.loads(response.body)
        assert body == {"error": "direction is required"}

    def test_get_invalid_direction(self):
        response = self.fetch(
            "/webmentions?resource=https://example.com&direction=invalid", method="GET"
        )
        assert response.code == 400
        body = json.loads(response.body)
        assert body == {"error": "invalid direction"}

    def test_get_returns_stored_webmentions(self):
        response = self.fetch(
            "/webmentions?resource=https://b.example/page&direction=in", method="GET"
        )
        assert response.code == 200
        body = json.loads(response.body)
        assert body == [
            {"source": "https://a.example/1", "target": "https://b.example/page"}
        ]


class TestLinkHeaderMixin(AsyncHTTPTestCase):
    def get_app(self):
        self.handler = _Handler()
        app = Application()
        bind_webmentions(app, self.handler, route="/webmentions")

        LinkedHandler = make_webmention_link_header_handler(
            RequestHandler, endpoints={"/webmentions"}
        )

        class PageHandler(LinkedHandler):
            def get(self):
                self.set_header("Content-Type", "text/html; charset=utf-8")
                self.write("<html><body>hello</body></html>")

        class JsonHandler(LinkedHandler):
            def get(self):
                self.set_header("Content-Type", "application/json")
                self.write("{}")

        class ExistingLinkHandler(LinkedHandler):
            def get(self):
                self.set_header("Content-Type", "text/plain")
                self.set_header("Link", '</webmentions>; rel="webmention"')
                self.write("hello")

        app.add_handlers(r".*", [("/page", PageHandler)])
        app.add_handlers(r".*", [("/data", JsonHandler)])
        app.add_handlers(r".*", [("/existing", ExistingLinkHandler)])
        return app

    def test_appends_link_header_for_text_responses(self):
        response = self.fetch("/page", method="GET")
        assert response.code == 200
        link = response.headers.get("Link")
        assert link is not None
        assert 'rel="webmention"' in link
        assert "</webmentions>" in link

    def test_does_not_touch_non_text_responses(self):
        response = self.fetch("/data", method="GET")
        assert response.code == 200
        assert response.headers.get("Link") is None

    def test_deduplicates_existing_link_header(self):
        response = self.fetch("/existing", method="GET")
        assert response.code == 200
        link = response.headers.get("Link")
        assert link == '</webmentions>; rel="webmention"'


class TestLinkHeaderFromApp(AsyncHTTPTestCase):
    def get_app(self):
        self.handler = _Handler()
        app = Application()
        bind_webmentions(app, self.handler, route="/webmentions")

        LinkedHandler = make_webmention_link_header_handler(RequestHandler)

        class PageHandler(LinkedHandler):
            def get(self):
                self.set_header("Content-Type", "text/html")
                self.write("<html></html>")

        app.add_handlers(r".*", [("/page", PageHandler)])
        return app

    def test_uses_endpoints_from_app(self):
        response = self.fetch("/page", method="GET")
        assert response.code == 200
        link = response.headers.get("Link")
        assert link is not None
        assert "</webmentions>" in link
