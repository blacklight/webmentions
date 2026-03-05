import json
from concurrent.futures import ThreadPoolExecutor
from typing import TYPE_CHECKING

import tornado.web as tornado_upstream  # pylint: disable=W0406
from tornado.ioloop import IOLoop

if getattr(tornado_upstream, "__file__", None) == __file__:
    raise RuntimeError(
        "Local module name 'tornado.py' is shadowing the upstream 'tornado' dependency. "
        "Do not run this file directly; import it as 'webmentions.server.adapters.tornado'."
    )

Application = tornado_upstream.Application
RequestHandler = tornado_upstream.RequestHandler

# isort: off
from ...handlers import WebmentionsHandler  # noqa: E402
from ..._exceptions import WebmentionException  # noqa: E402
from ..._model import WebmentionDirection  # noqa: E402
from ._common import append_link_header, webmention_link_header_value  # noqa: E402

if TYPE_CHECKING:
    from concurrent.futures import Executor
# isort: on


_default_executor: "Executor | None" = None


def _get_default_executor() -> "Executor":
    global _default_executor
    if _default_executor is None:
        _default_executor = ThreadPoolExecutor(max_workers=4)
    return _default_executor


def _make_webmention_handler(
    handler: "WebmentionsHandler",
    executor: "Executor | None" = None,
):
    """
    Create a Tornado RequestHandler class for processing Webmentions.
    """
    _executor = executor or _get_default_executor()

    class WebmentionHandler(RequestHandler):
        """
        Tornado RequestHandler wrapper for processing Webmentions.
        """

        def data_received(self, _):
            pass

        async def post(self):
            source = self.get_body_argument("source", None)
            target = self.get_body_argument("target", None)

            loop = IOLoop.current()
            try:
                await loop.run_in_executor(
                    _executor,
                    handler.process_incoming_webmention,
                    source,
                    target,
                )
            except WebmentionException as e:
                self.set_status(400)
                self.set_header("Content-Type", "application/json")
                self.write({"error": str(e)})
                return

            self.set_header("Content-Type", "application/json")
            self.write({"status": "ok"})

        async def get(self):
            resource = self.get_argument("resource", None)
            direction_raw = self.get_argument("direction", None)

            if not resource:
                self.set_status(400)
                self.set_header("Content-Type", "application/json")
                self.write({"error": "resource is required"})
                return

            if not direction_raw:
                self.set_status(400)
                self.set_header("Content-Type", "application/json")
                self.write({"error": "direction is required"})
                return

            try:
                direction = WebmentionDirection.from_raw(direction_raw)
            except ValueError:
                self.set_status(400)
                self.set_header("Content-Type", "application/json")
                self.write({"error": "invalid direction"})
                return

            loop = IOLoop.current()
            try:
                stored = await loop.run_in_executor(
                    _executor,
                    handler.storage.retrieve_webmentions,
                    resource,
                    direction,
                )
            except Exception as e:
                self.set_status(400)
                self.set_header("Content-Type", "application/json")
                self.write({"error": str(e)})
                return

            self.set_header("Content-Type", "application/json")
            self.write(json.dumps([wm.to_dict() for wm in stored]))

    return WebmentionHandler


def bind_webmentions(
    app: Application,
    handler: "WebmentionsHandler",
    route: str = "/webmentions",
    executor: "Executor | None" = None,
) -> str:
    """
    Bind a Tornado endpoint to process incoming Webmentions.

    :param app: The Tornado Application to bind the endpoint to.
    :param handler: The WebmentionsHandler to use for processing incoming Webmentions.
    :param route: The route to bind the endpoint to.
    :param executor: Optional executor for running blocking operations.
        If not provided, a default ThreadPoolExecutor with 4 workers is used.
    :return: The route that was bound.
    """
    if not hasattr(app, "_webmentions_endpoints"):
        app._webmentions_endpoints = set()

    app._webmentions_endpoints.add(route)

    WebmentionHandler = _make_webmention_handler(handler, executor)
    app.add_handlers(r".*", [(route, WebmentionHandler)])

    return route


def make_webmention_link_header_handler(
    base_handler: type,
    endpoints: set | None = None,
) -> type:
    """
    Create a RequestHandler subclass that injects the Webmention Link header.

    Use this to wrap your own handlers so they advertise the Webmention endpoint.

    :param base_handler: The base RequestHandler class to extend.
    :param endpoints: Set of Webmention endpoint routes to advertise.
        If not provided, you must set ``_webmentions_endpoints`` on the Application.
    :return: A new RequestHandler class with Link header injection.

    Example::

        from tornado.web import RequestHandler
        from webmentions.server.adapters.tornado import (
            bind_webmentions,
            make_webmention_link_header_handler,
        )

        # After binding webmentions
        bind_webmentions(app, handler)

        # Create a handler that advertises the endpoint
        LinkedHandler = make_webmention_link_header_handler(
            RequestHandler,
            endpoints={"/webmentions"},
        )

        class MyPageHandler(LinkedHandler):
            def get(self):
                self.set_header("Content-Type", "text/html")
                self.write("<html>...</html>")
    """

    class LinkedHandler(base_handler):  # pylint: disable=too-few-public-methods
        """
        RequestHandler subclass that injects the Webmention Link header.
        """

        def finish(self, chunk=None):
            _endpoints = endpoints
            if _endpoints is None:
                _endpoints = getattr(self.application, "_webmentions_endpoints", set())

            content_type = self._headers.get("Content-Type", "")
            if content_type.split(";", 1)[0].strip().startswith("text/"):
                existing = self._headers.get("Link")
                for endpoint in _endpoints:
                    existing = append_link_header(
                        existing, webmention_link_header_value(endpoint)
                    )
                if existing is not None:
                    self.set_header("Link", existing)
            return super().finish(chunk)

    return LinkedHandler
