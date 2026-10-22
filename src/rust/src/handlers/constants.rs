//! Default configuration values — mirrors
//! `webmentions.handlers._constants`.

/// Default `User-Agent` for fetches.
pub const DEFAULT_USER_AGENT: &str = "WebmentionSender";
/// Default HTTP timeout, in seconds.
pub const DEFAULT_HTTP_TIMEOUT: f64 = 10.0;
/// Default cap on endpoint-discovery response bodies.
pub const DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES: u64 = 1024 * 1024;
/// Default cap on incoming-source response bodies.
pub const DEFAULT_MAX_SOURCE_RESPONSE_BYTES: u64 = 1024 * 1024;
/// Whether SSRF protection is enabled by default.
pub const DEFAULT_SSRF_PROTECTION: bool = true;
/// Whether local targets are excluded by default.
pub const DEFAULT_EXCLUDE_LOCAL_TARGETS: bool = false;
