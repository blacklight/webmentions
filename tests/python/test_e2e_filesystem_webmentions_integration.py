import os
import queue
import socket
import threading
import time
from contextlib import contextmanager
from pathlib import Path

import pytest
import requests

from webmentions import WebmentionDirection
from webmentions._model import Webmention
from webmentions.handlers import WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage
from webmentions.storage.adapters.file import FileSystemMonitor


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def _wait_for(predicate, *, timeout: float = 15.0, interval: float = 0.2):
    t0 = time.monotonic()
    last_exc: Exception | None = None
    while time.monotonic() - t0 < timeout:
        try:
            if predicate():
                return
        except Exception as e:  # pragma: no cover
            last_exc = e
        time.sleep(interval)
    if last_exc:
        raise AssertionError(f"Condition not met within {timeout}s") from last_exc
    raise AssertionError(f"Condition not met within {timeout}s")


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _delete(path: Path) -> None:
    try:
        path.unlink()
    except FileNotFoundError:
        return


@contextmanager
def _run_flask_app(app, host: str, port: int):
    pytest.importorskip("flask")
    from werkzeug.serving import make_server

    server = make_server(host, port, app)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield
    finally:
        server.shutdown()
        thread.join(timeout=5)


@contextmanager
def _run_fastapi_app(app, host: str, port: int):
    pytest.importorskip("fastapi")
    pytest.importorskip("uvicorn")
    import uvicorn

    config = uvicorn.Config(
        app,
        host=host,
        port=port,
        log_level="warning",
        access_log=False,
    )
    server = uvicorn.Server(config)

    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    try:
        yield
    finally:
        server.should_exit = True
        thread.join(timeout=10)


def _make_file_to_url_mapper(root_dir: Path, base_url: str):
    root_dir = root_dir.resolve()

    def _mapper(path: str) -> str:
        rel = os.path.relpath(Path(path).resolve(), root_dir)
        rel = rel.replace(os.path.sep, "/")
        return f"{base_url}/content/{rel}"

    return _mapper


def _assert_in_mentions(storage, *, target_url: str, expected_sources: set[str]):
    mentions = storage.retrieve_webmentions(
        target_url, direction=WebmentionDirection.IN
    )
    assert {m.source for m in mentions} == expected_sources
    return mentions


def _get_webmentions_json(
    *, base_url: str, resource: str, direction: WebmentionDirection
) -> list[dict]:
    resp = requests.get(
        f"{base_url}/webmentions",
        params={"resource": resource, "direction": direction.value},
        timeout=2,
    )
    assert resp.status_code == 200
    data = resp.json()
    assert isinstance(data, list)
    return data


def _assert_get_mentions(
    *, base_url: str, target_url: str, expected_sources: set[str]
) -> list[dict]:
    mentions = _get_webmentions_json(
        base_url=base_url, resource=target_url, direction=WebmentionDirection.IN
    )
    assert {m.get("source") for m in mentions} == expected_sources
    for m in mentions:
        assert m.get("target") == target_url
        assert m.get("direction") == WebmentionDirection.IN.value
    return mentions


def _wait_for_received(
    q: "queue.Queue[Webmention]",
    *,
    source_url: str,
    target_url: str,
    direction: WebmentionDirection,
    timeout: float = 8.0,
) -> None:
    t0 = time.monotonic()
    while True:
        remaining = timeout - (time.monotonic() - t0)
        if remaining <= 0:
            raise AssertionError(f"No callback received within {timeout}s")
        try:
            mention = q.get(timeout=min(0.25, remaining))
        except queue.Empty:
            continue
        if (
            mention.source == source_url
            and mention.target == target_url
            and mention.direction == direction
        ):
            return


@pytest.mark.integration
@pytest.mark.parametrize("adapter", ["flask", "fastapi"])
def test_e2e_filesystem_webmentions_two_servers_db_storage(adapter, tmp_path):
    host = "127.0.0.1"
    port_a = _pick_free_port()
    port_b = _pick_free_port()

    base_a = f"http://{host}:{port_a}"
    base_b = f"http://{host}:{port_b}"

    root_a = tmp_path / "srv_a"
    root_b = tmp_path / "srv_b"
    root_a.mkdir(parents=True, exist_ok=True)
    root_b.mkdir(parents=True, exist_ok=True)

    db_a = init_db_storage(engine=f"sqlite:///{tmp_path / 'a.sqlite'}")
    db_b = init_db_storage(engine=f"sqlite:///{tmp_path / 'b.sqlite'}")

    received_b: "queue.Queue[Webmention]" = queue.Queue()

    handler_a = WebmentionsHandler(storage=db_a, base_url=base_a)
    handler_b = WebmentionsHandler(
        storage=db_b,
        base_url=base_b,
        on_mention_processed=lambda m: received_b.put(m),
        on_mention_deleted=lambda m: received_b.put(m),
    )

    fs_a = FileSystemMonitor(
        handler_a,
        root_dir=str(root_a),
        file_to_url_mapper=_make_file_to_url_mapper(root_a, base_a),
        throttle_seconds=0.1,
    )
    fs_b = FileSystemMonitor(
        handler_b,
        root_dir=str(root_b),
        file_to_url_mapper=_make_file_to_url_mapper(root_b, base_b),
        throttle_seconds=0.1,
    )

    source_file = root_a / "source.html"
    target_file = root_b / "target.html"

    target_url = f"{base_b}/content/target.html"
    source_url = f"{base_a}/content/source.html"

    _write(
        target_file,
        "<html><head><title>Target</title></head><body>target</body></html>",
    )
    _write(
        source_file,
        "<html><head><title>Source</title></head><body>initial</body></html>",
    )

    if adapter == "flask":
        flask = pytest.importorskip("flask")
        from webmentions.server.adapters.flask import Flask, bind_webmentions

        app_a = Flask("srv_a")
        app_b = Flask("srv_b")

        @app_a.get("/health")
        def _health_a():
            return "ok"

        @app_b.get("/health")
        def _health_b():
            return "ok"

        @app_a.get("/content/<path:p>")
        def _content_a(p: str):
            path = (root_a / p).resolve()
            if not str(path).startswith(str(root_a.resolve())) or not path.is_file():
                return ("not found", 404)
            return flask.Response(
                path.read_text(encoding="utf-8"), mimetype="text/html"
            )

        @app_b.get("/content/<path:p>")
        def _content_b(p: str):
            path = (root_b / p).resolve()
            if not str(path).startswith(str(root_b.resolve())) or not path.is_file():
                return ("not found", 404)
            return flask.Response(
                path.read_text(encoding="utf-8"), mimetype="text/html"
            )

        bind_webmentions(app_a, handler_a, route="/webmentions")
        bind_webmentions(app_b, handler_b, route="/webmentions")

        run_ctx_a = _run_flask_app(app_a, host, port_a)
        run_ctx_b = _run_flask_app(app_b, host, port_b)

    else:
        pytest.importorskip("fastapi")
        from webmentions.server.adapters.fastapi import FastAPI, bind_webmentions
        from fastapi.responses import HTMLResponse

        app_a = FastAPI()
        app_b = FastAPI()

        @app_a.get("/health")
        def _health_a():
            return {"status": "ok"}

        @app_b.get("/health")
        def _health_b():
            return {"status": "ok"}

        @app_a.get("/content/{p:path}")
        def _content_a(p: str):
            path = (root_a / p).resolve()
            if not str(path).startswith(str(root_a.resolve())) or not path.is_file():
                return HTMLResponse("not found", status_code=404)
            return HTMLResponse(path.read_text(encoding="utf-8"))

        @app_b.get("/content/{p:path}")
        def _content_b(p: str):
            path = (root_b / p).resolve()
            if not str(path).startswith(str(root_b.resolve())) or not path.is_file():
                return HTMLResponse("not found", status_code=404)
            return HTMLResponse(path.read_text(encoding="utf-8"))

        bind_webmentions(app_a, handler_a, route="/webmentions")
        bind_webmentions(app_b, handler_b, route="/webmentions")

        run_ctx_a = _run_fastapi_app(app_a, host, port_a)
        run_ctx_b = _run_fastapi_app(app_b, host, port_b)

    with run_ctx_a, run_ctx_b:
        _wait_for(
            lambda: requests.get(f"{base_a}/health", timeout=2).status_code == 200
        )
        _wait_for(
            lambda: requests.get(f"{base_b}/health", timeout=0.5).status_code == 200
        )

        fs_a.start_watcher()
        fs_b.start_watcher()
        time.sleep(0.05)
        try:
            _assert_in_mentions(db_b, target_url=target_url, expected_sources=set())
            _assert_get_mentions(
                base_url=base_b, target_url=target_url, expected_sources=set()
            )

            # 1) Add mention
            _write(
                source_file,
                f"""<html><head>
<meta property=\"article:published_time\" content=\"2026-02-07T00:00:00+00:00\" />
<title>Source v1</title>
</head>
<body>
<h1>hello</h1>
<a href=\"{target_url}\">target</a>
</body></html>""",
            )

            _wait_for_received(
                received_b,
                source_url=source_url,
                target_url=target_url,
                direction=WebmentionDirection.IN,
                timeout=8,
            )
            _assert_in_mentions(
                db_b, target_url=target_url, expected_sources={source_url}
            )

            _wait_for(
                lambda: {
                    m.get("source")
                    for m in _get_webmentions_json(
                        base_url=base_b,
                        resource=target_url,
                        direction=WebmentionDirection.IN,
                    )
                }
                == {source_url}
            )
            mentions_json = _assert_get_mentions(
                base_url=base_b, target_url=target_url, expected_sources={source_url}
            )
            assert mentions_json and (mentions_json[0].get("title") == "Source v1")

            # 2) Update resource (should update stored fields)
            _write(
                source_file,
                f"""<html><head>
<meta property=\"og:title\" content=\"Source v2\" />
<meta property=\"article:published_time\" content=\"2026-02-07T00:00:00+00:00\" />
<title>Source v2</title>
</head>
<body>
<p>changed</p>
<a href=\"{target_url}\">target</a>
</body></html>""",
            )

            _wait_for_received(
                received_b,
                source_url=source_url,
                target_url=target_url,
                direction=WebmentionDirection.IN,
                timeout=8,
            )
            mentions = _assert_in_mentions(
                db_b, target_url=target_url, expected_sources={source_url}
            )
            assert mentions and (mentions[0].title == "Source v2")

            _wait_for(
                lambda: (
                    _assert_get_mentions(
                        base_url=base_b,
                        target_url=target_url,
                        expected_sources={source_url},
                    )[0].get("title")
                    == "Source v2"
                )
            )

            # 3) Remove mention from resource
            _write(
                source_file,
                "<html><head><title>Source v3</title></head><body>no links</body></html>",
            )
            _wait_for_received(
                received_b,
                source_url=source_url,
                target_url=target_url,
                direction=WebmentionDirection.IN,
                timeout=8,
            )
            _assert_in_mentions(db_b, target_url=target_url, expected_sources=set())
            _wait_for(
                lambda: (
                    _assert_get_mentions(
                        base_url=base_b, target_url=target_url, expected_sources=set()
                    )
                    == []
                )
            )

            # 4) Add mention again
            _write(
                source_file,
                f"""<html><head>
<meta property=\"article:published_time\" content=\"2026-02-07T00:00:00+00:00\" />
</head><body>again <a href=\"{target_url}\">t</a></body></html>""",
            )
            _wait_for_received(
                received_b,
                source_url=source_url,
                target_url=target_url,
                direction=WebmentionDirection.IN,
                timeout=8,
            )
            _assert_in_mentions(
                db_b, target_url=target_url, expected_sources={source_url}
            )

            _wait_for(
                lambda: {
                    m.get("source")
                    for m in _get_webmentions_json(
                        base_url=base_b,
                        resource=target_url,
                        direction=WebmentionDirection.IN,
                    )
                }
                == {source_url}
            )

            # 5) Remove the resource entirely
            _delete(source_file)
            _wait_for_received(
                received_b,
                source_url=source_url,
                target_url=target_url,
                direction=WebmentionDirection.IN,
                timeout=8,
            )
            _assert_in_mentions(db_b, target_url=target_url, expected_sources=set())
            _wait_for(
                lambda: (
                    _assert_get_mentions(
                        base_url=base_b, target_url=target_url, expected_sources=set()
                    )
                    == []
                )
            )
        finally:
            fs_a.stop_watcher()
            fs_b.stop_watcher()
