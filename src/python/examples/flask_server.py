from flask import Flask

from webmentions import WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage
from webmentions.server.adapters.flask import bind_webmentions

app = Flask(__name__)


def run_server(engine: str, address: str, port: int):
    """
    Run a local Flask Webmentions example server.
    """
    handler = WebmentionsHandler(
        storage=init_db_storage(engine),
        # This should match the public base URL of your site
        base_url=f"http://{address}:{port}",
    )

    bind_webmentions(app, handler)
    app.run(host=address, port=port)
