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


def bind_webmentions_endpoint(
    app: FastAPI, handler: "WebmentionsHandler", route: str = "/webmention"
):
    """
    Bind a FastAPI endpoint to process incoming Webmentions.

    :param app: The FastAPI application to bind the endpoint to.
    :param handler: The WebmentionsHandler to use for processing incoming Webmentions.
    :param route: The route to bind the endpoint to.
    """

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
