# webmentions-sdk

Rust implementation of
[webmentions](https://github.com/blacklight/webmentions) — a
general-purpose library to add Webmentions support to your website.
Feature-parity with the Python API, with idiomatic Rust types
(`serde_json::Value` metadata, `Result` errors, options structs).

```toml
[dependencies]
webmentions-sdk = "0.1"
```

## Features

| Feature | Enables |
|---------|---------|
| `db` | `SqliteWebmentionsStorage` via `rusqlite` |
| `file` | `FileSystemWatcher`/`FileSystemMonitor` via `notify` |
| `axum` | `server::axum::{bind_webmentions, webmentions_router}` |

## Quick start

```rust
use std::sync::Arc;
use webmentions_sdk::{
    HandlerOptions, SqliteWebmentionsStorage, WebmentionsHandler,
};

let storage = Arc::new(
    SqliteWebmentionsStorage::new("/var/lib/myapp/webmentions.sqlite")?,
);
let handler = Arc::new(WebmentionsHandler::new(
    storage,
    HandlerOptions {
        base_url: Some("https://blog.example.com".into()),
        ..Default::default()
    },
));

// Receive mentions — usually called from your web framework's route:
handler.process_incoming_webmention(
    Some("https://remote.example/post"),
    Some("https://blog.example.com/article"),
)?;

// Send mentions to all links in a source:
handler.process_outgoing_webmentions(
    "https://blog.example.com/article", None, None,
)?;
```

With axum (feature `axum`):

```rust
let (app, _endpoints) = webmentions_sdk::server::axum::bind_webmentions(
    app, handler, "/webmentions",
);
```

This registers `POST /webmentions` (incoming mentions) and
`GET /webmentions?resource&direction` (stored-mentions lookup), and
advertises `<endpoint>; rel="webmention"` via `Link` headers on
`text/*` responses — mirroring the Flask/FastAPI/Tornado adapters.

For other frameworks, call `webmentions_sdk::server::dispatch(handler,
request, "/webmentions")` from your route handlers — it takes a plain
`DispatchRequest` and returns `HttpResp`.

## Render stored mentions

```rust
let mentions = handler.retrieve_stored_webmentions(
    "https://blog.example.com/article",
    webmentions_sdk::WebmentionDirection::In,
)?;
let html = handler.render_webmentions(&mentions, None)?;
```

## Watch a content directory (feature `file`)

```rust
use webmentions_sdk::{FileSystemMonitor, FileSystemWatcher};

let monitor = FileSystemMonitor::new(
    handler.clone(),
    "/srv/content",
    Arc::new(|path| format!("https://blog.example.com/{}", slug(path))),
    None,  // default extensions (.md/.markdown/.txt/.html/.htm)
    Some(10.0),  // throttle_seconds
);
monitor.start();
```

## Testing hooks

The HTTP layer is injectable through the `Fetcher` type on
`HandlerOptions`/`ParserOptions`/`OutgoingOptions` — the seam the
Python test suite reaches through `requests`/`fetch_guarded`
monkeypatching. Tests in `tests/rust` show how to script canned
responses.

See the crate docs and `docs/` in the repository for the
module-by-module mapping to the Python API and the semantic
relaxations.

## License

AGPL-3.0-or-later
