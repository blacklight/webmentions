from fastapi import FastAPI

from webmentions import WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage
from webmentions.server.adapters.fastapi import bind_webmentions

app = FastAPI()


def run_server(engine: str, address: str, port: int):
    """
    Run a local FastAPI Webmentions example server.
    """
    handler = WebmentionsHandler(
        storage=init_db_storage(engine),
        # This should match the public base URL of your site
        base_url=f"http://{address}:{port}",
    )

    bind_webmentions(app, handler)

    try:
        import uvicorn  # type: ignore
    except ImportError as e:  # pragma: no cover
        raise RuntimeError(
            "uvicorn is required to run the example server. "
            "Install it with: pip install uvicorn"
        ) from e

    uvicorn.run(app, host=address, port=port)
