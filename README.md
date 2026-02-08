![Webmentions](https://s3.fabiomanganiello.com/fabio/img/webmentions.png)

[![build](https://github.com/blacklight/webmentions/actions/workflows/build.yml/badge.svg)](https://github.com/blacklight/webmentions/actions/workflows/build.yml)
[![Coverage Badge](https://app.codacy.com/project/badge/Coverage/80a5b14c9beb4680a02477c7bd5a3df3)](https://app.codacy.com/gh/blacklight/webmentions/dashboard?utm_source=gh&utm_medium=referral&utm_content=&utm_campaign=Badge_coverage)
[![Codacy Badge](https://app.codacy.com/project/badge/Grade/80a5b14c9beb4680a02477c7bd5a3df3)](https://app.codacy.com/gh/blacklight/webmentions/dashboard?utm_source=gh&utm_medium=referral&utm_content=&utm_campaign=Badge_grade)

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

### Receiving Webmentions

If you use a framework with officially supported bindings (FastAPI or Flask)
then the `bind_webmentions` API is available to easily bind the Webmentions
handler to your app, which provides:

- A `POST /webmention` endpoint to receive Webmentions
- A `Link` header to advertise the Webmention endpoint on all the `text/*`
  responses

#### SQLAlchemy + FastAPI

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
    base_url="https://example.com",
)

# ...Initialize your Web app here...

# Bind Webmentions to your FastAPI app
bind_webmentions(app, handler)
```

#### SQLAlchemy + Flask

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
    base_url="https://example.com",
)

# ...Initialize your Web app here...

# Bind Webmentions to your Flask app
bind_webmentions(app, handler)
```

#### Generic setup

If you use neither FastAPI nor Flask, you can use Webmentions by modifying your
application as it follows:

- Create a `WebmentionsHandler` with an attached `WebmentionsStorage`

- Expose a `POST` endpoint for Webmentions that calls
  `WebmentionsHandler.process_incoming_webmentions(source, target)` when
  invoked, where `source` and `target` are the URLs of the source and the
  target of the Webmention, respectively, and they are passed as form
  parameters.

- Ensure that all the pages that support Webmentions advertise the endpoint
  either by adding a `Link` header or by adding a `rel="webmention"` link to
  the body.

Example with a FastAPI app:

```python
from fastapi import FastAPI, Form, HTTPException
from fastapi.templating import Jinja2Templates
from webmentions import WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage

app = FastAPI()
base_url = "https://example.com"
templates = Jinja2Templates(directory="/path/to/your/templates")
handler = WebmentionsHandler(
    storage=init_db_storage(engine="sqlite:////tmp/webmentions.db"),
    base_url=base_url,
)


# Route to receive Webmentions
@app.post("/webmention")
def webmention(
    source: str | None = Form(default=None),
    target: str | None = Form(default=None),
):
    try:
        # Forward the Webmention to the handler
        handler.process_incoming_webmention(source, target)
    except WebmentionException as e:
        raise HTTPException(status_code=400, detail=str(e)) from e

    return {"status": "ok"}


# Route that serves your static pages
@app.get("/article/<path:path>")
def serve_page(path: str):
    response = templates.TemplateResponse(path, {"request": request})
    # Add a Link header to advertise the Webmention endpoint
    response.headers["Link"] = f'<{base_url}/webmention>; rel="webmention"'
    return response
```

#### Generic storage

If you don't want to use `sqlalchemy` as a backend to store Webmentions, you
can extend `WebmentionsStorage` with your own implementation and bind it to
the `WebmentionsHandler` instance.

```python
from webmentions import (
    Webmention,
    WebmentionDirection,
    WebmentionsHandler,
    WebmentionsStorage,
)


class MyWebmentionsStorage(WebmentionsStorage):
    def store_webmention(self, mention: Webmention):
        ...

    def delete_webmention(
        self,
        source: str,
        target: str,
        direction: WebmentionDirection,
    ):
        ...

    def retrieve_webmentions(
        self, resource: str, direction: WebmentionDirection
    ) -> list[Webmention]:
        """
        Retrieve the stored Webmentions for a given URL.

        :param resource: The URL of the resource associated to the Webmentions
        :param direction: The direction of the Webmentions (inbound or outbound)
        :return: A list of Webmentions
        """

...

storage = MyWebmentionsStorage(...)
handler = WebmentionsHandler(
    storage=storage,
    base_url="https://example.com",
)
```

### Sending Webmentions

#### Filesystem monitor

If your pages are served as static files from a folder, you can use the
`FileSystemMonitor` to automatically send Webmentions when files are added or
modified.

First install `webmentions` with the `file` extra:

```bash
pip install "webmentions[file]"
```

Then add the following code to your app (FastAPI example):

```python
from webmentions import WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage
from webmentions.storage.adapters.file import FileSystemMonitor

app = FastAPI()
base_url = "https://example.com"
static_dir = "/srv/html/articles"
handler = WebmentionsHandler(
    storage=init_db_storage(engine="sqlite:////tmp/webmentions.db"),
    base_url=base_url,
)

# ...Initialize your Web app and bind the Webmentions handler...

# A function that takes a path to a created/modified/deleted text/* file
# and maps it to a URL on the Web server to be used as the Webmention source
def path_to_url(path: str) -> str:
    # Convert path (absolute) to a path relative to static_dir
    # and drop the extension.
    # For example, /srv/http/articles/2022/01/01/article.md
    # becomes /2022/01/01/article
    path = path[len(static_dir) + 1 :].rsplit(".", 1)[0].lstrip("/")
    # Convert the path to a URL on the Web server
    # For example, /2022/01/01/article
    # becomes https://example.com/articles/2022/01/01/article
    return f"{base_url.rstrip('/')}/articles/{path}"


# Create the filesystem monitor
monitor = FileSystemMonitor(
    # This should match the base path of your static files
    root_dir=static_dir,
    handler=handler,
    file_to_url_mapper=path_to_url,
)

# Start the monitor before running your app
monitor.start()
app.run(...)

# Stop the monitor when your app is stopped
monitor.stop()
```

Now every time a `text/*` file is created, modified or deleted (supported:
text, Markdown and HTML), or you run a `git pull` to update your static files,
Webmentions for updated resources will be automatically sent to any Web server
in the mentioned links that supports Webmentions.

#### Generic setup

If you don't store your articles or posts as static files (e.g. a database),
you can still send Webmentions by explicitly calling the
`WebmentionsHandler.process_outgoing_webmentions` whenever there are updates
to your content.

For example:

```python
from webmentions import ContentTextFormat, WebmentionsHandler

from myapp.storage import db

handler = WebmentionsHandler(...)

def save_post(post):
    db.save_post(post)

    # Note: This will process both added, updated or deleted mentions
    # in the provided post
    handler.process_outgoing_webmentions(
      source_url=post.url,
      # Optional. If not passed then the source content will be fetched
      # from the provided source_url. Otherwise, all links will be parsed
      # from the provided text. Plain text, Markdown or HTML are supported
      text=post.text,
      # Optional. If not passed then the source content type is inferred
      # either from the provided text or the source_url
      content_type=ContentTextFormat.MARKDOWN,
    )


def delete_post(post):
    db.delete_post(post)
    handler.process_outgoing_webmentions(
        source_url=post.url,
        # Pass an empty text to make sure that no links are parsed from cached
        # versions
        text="",
    )
```

Once `process_outgoing_webmentions` is hooked to your content modifications,
your Webmentions will be automatically sent whenever there are updates to your
pages.

## Add notifications to mentions

You may want to add your custom callbacks when a Webmention is sent or received -
for example to send notifications to your users when some of their content is
mentioned, or to keep track of the number of mentions sent by your pages, or to
perform any automated moderation or filtering when mentions are processed etc.

```python
from webmentions import Webmention, WebmentionDirection, WebmentionsHandler
from webmentions.storage.adapters.db import init_db_storage

base_url = "https://example.com"


def on_mention_processed(mention: Webmention):
    if mention.direction == WebmentionDirection.IN:
        print(
            f"Processed incoming Webmention from {mention.source} to {mention.target}"
        )
    else:
        print(
            f"Processed outgoing Webmention from {mention.source} to {mention.target}"
        )


def on_mention_deleted(mention: Webmention):
    if mention.direction == WebmentionDirection.IN:
        print(f"Deleted incoming Webmention from {mention.source} to {mention.target}")
    else:
        print(f"Deleted outgoing Webmention from {mention.source} to {mention.target}")


handler = WebmentionsHandler(
    storage=init_db_storage(engine="sqlite:////tmp/webmentions.db"),
    base_url=base_url,
    on_mention_processed=on_mention_processed,
    on_mention_deleted=on_mention_deleted,
)
```

## Filtering and moderation

By default any Webmention sent to your server will be processed if it matches a
target on your server.

You can change this behaviour by specifying `initial_mention_status` on your
Webmentions handler (supported: `WebmentionStatus.CONFIRMED`,
`WebmentionStatus.PENDING`, `WebmentionStatus.DELETED`).

Note that if you pass `PENDING` as an initial status, you will need to
explicitly approve the Webmentions to make them visible to your users.

You have two options:

- Manually set the `status` flag for the Webmention on your storage
- Pass an `on_mention_processed` callback to your Webmentions handler that will
  perform any moderation or filtering. For example:

```python
from webmentions import (
    Webmention,
    WebmentionDirection,
    WebmentionStatus,
    WebmentionsHandler,
)
from webmentions.storage.adapters.db import init_db_storage

base_url = "https://example.com"


def on_mention_processed(mention: Webmention):
    # Don't do anything for outgoing mentions
    if mention.direction == WebmentionDirection.OUT:
        return

    # Delete Webmentions coming from notorious spam domains or authors
    if mention.direction == WebmentionDirection.IN:
        if (
            mention.source in ["https://example.com/spam"] or
            mention.author_name in ["Spam Author"]
        ):
            mention.status = WebmentionStatus.DELETED
        # Otherwise, confirm the Webmention
        else:
            mention.status = WebmentionStatus.CONFIRMED

    # Save the modified mention
    handler.storage.store_webmention(mention)


handler = WebmentionsHandler(
    storage=init_db_storage(engine="sqlite:////tmp/webmentions.db"),
    base_url=base_url,
    on_mention_processed=on_mention_processed,
    initial_mention_status=WebmentionStatus.PENDING,
)
```

## Optimize your pages for Webmentions

The Webmentions recommendation is intentionally simple, and most of the
implementations only require a source and target URL to be sent to the
Webmentions endpoint.

It is then usually on the Web server that receives the notification to parse
any additional metadata from the source URL, such as the title of the article,
the author, the original publication date, etc.

You can add this information to your pages that support Webmentions by following
the [W3C recommendations for Semantic Elements](https://www.w3schools.com/html/html5_semantic_elements.asp)
and the [microformats2](https://microformats.org/wiki/microformats2) specification,
in particular the [`h-card`](https://microformats.org/wiki/h-card) and
[`h-entry`](https://microformats.org/wiki/h-entry) elements.

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />

    <!-- Advertise the Webmention endpoint -->
    <link rel="webmention" href="https://example.com/webmention" />

    <title>Example post</title>
  </head>

  <body>
    <header>
      <!-- Author/Publisher (microformats2 h-card) -->
      <div class="h-card">
        <a class="u-url p-name" href="https://example.com">Example Author</a>
        <img class="u-photo" alt="Example Author"
             src="https://example.com/avatar.jpg" />
      </div>
    </header>

    <main>
      <!-- Post (microformats2 h-entry) -->
      <article class="h-entry">
        <h1 class="p-name">Example post</h1>
        <a class="u-url u-uid" href="https://example.com/posts/example-post">
          Permalink
        </a>
        <time class="dt-published" datetime="2026-02-07T21:03:00+01:00">
          Feb 7, 2026
        </time>
        <div class="e-content">
          <p>
            This post mentions
            <a class="u-in-reply-to" href="https://target.example.org/post/123">a target URL</a>.
            <!--
              Other supported options besides u-in-reply-to include:

                - u-like-of (for likes)
                - u-repost-of (for reposts)
                - p-rsvp (for RSVPs, supports YES, MAYBE, NO and INTERESTED)
                - p-location (for location mentions, see https://microformats.org/wiki/h-geo)

              See https://microformats.org/wiki/h-entry#Properties for a full list,
              including proposed extensions.

              Both u-like-of and u-repost-of support an optional embedded h-cite
              for more granular details about the referenced content (e.g.
              author, title or published date)
            -->
          </p>
        </div>
      </article>
    </main>
  </body>
</html>
```

If you render your pages following these specifications then any mention on
target URLs that support Webmention will automatically include rich information
such as author details, media elements, likes/reposts, location, etc.

## Tests

```bash
pip install -e ".[tests]"
pytest tests
```
