import flask as flask_upstream  # pylint: disable=W0406

if getattr(flask_upstream, "__file__", None) == __file__:
    raise RuntimeError(
        "Local module name 'flask.py' is shadowing the upstream 'flask' dependency. "
        "Do not run this file directly; import it as 'webmentions.server.adapters.flask'."
    )

Flask = flask_upstream.Flask
Blueprint = flask_upstream.Blueprint
jsonify = flask_upstream.jsonify
request = flask_upstream.request

from ...handlers import WebmentionsHandler
from ..._exceptions import WebmentionException
from ._common import append_link_header, webmention_link_header_value


def _join_url_prefix(url_prefix: str | None, route: str) -> str:
    prefix = (url_prefix or "").rstrip("/")
    path = route if route.startswith("/") else f"/{route}"
    if not prefix:
        return path
    return f"{prefix}{path}"


def _install_webmentions_link_header_after_request(app: Flask):
    if not app.extensions.get("webmentions_link_header_after_request_installed", False):
        app.extensions["webmentions_link_header_after_request_installed"] = True

        @app.after_request
        def _webmentions_link_header_after_request(response):
            content_type = response.headers.get("Content-Type")
            if content_type is not None and content_type.split(";", 1)[
                0
            ].strip().startswith("text/"):
                existing = response.headers.get("Link")
                for endpoint in app.extensions.get("webmentions_endpoints", set()):
                    existing = append_link_header(
                        existing, webmention_link_header_value(endpoint)
                    )
                if existing is not None:
                    response.headers["Link"] = existing
            return response

        return _webmentions_link_header_after_request

    return None


def webmention_route(handler: WebmentionsHandler):
    source = request.form.get("source")
    target = request.form.get("target")

    try:
        handler.process_incoming_webmention(source, target)
    except WebmentionException as e:
        return jsonify({"error": str(e)}), 400

    return jsonify({"status": "ok"})


def bind_webmentions(
    app: Flask, handler: "WebmentionsHandler", route: str = "/webmentions"
):
    """
    Bind a Flask endpoint to process incoming Webmentions.

    :param app: The Flask application to bind the endpoint to.
    :param handler: The WebmentionsHandler to use for processing incoming Webmentions.
    :param route: The route to bind the endpoint to.
    """

    app.extensions.setdefault("webmentions_endpoints", set()).add(route)
    _install_webmentions_link_header_after_request(app)

    def _route():
        return webmention_route(handler)

    endpoint_name = f"webmention_{abs(hash(route))}"
    app.add_url_rule(route, endpoint=endpoint_name, view_func=_route, methods=["POST"])
    return route


def bind_webmentions_blueprint(
    handler: "WebmentionsHandler", route: str = "/webmentions"
) -> Blueprint:
    """
    Create a Flask Blueprint with a bound Webmentions endpoint.

    :param handler: The WebmentionsHandler to use for processing incoming Webmentions.
    :param route: The route to bind the endpoint to.
    :return: A Flask Blueprint instance.
    """

    bp = Blueprint("webmentions", __name__)

    @bp.record_once
    def _register(state):
        app = state.app
        effective_route = _join_url_prefix(state.url_prefix, route)
        app.extensions.setdefault("webmentions_endpoints", set()).add(effective_route)
        _install_webmentions_link_header_after_request(app)

    @bp.post(route)
    def _route():
        return webmention_route(handler)

    return bp
