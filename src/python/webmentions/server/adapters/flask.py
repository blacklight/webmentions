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


def webmention_route(handler: WebmentionsHandler):
    source = request.form.get("source")
    target = request.form.get("target")

    try:
        handler.process_incoming_webmention(source, target)
    except WebmentionException as e:
        return jsonify({"error": str(e)}), 400

    return jsonify({"status": "ok"})


def bind_webmentions_endpoint(
    app: Flask, handler: "WebmentionsHandler", route: str = "/webmention"
):
    """
    Bind a Flask endpoint to process incoming Webmentions.

    :param app: The Flask application to bind the endpoint to.
    :param handler: The WebmentionsHandler to use for processing incoming Webmentions.
    :param route: The route to bind the endpoint to.
    """

    def _route():
        return webmention_route(handler)

    endpoint_name = f"webmention_{abs(hash(route))}"
    app.add_url_rule(route, endpoint=endpoint_name, view_func=_route, methods=["POST"])
    return route


def bind_webmentions_blueprint(
    handler: "WebmentionsHandler", route: str = "/webmention"
) -> Blueprint:
    """
    Create a Flask Blueprint with a bound Webmentions endpoint.

    :param handler: The WebmentionsHandler to use for processing incoming Webmentions.
    :param route: The route to bind the endpoint to.
    :return: A Flask Blueprint instance.
    """

    bp = Blueprint("webmentions", __name__)

    @bp.post(route)
    def _route():
        return webmention_route(handler)

    return bp
