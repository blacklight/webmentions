from tornado.web import Application
from tornado.ioloop import IOLoop

from webmentions import WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage
from webmentions.server.adapters.tornado import bind_webmentions

app = Application()


def run_server(engine: str, address: str, port: int):
    """
    Run a local Tornado Webmentions example server.
    """
    handler = WebmentionsHandler(
        storage=init_db_storage(engine),
        # This should match the public base URL of your site
        base_url=f"http://{address}:{port}",
    )

    bind_webmentions(app, handler)
    app.listen(port, address=address)
    IOLoop.current().start()
