//! Outgoing Webmentions processor — mirrors
//! `webmentions.handlers._outgoing`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{mpsc, Arc, LazyLock, Mutex};
use std::time::Duration;

use regex::Regex;
use scraper::{Html, Selector};

use crate::error::{Result, WebmentionError};
use crate::model::{ContentTextFormat, Webmention, WebmentionDirection};
use crate::storage::WebmentionsStorage;

use super::common::{wrap_callback, MentionCallback};
use super::constants::{
    DEFAULT_EXCLUDE_LOCAL_TARGETS, DEFAULT_HTTP_TIMEOUT, DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES,
    DEFAULT_SSRF_PROTECTION, DEFAULT_USER_AGENT,
};
use super::fetch::{FetchRequest, FetchResponse, Fetcher};

static LINK_IN_MD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)]\((https?://[^\s)]+)\)").unwrap());
static AUTO_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<(https?://[^>\s]+)>").unwrap());
static BARE_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(https?://[^\s)\]}>"']+)"#).unwrap());
static LINK_TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<([^>]+)>").unwrap());

/// Maximum parallel delivery workers — mirrors the Python
/// `ThreadPoolExecutor(max_workers=10)` pools.
const DEFAULT_MAX_WORKERS: usize = 10;

/// Options for [`OutgoingWebmentionsProcessor`] — mirrors the Python
/// kwargs, including their defaults.
pub struct OutgoingOptions {
    /// The base URL of the server, used to identify local targets when
    /// `exclude_local_targets` is enabled.
    pub base_url: Option<String>,
    /// Additional base URLs, merged with `base_url`.
    pub base_urls: Vec<String>,
    /// Skip extracted targets whose host matches one of the configured
    /// base URLs — the same notion of "local" the incoming processor
    /// uses to validate targets. Has no effect without configured
    /// `base_url`/`base_urls`.
    pub exclude_local_targets: bool,
    /// `User-Agent` for outgoing fetches.
    pub user_agent: String,
    /// HTTP timeout, in seconds.
    pub http_timeout: f64,
    /// Maximum response body size read when discovering Webmention
    /// endpoints from target HTML.
    pub max_discovery_response_bytes: u64,
    /// Validate fetched URLs (and redirect hops) against
    /// private/loopback/reserved addresses.
    pub ssrf_protection: bool,
    /// Callback invoked when a Webmention is processed.
    pub on_mention_processed: Option<MentionCallback>,
    /// Callback invoked when a Webmention is deleted.
    pub on_mention_deleted: Option<MentionCallback>,
    /// Injectable fetch backend — defaults to the guarded/plain
    /// fetchers selected by `ssrf_protection`.
    pub fetcher: Option<Fetcher>,
    /// Maximum parallel delivery workers.
    pub max_workers: usize,
}

impl Default for OutgoingOptions {
    fn default() -> Self {
        OutgoingOptions {
            base_url: None,
            base_urls: Vec::new(),
            exclude_local_targets: DEFAULT_EXCLUDE_LOCAL_TARGETS,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            http_timeout: DEFAULT_HTTP_TIMEOUT,
            max_discovery_response_bytes: DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES,
            ssrf_protection: DEFAULT_SSRF_PROTECTION,
            on_mention_processed: None,
            on_mention_deleted: None,
            fetcher: None,
            max_workers: DEFAULT_MAX_WORKERS,
        }
    }
}

/// Outgoing Webmentions processor — mirrors
/// `OutgoingWebmentionsProcessor`.
pub struct OutgoingWebmentionsProcessor {
    storage: Arc<dyn WebmentionsStorage>,
    fetcher: Fetcher,
    http_timeout: Duration,
    max_discovery_response_bytes: u64,
    user_agent: String,
    on_mention_processed: Option<MentionCallback>,
    on_mention_deleted: Option<MentionCallback>,
    local_netlocs: HashSet<String>,
    max_workers: usize,
}

impl OutgoingWebmentionsProcessor {
    /// Create the processor — mirrors
    /// `OutgoingWebmentionsProcessor.__init__`.
    pub fn new(storage: Arc<dyn WebmentionsStorage>, options: OutgoingOptions) -> Self {
        let mut local_netlocs = HashSet::new();
        if options.exclude_local_targets {
            let mut local_urls = options.base_urls.clone();
            if let Some(base) = &options.base_url {
                local_urls.push(base.clone());
            }
            for u in &local_urls {
                if let Ok(parsed) = url::Url::parse(u) {
                    if let Some(host) = parsed.host_str() {
                        let netloc = match parsed.port() {
                            Some(p) => format!("{}:{p}", host.to_lowercase()),
                            None => host.to_lowercase(),
                        };
                        local_netlocs.insert(netloc);
                    }
                }
            }
            if local_netlocs.is_empty() {
                log::warn!(
                    "exclude_local_targets is enabled but no base_url/base_urls \
                     are configured - no targets will be treated as local"
                );
            }
        }

        OutgoingWebmentionsProcessor {
            storage,
            fetcher: options
                .fetcher
                .unwrap_or_else(|| super::fetch::default_fetcher(options.ssrf_protection)),
            http_timeout: Duration::from_secs_f64(options.http_timeout),
            max_discovery_response_bytes: options.max_discovery_response_bytes,
            user_agent: options.user_agent,
            on_mention_processed: wrap_callback(options.on_mention_processed),
            on_mention_deleted: wrap_callback(options.on_mention_deleted),
            local_netlocs,
            max_workers: options.max_workers,
        }
    }

    fn fetch(&self, url: &str, data: Option<HashMap<String, String>>) -> Result<FetchResponse> {
        (self.fetcher)(&FetchRequest {
            url: url.to_string(),
            timeout: self.http_timeout,
            user_agent: self.user_agent.clone(),
            max_bytes: self.max_discovery_response_bytes,
            data,
        })
    }

    /// Process outgoing Webmentions — mirrors
    /// `OutgoingWebmentionsProcessor.process_outgoing_webmentions`.
    ///
    /// - `source_url`: the source URL of the Webmention. Ignored when
    ///   `text` is provided.
    /// - `text`: the text of the Webmention; when `None`, `source_url`
    ///   is fetched.
    /// - `text_format`: inferred when `None`.
    pub fn process_outgoing_webmentions(
        &self,
        source_url: &str,
        text: Option<&str>,
        text_format: Option<ContentTextFormat>,
    ) -> Result<()> {
        let (text, text_format) = match text {
            Some(t) => (
                t.to_string(),
                text_format.unwrap_or(ContentTextFormat::Text),
            ),
            None => {
                let resp = self.fetch(source_url, None)?;
                if !resp.is_success() {
                    return Err(WebmentionError::Http(format!(
                        "source fetch failed with status {}",
                        resp.status
                    )));
                }
                (resp.text(), ContentTextFormat::Html)
            }
        };

        let new_targets = self.extract_targets(&text, text_format);
        let old_targets: HashSet<String> = self
            .storage
            .retrieve_webmentions(source_url, WebmentionDirection::Out)
            .map(|mentions| mentions.into_iter().map(|m| m.target).collect())
            .unwrap_or_default();

        let removed: Vec<String> = old_targets
            .difference(&new_targets)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let added: Vec<String> = new_targets
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|t| t != source_url) // Ignore self-mentions
            .collect();

        self.run_pool(added, |target| self.notify_added(source_url, &target));
        self.run_pool(removed, |target| self.notify_removed(source_url, &target));
        Ok(())
    }

    /// Fan `items` out to up to `max_workers` scoped threads — mirrors
    /// the `ThreadPoolExecutor` fan-out.
    fn run_pool(&self, items: Vec<String>, f: impl Fn(String) + Sync) {
        if items.is_empty() {
            return;
        }
        let workers = self.max_workers.max(1).min(items.len());
        let (tx, rx) = mpsc::channel::<String>();
        let rx = Mutex::new(rx);
        std::thread::scope(|s| {
            for _ in 0..workers {
                let rx = &rx;
                let f = &f;
                s.spawn(move || {
                    while let Ok(item) = rx.lock().unwrap().recv() {
                        f(item);
                    }
                });
            }
            for item in items {
                let _ = tx.send(item);
            }
            drop(tx);
        });
    }

    /// Notify a target of an outgoing Webmention — mirrors
    /// `_notify_added`.
    fn notify_added(&self, source_url: &str, target_url: &str) {
        let result = (|| -> Result<()> {
            self.notify_target(source_url, target_url)?;
            self.storage.mark_sent(source_url, target_url)?;
            if let Some(cb) = &self.on_mention_processed {
                cb(&Webmention::new(
                    source_url,
                    target_url,
                    WebmentionDirection::Out,
                ));
            }
            Ok(())
        })();
        if let Err(e) = result {
            log::info!("Outgoing Webmention failed (source={source_url} target={target_url}): {e}");
        }
    }

    /// Notify a target of a removed outgoing Webmention — mirrors
    /// `_notify_removed`.
    fn notify_removed(&self, source_url: &str, target_url: &str) {
        let result = (|| -> Result<()> {
            self.notify_target(source_url, target_url)?;
            self.storage
                .delete_webmention(source_url, target_url, WebmentionDirection::Out)?;
            if let Some(cb) = &self.on_mention_deleted {
                cb(&Webmention::new(
                    source_url,
                    target_url,
                    WebmentionDirection::Out,
                ));
            }
            Ok(())
        })();
        if let Err(e) = result {
            log::info!(
                "Outgoing Webmention deletion failed (source={source_url} target={target_url}): {e}"
            );
        }
    }

    /// Given a text and a text format, extract the target URLs —
    /// mirrors `_extract_targets`.
    fn extract_targets(&self, text: &str, text_format: ContentTextFormat) -> HashSet<String> {
        match text_format {
            ContentTextFormat::Html => self.extract_urls_from_html(text),
            _ => self.extract_urls_from_markdown_or_text(text),
        }
    }

    /// Extract URLs from HTML — mirrors `_extract_urls_from_html`.
    fn extract_urls_from_html(&self, html: &str) -> HashSet<String> {
        static A_LINK_SEL: LazyLock<Selector> =
            LazyLock::new(|| Selector::parse("a[href], link[href]").unwrap());
        static SRC_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("[src]").unwrap());

        let mut urls = HashSet::new();
        let doc = Html::parse_document(html);
        for el in doc.select(&A_LINK_SEL) {
            if let Some(href) = el.value().attr("href") {
                let href = href.trim();
                if href.to_lowercase().starts_with("http://")
                    || href.to_lowercase().starts_with("https://")
                {
                    urls.insert(href.to_string());
                }
            }
        }

        // Media embeds (<audio>, <video>, <img>, <source>, …) also
        // reference targets - the receiver-side verification accepts
        // any ``src`` match, so outgoing extraction should too.
        for el in doc.select(&SRC_SEL) {
            if let Some(src) = el.value().attr("src") {
                let src = src.trim();
                if src.to_lowercase().starts_with("http://")
                    || src.to_lowercase().starts_with("https://")
                {
                    urls.insert(src.to_string());
                }
            }
        }

        self.clean_and_filter_targets(urls)
    }

    /// Extract URLs from Markdown or text — mirrors
    /// `_extract_urls_from_markdown_or_text`.
    fn extract_urls_from_markdown_or_text(&self, md: &str) -> HashSet<String> {
        let mut urls = HashSet::new();
        for caps in LINK_IN_MD_RE.captures_iter(md) {
            urls.insert(caps[1].to_string());
        }
        for caps in AUTO_LINK_RE.captures_iter(md) {
            urls.insert(caps[1].to_string());
        }
        for caps in BARE_URL_RE.captures_iter(md) {
            urls.insert(caps[1].to_string());
        }
        self.clean_and_filter_targets(urls)
    }

    /// Clean and filter URLs — mirrors `_clean_and_filter_targets`.
    fn clean_and_filter_targets(&self, urls: HashSet<String>) -> HashSet<String> {
        urls.into_iter()
            .filter_map(|u| {
                let cleaned = u
                    .trim()
                    .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', ']', '\''])
                    .to_string();
                if cleaned.is_empty() {
                    return None;
                }
                let parsed = url::Url::parse(&cleaned).ok()?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    return None;
                }
                let host = parsed.host_str()?;
                if host.is_empty() {
                    return None;
                }
                let netloc = match parsed.port() {
                    Some(p) => format!("{}:{p}", host.to_lowercase()),
                    None => host.to_lowercase(),
                };
                if self.local_netlocs.contains(&netloc) {
                    return None;
                }
                Some(cleaned)
            })
            .collect()
    }

    /// Notify a target of an outgoing Webmention — mirrors
    /// `_notify_target`.
    fn notify_target(&self, source_url: &str, target_url: &str) -> Result<()> {
        let Some(endpoint) = self.discover_webmention_endpoint(target_url)? else {
            return Ok(());
        };
        let mut form = HashMap::new();
        form.insert("source".to_string(), source_url.to_string());
        form.insert("target".to_string(), target_url.to_string());
        let resp = self.fetch(&endpoint, Some(form))?;
        if resp.status >= 400 {
            return Err(WebmentionError::Http(format!(
                "webmention endpoint returned status {}",
                resp.status
            )));
        }
        Ok(())
    }

    fn is_html_response(resp: &FetchResponse) -> bool {
        let content_type = resp
            .header("content-type")
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        matches!(
            content_type.as_str(),
            "" | "text/html" | "application/xhtml+xml"
        )
    }

    /// Mirrors `_read_discovery_html` — returns the decoded body when
    /// the response is HTML and within the discovery byte cap.
    fn read_discovery_html(&self, resp: &FetchResponse, target_url: &str) -> Option<String> {
        if !Self::is_html_response(resp) {
            log::info!(
                "Skipping Webmention endpoint discovery for non-HTML target \
                 {target_url} (Content-Type: {})",
                resp.header("content-type").unwrap_or_default()
            );
            return None;
        }

        let max_bytes = self.max_discovery_response_bytes;
        if let Some(content_length) = resp.header("content-length") {
            if let Ok(n) = content_length.trim().parse::<u64>() {
                if n > max_bytes {
                    log::info!(
                        "Skipping Webmention endpoint discovery for oversized target \
                         {target_url} (Content-Length: {content_length})"
                    );
                    return None;
                }
            } else {
                log::debug!("Ignoring invalid Content-Length for {target_url}");
            }
        }

        if resp.body.len() as u64 > max_bytes {
            log::info!(
                "Skipping Webmention endpoint discovery for oversized target \
                 {target_url} (read more than {max_bytes} bytes)"
            );
            return None;
        }
        Some(resp.text())
    }

    /// Discover a Webmention endpoint for a target URL — mirrors
    /// `_discover_webmention_endpoint`.
    fn discover_webmention_endpoint(&self, target_url: &str) -> Result<Option<String>> {
        let resp = match self.fetch(target_url, None) {
            Ok(r) => r,
            // Policy rejections (SSRF guard) skip discovery entirely,
            // like the Python `except ValueError: return None`.
            Err(e @ WebmentionError::InvalidValue(_)) => {
                log::info!("Skipping Webmention endpoint discovery for {target_url}: {e}");
                return Ok(None);
            }
            Err(e) => return Err(e),
        };

        if !resp.is_success() {
            return Err(WebmentionError::Http(format!(
                "target fetch failed with status {}",
                resp.status
            )));
        }

        // Check if there is a Link header. This applies to any content
        // type: for non-HTML resources (audio, images, PDFs) the Link
        // header is the only way to advertise a Webmention endpoint.
        if let Some(link_header) = resp.header("link") {
            for part in link_header.split(',') {
                let lower = part.to_lowercase();
                if !lower.contains("rel=") || !lower.contains("webmention") {
                    continue;
                }
                if let Some(m) = LINK_TAG_RE.captures(part) {
                    return Ok(Some(url_join(&resp.url, &m[1])));
                }
            }
        }

        let Some(html) = self.read_discovery_html(&resp, target_url) else {
            return Ok(None);
        };

        // Check if there is a <link> or <a> tag
        static WEBMENTION_SEL: LazyLock<Selector> =
            LazyLock::new(|| Selector::parse("link[href], a[href]").unwrap());
        let doc = Html::parse_document(&html);
        for el in doc.select(&WEBMENTION_SEL) {
            let Some(href) = el.value().attr("href") else {
                continue;
            };
            let rel = el.value().attr("rel").unwrap_or_default();
            if rel.to_lowercase().contains("webmention") {
                return Ok(Some(url_join(&resp.url, href)));
            }
        }
        Ok(None)
    }
}

/// Resolve `href` against `base` — mirrors `urljoin`.
fn url_join(base: &str, href: &str) -> String {
    url::Url::parse(base)
        .ok()
        .and_then(|b| b.join(href).ok())
        .map(|u| u.to_string())
        .unwrap_or_else(|| href.to_string())
}
