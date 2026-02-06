import fastapi as fastapi_upstream  # pylint: disable=W0406

if getattr(fastapi_upstream, "__file__", None) == __file__:
    raise RuntimeError(
        "Local module name 'fastapi.py' is shadowing the upstream 'fastapi' dependency. "
        "Do not run this file directly; import it as 'webmentions.server.adapters.fastapi'."
    )

FastAPI = fastapi_upstream.FastAPI
Form = fastapi_upstream.Form
HTTPException = fastapi_upstream.HTTPException

from ...handlers import WebmentionsHandler
from ..._exceptions import WebmentionException
from ._common import append_link_header, webmention_link_header_value


def bind_webmentions(
    app: FastAPI, handler: "WebmentionsHandler", route: str = "/webmention"
):
    """
    Bind a FastAPI endpoint to process incoming Webmentions.

    :param app: The FastAPI application to bind the endpoint to.
    :param handler: The WebmentionsHandler to use for processing incoming Webmentions.
    :param route: The route to bind the endpoint to.
    """

    if not hasattr(app.state, "_webmentions_endpoints"):
        app.state._webmentions_endpoints = set()

    app.state._webmentions_endpoints.add(route)

    if not getattr(app.state, "_webmentions_link_header_middleware_installed", False):
        app.state._webmentions_link_header_middleware_installed = True

        @app.middleware("http")
        async def _webmentions_link_header_middleware(request, call_next):
            response = await call_next(request)
            content_type = response.headers.get("content-type")
            if content_type is not None and content_type.split(";", 1)[
                0
            ].strip().startswith("text/"):
                existing = response.headers.get("link")
                for endpoint in getattr(app.state, "_webmentions_endpoints", set()):
                    existing = append_link_header(
                        existing, webmention_link_header_value(endpoint)
                    )
                if existing is not None:
                    response.headers["link"] = existing
            return response

        return _webmentions_link_header_middleware

    @app.post(route)
    def webmention(
        source: str | None = Form(default=None),
        target: str | None = Form(default=None),
    ):
        try:
            handler.process_incoming_webmention(source, target)
        except WebmentionException as e:
            raise HTTPException(status_code=400, detail=str(e)) from e

        return {"status": "ok"}

    return webmention
