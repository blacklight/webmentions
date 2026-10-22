//! Framework-agnostic HTTP handling for the Webmention endpoints —
//! the Rust equivalent of the shared logic in
//! `webmentions.server.adapters.*`.
//!
//! [`dispatch`] maps `(method, path, query, headers, body)` to a plain
//! [`HttpResp`]. Any Rust web framework can adapt it; the `axum`
//! feature provides a ready-made adapter (see `server::axum`).

#[cfg(feature = "axum")]
pub mod axum;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::handlers::WebmentionsHandler;
use crate::model::WebmentionDirection;

/// A framework-agnostic HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResp {
    /// HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body bytes.
    pub body: Vec<u8>,
}

impl HttpResp {
    /// Build a JSON response.
    pub fn json(status: u16, body: &Value) -> Self {
        HttpResp {
            status,
            headers: [("Content-Type".to_string(), "application/json".to_string())]
                .into_iter()
                .collect(),
            body: serde_json::to_vec(body).unwrap_or_default(),
        }
    }
}

/// A framework-agnostic incoming request passed to [`dispatch`].
#[derive(Debug, Clone, Default)]
pub struct DispatchRequest {
    /// HTTP method (`GET`, `POST`, …).
    pub method: String,
    /// Request path.
    pub path: String,
    /// Query parameters (first value wins).
    pub query: HashMap<String, String>,
    /// Request headers (names lowercased).
    pub headers: HashMap<String, String>,
    /// Raw request body.
    pub body: Vec<u8>,
}

/// The `Link` header value advertising a Webmention endpoint —
/// mirrors `webmention_link_header_value`.
pub fn webmention_link_header_value(endpoint: &str) -> String {
    format!("<{endpoint}>; rel=\"webmention\"")
}

/// Append a `Link` header value, deduplicated — mirrors
/// `append_link_header`.
pub fn append_link_header(existing: Option<&str>, to_add: &str) -> Option<String> {
    match existing {
        None => Some(to_add.to_string()),
        Some(e) if e.contains(to_add) => Some(e.to_string()),
        Some(e) => Some(format!("{e}, {to_add}")),
    }
}

/// Compute the `Link` header for a `text/*` response — the shared
/// logic of the framework middlewares, which advertise every bound
/// Webmention endpoint on textual responses.
///
/// `content_type`/`existing` may be `None`.
pub fn injected_link_header(
    content_type: Option<&str>,
    existing: Option<&str>,
    endpoints: &HashSet<String>,
) -> Option<String> {
    let ct = content_type?;
    if !ct
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .starts_with("text/")
    {
        return existing.map(String::from);
    }
    let mut link = existing.map(String::from);
    for endpoint in endpoints {
        link = append_link_header(link.as_deref(), &webmention_link_header_value(endpoint));
    }
    link
}

/// Handle an incoming-Webmention POST — mirrors the Flask/FastAPI/
/// Tornado `webmention` route.
///
/// Parses `source`/`target` from the form-encoded body; on success
/// returns `{"status": "ok"}`; `WebmentionException`-class failures
/// return `400 {"error": …}`.
pub fn handle_webmention_post(handler: &Arc<WebmentionsHandler>, body: &[u8]) -> HttpResp {
    let form: HashMap<String, String> = url::form_urlencoded::parse(body).into_owned().collect();
    let source = form.get("source").cloned();
    let target = form.get("target").cloned();

    match handler.process_incoming_webmention(source.as_deref(), target.as_deref()) {
        Ok(_) => HttpResp::json(200, &json!({"status": "ok"})),
        Err(e) => HttpResp::json(400, &json!({"error": e.to_string()})),
    }
}

/// Handle the stored-Webmentions GET — mirrors the `retrieve_webmentions`
/// route.
///
/// Requires `resource` and `direction` query parameters; returns the
/// stored mentions serialized via `Webmention.to_dict`.
pub fn handle_retrieve_webmentions(
    handler: &Arc<WebmentionsHandler>,
    query: &HashMap<String, String>,
) -> HttpResp {
    let bad = |msg: &str| HttpResp::json(400, &json!({"error": msg}));
    let Some(resource) = query.get("resource") else {
        return bad("resource is required");
    };
    let Some(direction_raw) = query.get("direction") else {
        return bad("direction is required");
    };
    let Ok(direction) = WebmentionDirection::from_raw(direction_raw) else {
        return bad("invalid direction");
    };

    match handler.storage.retrieve_webmentions(resource, direction) {
        Ok(stored) => HttpResp::json(
            200,
            &Value::Array(stored.iter().map(|w| w.to_dict()).collect()),
        ),
        Err(e) => bad(&e.to_string()),
    }
}

/// Route `request` to the Webmention endpoint handlers — mirrors the
/// bound POST/GET routes of the Python adapters.
///
/// `route` is the bound path (default `"/webmentions"` in Python).
/// Non-matching requests get a 404.
pub fn dispatch(
    handler: &Arc<WebmentionsHandler>,
    request: &DispatchRequest,
    route: &str,
) -> HttpResp {
    if request.path != route {
        return HttpResp::json(404, &json!({"error": "not found"}));
    }
    match request.method.to_uppercase().as_str() {
        "POST" => handle_webmention_post(handler, &request.body),
        "GET" => handle_retrieve_webmentions(handler, &request.query),
        _ => HttpResp::json(405, &json!({"error": "method not allowed"})),
    }
}
