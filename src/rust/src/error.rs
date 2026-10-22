//! Error types for the webmentions crate — mirrors `webmentions._exceptions`.

/// Result type alias for webmentions operations.
pub type Result<T> = std::result::Result<T, WebmentionError>;

fn fmt_endpoint(endpoint: &Option<String>) -> String {
    endpoint.clone().unwrap_or_else(|| "None".to_string())
}

/// Base error type for Webmention operations.
///
/// Mirrors the Python exception hierarchy: the variants correspond to
/// `WebmentionException`, `WebmentionGone`, and the `ValueError` /
/// `requests.RequestException` exceptions raised across the package.
#[derive(Debug, thiserror::Error)]
pub enum WebmentionError {
    /// Generic Webmention error (mirrors `WebmentionException`).
    ///
    /// Displayed as ``Invalid Webmention from '{source}' to '{target}':
    /// {message}`` — the same format the Python exception renders.
    #[error("Invalid Webmention from '{source_url}' to '{target_url}': {message}", source_url = fmt_endpoint(source_url), target_url = fmt_endpoint(target_url))]
    Webmention {
        /// The source URL of the offending Webmention.
        source_url: Option<String>,
        /// The target URL of the offending Webmention.
        target_url: Option<String>,
        /// Description of the failure.
        message: String,
    },

    /// Raised when a Webmention no longer exists (mirrors
    /// `WebmentionGone`) — e.g. the source returns 404/410 or no longer
    /// links to the target.
    #[error("Invalid Webmention from '{source_url}' to '{target_url}': {message}", source_url = fmt_endpoint(source_url), target_url = fmt_endpoint(target_url))]
    Gone {
        /// The source URL of the offending Webmention.
        source_url: Option<String>,
        /// The target URL of the offending Webmention.
        target_url: Option<String>,
        /// Description of the failure.
        message: String,
    },

    /// Raised for invalid values (mirrors Python's `ValueError`).
    #[error("{0}")]
    InvalidValue(String),

    /// Errors without a dedicated Python exception counterpart
    /// (storage backends, internal failures).
    #[error("{0}")]
    General(String),

    /// Raised for HTTP transport failures (mirrors
    /// `requests.RequestException` / `requests.ConnectionError` —
    /// transient errors worth a retry).
    #[error("HTTP error: {0}")]
    Http(String),

    /// Wraps an I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Wraps a JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl WebmentionError {
    /// Construct a `Webmention` error — mirrors `WebmentionException`.
    pub fn webmention(
        source: impl Into<Option<String>>,
        target: impl Into<Option<String>>,
        message: impl Into<String>,
    ) -> Self {
        WebmentionError::Webmention {
            source_url: source.into(),
            target_url: target.into(),
            message: message.into(),
        }
    }

    /// Construct a `Gone` error — mirrors `WebmentionGone`. When
    /// `message` is `None`, the Python default
    /// `"Webmention no longer exists"` is used.
    pub fn gone(
        source: impl Into<Option<String>>,
        target: impl Into<Option<String>>,
        message: impl Into<Option<String>>,
    ) -> Self {
        WebmentionError::Gone {
            source_url: source.into(),
            target_url: target.into(),
            message: message
                .into()
                .unwrap_or_else(|| "Webmention no longer exists".to_string()),
        }
    }

    /// Whether this error is a "gone" rejection (mirrors
    /// `isinstance(e, WebmentionGone)`).
    pub fn is_gone(&self) -> bool {
        matches!(self, WebmentionError::Gone { .. })
    }

    /// The source URL attached to the error — mirrors
    /// `WebmentionException.source`.
    pub fn source(&self) -> Option<&str> {
        match self {
            WebmentionError::Webmention { source_url, .. }
            | WebmentionError::Gone { source_url, .. } => source_url.as_deref(),
            _ => None,
        }
    }

    /// The target URL attached to the error — mirrors
    /// `WebmentionException.target`.
    pub fn target(&self) -> Option<&str> {
        match self {
            WebmentionError::Webmention { target_url, .. }
            | WebmentionError::Gone { target_url, .. } => target_url.as_deref(),
            _ => None,
        }
    }
}

impl From<ureq::Error> for WebmentionError {
    fn from(e: ureq::Error) -> Self {
        WebmentionError::Http(e.to_string())
    }
}
