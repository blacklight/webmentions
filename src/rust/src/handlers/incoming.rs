//! Incoming Webmentions processor — mirrors
//! `webmentions.handlers._incoming`.

use std::sync::Arc;

use chrono::Utc;

use crate::error::{Result, WebmentionError};
use crate::model::{Webmention, WebmentionDirection, WebmentionStatus};
use crate::storage::WebmentionsStorage;

use super::common::{wrap_callback, MentionCallback};
use super::constants::{
    DEFAULT_HTTP_TIMEOUT, DEFAULT_MAX_SOURCE_RESPONSE_BYTES, DEFAULT_SSRF_PROTECTION,
    DEFAULT_USER_AGENT,
};
use super::fetch::Fetcher;
use super::parser::{ParserOptions, WebmentionsRequestParser};

/// Options for [`IncomingWebmentionsProcessor`] — mirrors the Python
/// kwargs, including their defaults.
pub struct IncomingOptions {
    /// The base URL of the server, used to validate target URLs.
    pub base_url: Option<String>,
    /// Additional base URLs for multi-domain setups.
    pub base_urls: Vec<String>,
    /// HTTP timeout for fetching source URLs, in seconds.
    pub http_timeout: f64,
    /// `User-Agent` for fetching source URLs.
    pub user_agent: String,
    /// Maximum response body size when parsing source pages.
    pub max_source_response_bytes: u64,
    /// Validate fetched URLs (and redirect hops) against
    /// private/loopback/reserved addresses.
    pub ssrf_protection: bool,
    /// Callback invoked when a Webmention is processed.
    pub on_mention_processed: Option<MentionCallback>,
    /// Callback invoked when a Webmention is deleted.
    pub on_mention_deleted: Option<MentionCallback>,
    /// The initial status of stored Webmentions.
    pub init_mention_status: WebmentionStatus,
    /// Injectable fetch backend — defaults to the guarded/plain
    /// fetchers selected by `ssrf_protection`.
    pub fetcher: Option<Fetcher>,
}

impl Default for IncomingOptions {
    fn default() -> Self {
        IncomingOptions {
            base_url: None,
            base_urls: Vec::new(),
            http_timeout: DEFAULT_HTTP_TIMEOUT,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            max_source_response_bytes: DEFAULT_MAX_SOURCE_RESPONSE_BYTES,
            ssrf_protection: DEFAULT_SSRF_PROTECTION,
            on_mention_processed: None,
            on_mention_deleted: None,
            init_mention_status: WebmentionStatus::Confirmed,
            fetcher: None,
        }
    }
}

/// Incoming Webmentions processor — mirrors
/// `IncomingWebmentionsProcessor`.
pub struct IncomingWebmentionsProcessor {
    /// The request parser used to fetch and parse source pages.
    pub parser: WebmentionsRequestParser,
    storage: Arc<dyn WebmentionsStorage>,
    on_mention_processed: Option<MentionCallback>,
    on_mention_deleted: Option<MentionCallback>,
    initial_mention_status: WebmentionStatus,
}

impl IncomingWebmentionsProcessor {
    /// Create the processor — mirrors
    /// `IncomingWebmentionsProcessor.__init__`.
    pub fn new(storage: Arc<dyn WebmentionsStorage>, options: IncomingOptions) -> Self {
        let parser = WebmentionsRequestParser::new(ParserOptions {
            base_url: options.base_url,
            base_urls: options.base_urls,
            http_timeout: options.http_timeout,
            user_agent: options.user_agent,
            max_source_response_bytes: options.max_source_response_bytes,
            ssrf_protection: options.ssrf_protection,
            fetcher: options.fetcher,
        });
        IncomingWebmentionsProcessor {
            parser,
            storage,
            on_mention_processed: wrap_callback(options.on_mention_processed),
            on_mention_deleted: wrap_callback(options.on_mention_deleted),
            initial_mention_status: options.init_mention_status,
        }
    }

    /// Process an incoming Webmention — mirrors
    /// `IncomingWebmentionsProcessor.process_incoming_webmention`.
    ///
    /// Returns `Ok(None)` when `source`/`target` are missing or when
    /// the source is gone (in which case any stored mention is deleted
    /// and `on_mention_deleted` fires), `Ok(Some(mention))` after a
    /// successful store.
    pub fn process_incoming_webmention(
        &self,
        source: Option<&str>,
        target: Option<&str>,
    ) -> Result<Option<Webmention>> {
        let (Some(source), Some(target)) = (source, target) else {
            return Ok(None);
        };

        log::info!("Received Webmention from '{source}' to '{target}'");

        let mut mention = match self.parser.parse(Some(source), Some(target)) {
            Ok(m) => m,
            Err(e) if e.is_gone() => {
                self.storage
                    .delete_webmention(source, target, WebmentionDirection::In)?;
                if let Some(cb) = &self.on_mention_deleted {
                    cb(&Webmention::new(source, target, WebmentionDirection::In));
                }
                log::info!("Deleted Webmention from '{source}' to '{target}'");
                return Ok(None);
            }
            Err(WebmentionError::InvalidValue(msg)) => {
                return Err(WebmentionError::webmention(
                    Some(source.to_string()),
                    Some(target.to_string()),
                    msg,
                ));
            }
            Err(e) => return Err(e),
        };

        let now = Utc::now();
        mention.created_at = Some(mention.created_at.or(mention.published).unwrap_or(now));
        mention.updated_at = Some(mention.updated_at.unwrap_or(now));
        mention.status = self.initial_mention_status;
        self.storage.store_webmention(&mention)?;
        if let Some(cb) = &self.on_mention_processed {
            cb(&mention);
        }
        log::info!("Processed Webmention from '{source}' to '{target}'");
        Ok(Some(mention))
    }
}
