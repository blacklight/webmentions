![Webmentions](https://s3.fabiomanganiello.com/fabio/img/webmentions.png)

[![build](https://github.com/blacklight/webmentions/actions/workflows/build.yml/badge.svg)](https://github.com/blacklight/webmentions/actions/workflows/build.yml)

A general-purpose library to add Webmentions support to your website.

This library implements support for
[Webmentions](https://www.w3.org/TR/2016/WD-webmention-20160112/), a W3C
recommendation for federated mentions, comments and reactions on the Web.

The underlying mechanism of Webmentions is relatively simple:

- Alice publishes an article A1 on her blog A

- Bob reads A1 and mentions it in an article B1 on his blog B

- If both A and B support Webmentions, then:

  - A exposes an HTTP endpoint to receive mention notifications, and that
    endpoint is advertised on A1 as a `rel="webmention"` link in the page
    or a `Link` header in the HTTP response.

  - When Bob saves B1, its blog will scan for links in the page, discover
    which services expose a Webmention endpoint, and send a Webmention to
    A to notify them of the mention.

  - A will receive the notification and, if it accepts comments from B, the
    comment or the reaction will be rendered as a response on A1.

## Installation

For the base installation:

```bash
pip install webmentions
```

## Usage

This library provides the bindings to send and process Webmentions. It is
agnostic about how the Webmentions are stored and rendered, but it provides
a few helpers for common frameworks.

Some examples are provided under the
[examples](https://git.platypush.tech/blacklight/webmentions/src/branch/main/src/python/examples)
directory.

If you use a framework with officially supported bindings (FastAPI or Flask)
then the `bind_webmentions` API is available to easily bind the Webmentions
handler to your app, which provides:

- A `POST /webmention` endpoint to receive Webmentions
- A `Link` header to advertise the Webmention endpoint on all the `text/*`
  responses

### SQLAlchemy + FastAPI

```bash
pip install "webmentions[db,fastapi]"
```

```python
from fastapi import FastAPI
from webmentions import WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage
from webmentions.server.adapters.fastapi import bind_webmentions

app = FastAPI()

# Replace this with your own database URL, or an existing engine.
# Extra arguments passed to init_db_storage will be passed to create_engine
storage = init_db_storage(engine="sqlite:////tmp/webmentions.db")

# Your Webmentions handler
handler = WebmentionsHandler(
    storage=storage,
    # This should match the public base URL of your site
    base_url=f"https://example.com",
)

# ...Initialize your Web app here...

# Bind a POST /webmention to your FastAPI app
bind_webmentions(app, handler)
```

### SQLAlchemy + Flask

```bash
pip install "webmentions[db,flask]"
```

```python
from flask import Flask
from webmentions import WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage
from webmentions.server.adapters.flask import bind_webmentions

app = Flask(__name__)

# Replace this with your own database URL, or an existing engine.
# Extra arguments passed to init_db_storage will be passed to create_engine
storage = init_db_storage(engine="sqlite:////tmp/webmentions.db")

# Your Webmentions handler
handler = WebmentionsHandler(
    storage=storage,
    # This should match the public base URL of your site
    base_url=f"https://example.com",
)

# ...Initialize your Web app here...

# Bind a POST /webmention to your Flask app
bind_webmentions(app, handler)
```

## Tests

```bash
pip install -e ".[tests]"
pytest tests
```
