//! Main Webmentions handler — mirrors `webmentions.handlers._handler`.
//!
//! Rust has no keyword arguments: construct the handler with
//! [`WebmentionsHandler::new`] and a [`HandlerOptions`] built via
//! struct-update syntax on `HandlerOptions::default()`.

use std::sync::Arc;

use crate::error::Result;
use crate::model::{ContentTextFormat, Webmention, WebmentionDirection, WebmentionStatus};
use crate::render::WebmentionsRenderer;
use crate::storage::WebmentionsStorage;

use super::common::MentionCallback;
use super::constants::{
    DEFAULT_EXCLUDE_LOCAL_TARGETS, DEFAULT_HTTP_TIMEOUT, DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES,
    DEFAULT_MAX_SOURCE_RESPONSE_BYTES, DEFAULT_SSRF_PROTECTION, DEFAULT_USER_AGENT,
};
use super::fetch::Fetcher;
use super::incoming::{IncomingOptions, IncomingWebmentionsProcessor};
use super::outgoing::{OutgoingOptions, OutgoingWebmentionsProcessor};

/// Options accepted by [`WebmentionsHandler::new`] — mirrors the
/// Python kwargs, including their defaults.
pub struct HandlerOptions {
    /// The base URL of the server, used to validate target URLs.
    pub base_url: Option<String>,
    /// A list of base URLs, used to validate target URLs when the
    /// server responds on multiple domains (e.g. split-domain setups).
    /// If both `base_url` and `base_urls` are provided, they are merged.
    pub base_urls: Vec<String>,
    /// The HTTP timeout for fetching source URLs, in seconds.
    pub http_timeout: f64,
    /// Maximum response body size to read when discovering outgoing
    /// Webmention endpoints from target HTML.
    pub max_discovery_response_bytes: u64,
    /// Maximum response body size to read when parsing incoming
    /// Webmention source pages.
    pub max_source_response_bytes: u64,
    /// Validate every fetched URL (including each redirect hop) against
    /// private/loopback/link-local/reserved addresses and cap response
    /// bodies. Keep enabled on public-facing deployments; disable only
    /// for private-network setups that legitimately mention internal
    /// hosts.
    pub ssrf_protection: bool,
    /// If `true`, outgoing targets whose host matches one of the
    /// configured `base_url`/`base_urls` are skipped instead of being
    /// notified. Enable it when the application already handles
    /// same-site mentions through its own pipeline; leave it off (the
    /// default) when cross-post mentions on the same domain should be
    /// delivered as regular Webmentions.
    pub exclude_local_targets: bool,
    /// The `User-Agent` header to use when fetching source URLs.
    pub user_agent: String,
    /// A callback to call when a Webmention is processed.
    pub on_mention_processed: Option<MentionCallback>,
    /// A callback to call when a Webmention is deleted.
    pub on_mention_deleted: Option<MentionCallback>,
    /// The initial status of Webmentions. Defaults to
    /// [`WebmentionStatus::Confirmed`]; set it to
    /// [`WebmentionStatus::Pending`] to require manual confirmation —
    /// e.g. through an `on_mention_processed` callback that performs
    /// custom filtering/moderation and then calls
    /// `handler.storage.store_webmention(webmention)` with the right
    /// status.
    pub initial_mention_status: WebmentionStatus,
    /// Injectable fetch backend shared by the parser and the outgoing
    /// processor — the seam used by tests to stub HTTP. Defaults to the
    /// guarded/plain fetchers selected by `ssrf_protection`.
    pub fetcher: Option<Fetcher>,
    /// Maximum parallel delivery workers for outgoing mentions.
    pub max_delivery_workers: usize,
}

impl Default for HandlerOptions {
    /// The same defaults as the Python kwargs
    /// (`http_timeout=10`, `ssrf_protection=True`, …).
    fn default() -> Self {
        HandlerOptions {
            base_url: None,
            base_urls: Vec::new(),
            http_timeout: DEFAULT_HTTP_TIMEOUT,
            max_discovery_response_bytes: DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES,
            max_source_response_bytes: DEFAULT_MAX_SOURCE_RESPONSE_BYTES,
            ssrf_protection: DEFAULT_SSRF_PROTECTION,
            exclude_local_targets: DEFAULT_EXCLUDE_LOCAL_TARGETS,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            on_mention_processed: None,
            on_mention_deleted: None,
            initial_mention_status: WebmentionStatus::Confirmed,
            fetcher: None,
            max_delivery_workers: 10,
        }
    }
}

/// Main Webmentions handler — façade tying together the incoming and
/// outgoing processors and the renderer.
///
/// Mirrors `WebmentionsHandler`.
pub struct WebmentionsHandler {
    /// The storage backend.
    pub storage: Arc<dyn WebmentionsStorage>,
    /// The incoming-mentions processor.
    pub incoming: IncomingWebmentionsProcessor,
    /// The outgoing-mentions processor.
    pub outgoing: OutgoingWebmentionsProcessor,
    renderer: WebmentionsRenderer,
}

impl WebmentionsHandler {
    /// Create a handler — mirrors `WebmentionsHandler.__init__`.
    pub fn new(storage: Arc<dyn WebmentionsStorage>, options: HandlerOptions) -> Self {
        let incoming = IncomingWebmentionsProcessor::new(
            storage.clone(),
            IncomingOptions {
                base_url: options.base_url.clone(),
                base_urls: options.base_urls.clone(),
                http_timeout: options.http_timeout,
                user_agent: options.user_agent.clone(),
                max_source_response_bytes: options.max_source_response_bytes,
                ssrf_protection: options.ssrf_protection,
                on_mention_processed: options.on_mention_processed.clone(),
                on_mention_deleted: options.on_mention_deleted.clone(),
                init_mention_status: options.initial_mention_status,
                fetcher: options.fetcher.clone(),
            },
        );
        let outgoing = OutgoingWebmentionsProcessor::new(
            storage.clone(),
            OutgoingOptions {
                base_url: options.base_url,
                base_urls: options.base_urls,
                exclude_local_targets: options.exclude_local_targets,
                user_agent: options.user_agent,
                http_timeout: options.http_timeout,
                max_discovery_response_bytes: options.max_discovery_response_bytes,
                ssrf_protection: options.ssrf_protection,
                on_mention_processed: options.on_mention_processed,
                on_mention_deleted: options.on_mention_deleted,
                fetcher: options.fetcher,
                max_workers: options.max_delivery_workers,
            },
        );
        WebmentionsHandler {
            storage,
            incoming,
            outgoing,
            renderer: WebmentionsRenderer::new(),
        }
    }

    /// Process an incoming Webmention — mirrors
    /// `process_incoming_webmention`.
    pub fn process_incoming_webmention(
        &self,
        source_url: Option<&str>,
        target_url: Option<&str>,
    ) -> Result<Option<Webmention>> {
        self.incoming
            .process_incoming_webmention(source_url, target_url)
    }

    /// Process outgoing Webmentions — mirrors
    /// `process_outgoing_webmentions`.
    ///
    /// - `source_url`: the source URL of the Webmention. Ignored when
    ///   `text` is provided.
    /// - `text`: the text of the Webmention. When `None`, `source_url`
    ///   is fetched.
    /// - `text_format`: inferred when `None`.
    pub fn process_outgoing_webmentions(
        &self,
        source_url: &str,
        text: Option<&str>,
        text_format: Option<ContentTextFormat>,
    ) -> Result<()> {
        self.outgoing
            .process_outgoing_webmentions(source_url, text, text_format)
    }

    /// Retrieve stored Webmentions for a given URL — mirrors
    /// `retrieve_stored_webmentions`.
    ///
    /// - `resource`: the resource URL
    /// - `direction`: the direction of the Webmentions
    pub fn retrieve_stored_webmentions(
        &self,
        resource: &str,
        direction: WebmentionDirection,
    ) -> Result<Vec<Webmention>> {
        self.storage.retrieve_webmentions(resource, direction)
    }

    /// Render a Webmention as an HTML snippet — mirrors
    /// `render_webmention`.
    ///
    /// `template` can be a template source string or a filesystem path;
    /// `None` uses the bundled `webmention.html` template.
    pub fn render_webmention(
        &self,
        webmention: &Webmention,
        template: Option<&str>,
    ) -> Result<String> {
        self.renderer.render_webmention(webmention, template)
    }

    /// Render a list of Webmentions as an HTML snippet — mirrors
    /// `render_webmentions`.
    pub fn render_webmentions(
        &self,
        webmentions: &[Webmention],
        template: Option<&str>,
    ) -> Result<String> {
        self.renderer.render_webmentions(webmentions, template)
    }
}
