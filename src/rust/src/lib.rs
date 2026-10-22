//! Webmentions — a general-purpose library to add Webmentions support
//! to any Rust web application.
//!
//! This is the Rust port of the Python `webmentions` package. It
//! exposes the same API surface: a `Webmention` data model,
//! SSRF-guarded HTTP fetch, an mf2-powered request parser, incoming
//! and outgoing processors, pluggable storage (SQLite backend behind
//! the `db` feature, filesystem watcher/monitor behind the `file`
//! feature), a Jinja-compatible renderer, and an `axum` server
//! adapter (feature `axum`).

pub mod error;
pub mod handlers;
pub mod model;
pub mod render;
pub mod server;
pub mod storage;

pub use error::{Result, WebmentionError};
pub use handlers::{
    FetchRequest, FetchResponse, Fetcher, HandlerOptions, IncomingWebmentionsProcessor,
    MentionCallback, OutgoingWebmentionsProcessor, ParserOptions, WebmentionsHandler,
    WebmentionsRequestParser,
};
pub use model::{
    ContentTextFormat, Webmention, WebmentionDirection, WebmentionStatus, WebmentionType,
};
pub use render::WebmentionsRenderer;
pub use storage::WebmentionsStorage;

#[cfg(feature = "db")]
pub use storage::db::SqliteWebmentionsStorage;

#[cfg(feature = "file")]
pub use storage::file::{ContentChange, ContentChangeType, FileSystemMonitor, FileSystemWatcher};

/// The crate version — mirrors Python's `webmentions.__version__`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
