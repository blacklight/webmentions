//! Storage layer — mirrors `webmentions.storage`.
//!
//! [`WebmentionsStorage`] is the pluggable storage trait (the Rust
//! equivalent of the Python ABC). `Arc<dyn WebmentionsStorage>` is
//! used wherever the handler needs a shared, thread-safe backend.

#[cfg(feature = "db")]
pub mod db;

#[cfg(feature = "file")]
pub mod file;

use chrono::Utc;

use crate::error::Result;
use crate::model::{Webmention, WebmentionDirection};

/// Abstract storage backend for Webmentions — mirrors
/// `WebmentionsStorage`.
///
/// Implementations must be `Send + Sync` — the processors invoke the
/// backend from delivery threads.
pub trait WebmentionsStorage: Send + Sync {
    /// Store a Webmention.
    fn store_webmention(&self, mention: &Webmention) -> Result<()>;

    /// Mark a Webmention as deleted.
    ///
    /// - `source`: the source URL of the Webmention
    /// - `target`: the target URL of the Webmention
    /// - `direction`: inbound or outbound
    fn delete_webmention(
        &self,
        source: &str,
        target: &str,
        direction: WebmentionDirection,
    ) -> Result<()>;

    /// Retrieve the stored Webmentions for a given URL.
    ///
    /// - `resource`: the URL of the resource associated to the
    ///   Webmentions (the target for incoming mentions, the source for
    ///   outgoing ones)
    /// - `direction`: inbound or outbound
    fn retrieve_webmentions(
        &self,
        resource: &str,
        direction: WebmentionDirection,
    ) -> Result<Vec<Webmention>>;

    /// Mark a Webmention as sent — mirrors the default
    /// `WebmentionsStorage.mark_sent` implementation.
    fn mark_sent(&self, source: &str, target: &str) -> Result<()> {
        let now = Utc::now();
        let mut mention = Webmention::new(source, target, WebmentionDirection::Out);
        mention.published = Some(mention.published.unwrap_or(now));
        mention.created_at = Some(mention.created_at.unwrap_or(now));
        mention.updated_at = Some(now);
        self.store_webmention(&mention)
    }
}
