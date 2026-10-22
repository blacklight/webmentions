//! axum adapter — mirrors `webmentions.server.adapters.*`.
//!
//! [`bind_webmentions`] attaches the Webmention POST/GET routes and a
//! `Link`-header middleware to an existing `axum::Router`; the
//! middleware advertises every bound endpoint on `text/*` responses,
//! like the `after_request` hooks of the Flask/FastAPI/Tornado
//! adapters.
//!
//! [`webmentions_router`] returns a standalone router to merge or nest.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use axum::body::{to_bytes, Body};
use axum::extract::{Query, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::handlers::WebmentionsHandler;

use super::{handle_retrieve_webmentions, handle_webmention_post, injected_link_header, HttpResp};

/// Maximum accepted incoming-Webmention body (1 MiB).
const MAX_BODY: usize = 1024 * 1024;

/// Endpoints advertised via `Link` headers — shared between the route
/// registrations and the middleware.
pub type WebmentionEndpoints = Arc<RwLock<HashSet<String>>>;

/// Shared state for the axum adapter.
#[derive(Clone)]
struct WmState {
    handler: Arc<WebmentionsHandler>,
}

fn into_response(resp: HttpResp) -> Response {
    let mut builder = axum::http::Response::builder().status(resp.status);
    for (k, v) in &resp.headers {
        builder = builder.header(k, v);
    }
    builder
        .body(Body::from(resp.body))
        .unwrap_or_else(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn webmention_post(State(s): State<WmState>, req: Request) -> Response {
    let body = to_bytes(req.into_body(), MAX_BODY)
        .await
        .unwrap_or_default();
    into_response(handle_webmention_post(&s.handler, &body))
}

async fn webmention_get(
    State(s): State<WmState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    into_response(handle_retrieve_webmentions(&s.handler, &query))
}

/// The `Link`-header middleware — injects
/// `<endpoint>; rel="webmention"` on `text/*` responses, for every
/// endpoint in `endpoints`.
///
/// Apply it to your application router (e.g.
/// `app.layer(middleware::from_fn_with_state(endpoints, link_header_middleware))`)
/// to advertise the endpoint outside [`bind_webmentions`] routes.
pub async fn link_header_middleware(
    State(endpoints): State<WebmentionEndpoints>,
    req: Request,
    next: Next,
) -> Response {
    let mut resp = next.run(req).await;
    let content_type = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let existing = resp
        .headers()
        .get(axum::http::header::LINK)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let eps = endpoints.read().unwrap().clone();
    if let Some(link) = injected_link_header(content_type.as_deref(), existing.as_deref(), &eps) {
        if let Ok(v) = axum::http::HeaderValue::from_str(&link) {
            resp.headers_mut().insert(axum::http::header::LINK, v);
        }
    }
    resp
}

/// Bind Webmention routes to an axum `Router` — mirrors
/// `bind_webmentions` from the Python adapters.
///
/// Registers `POST {route}` (incoming Webmentions) and `GET {route}`
/// (stored-Webmentions lookup), records `route` in the shared
/// [`WebmentionEndpoints`] registry, and applies the `Link`-header
/// middleware to all routes already on `app` — matching the Python
/// `after_request` semantics.
///
/// Returns the updated router plus the endpoints registry, so the
/// caller can reuse it for the middleware on routes added later.
pub fn bind_webmentions(
    app: Router,
    handler: Arc<WebmentionsHandler>,
    route: &str,
) -> (Router, WebmentionEndpoints) {
    let endpoints: WebmentionEndpoints = Arc::new(RwLock::new(HashSet::new()));
    (
        bind_webmentions_with_endpoints(app, handler, route, endpoints.clone()),
        endpoints,
    )
}

/// Like [`bind_webmentions`] but reuses a shared endpoints registry —
/// call it once per Webmention route when multiple endpoints must be
/// advertised together.
pub fn bind_webmentions_with_endpoints(
    app: Router,
    handler: Arc<WebmentionsHandler>,
    route: &str,
    endpoints: WebmentionEndpoints,
) -> Router {
    endpoints.write().unwrap().insert(route.to_string());
    let state = WmState { handler };
    let wm = Router::new()
        .route(route, get(webmention_get).post(webmention_post))
        .with_state(state);
    // The layer applies to every route already on `app` — matching the
    // app-wide `after_request` semantics of the Python adapters.
    app.merge(wm).layer(middleware::from_fn_with_state(
        endpoints,
        link_header_middleware,
    ))
}

/// Build a standalone Webmentions `Router` — registers the POST/GET
/// routes on `route` and applies the `Link`-header middleware to them.
///
/// Returns the router and the shared endpoints registry.
pub fn webmentions_router(
    handler: Arc<WebmentionsHandler>,
    route: &str,
) -> (Router, WebmentionEndpoints) {
    bind_webmentions(Router::new(), handler, route)
}
