import argparse


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run a local Webmentions example server"
    )
    parser.add_argument(
        "--backend",
        choices=("fastapi", "flask"),
        default="fastapi",
        help="Web framework backend to use",
    )
    parser.add_argument(
        "--address",
        default="127.0.0.1",
        help="Bind address",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=8000,
        help="Bind port",
    )
    parser.add_argument(
        "--engine",
        default="sqlite:////tmp/webmentions.db",
        help="SQLAlchemy engine string",
    )
    return parser


def main(argv: list[str] | None = None):
    args = _build_parser().parse_args(argv)

    if args.backend == "fastapi":
        try:
            from .fastapi_server import run_server
        except ImportError as e:  # pragma: no cover
            raise RuntimeError(
                "FastAPI example dependencies are missing. "
                "Install them with: pip install 'webmentions[fastapi]'"
            ) from e
    else:
        try:
            from .flask_server import run_server
        except ImportError as e:  # pragma: no cover
            raise RuntimeError(
                "Flask example dependencies are missing. "
                "Install them with: pip install 'webmentions[flask]'"
            ) from e

    run_server(engine=args.engine, address=args.address, port=args.port)


if __name__ == "__main__":
    main()
