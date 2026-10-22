//! Incoming-Webmention request parser — mirrors
//! `webmentions.handlers._parser`.
//!
//! Fetches the source URL, verifies that it links back to the target,
//! and extracts an [`Webmention`] — Microformats2 properties first
//! (h-entry), then Open Graph / plain-HTML fallbacks.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{json, Map, Value};

use crate::error::{Result, WebmentionError};
use crate::model::{parse_dt_str, Webmention, WebmentionDirection, WebmentionType};

use super::constants::{
    DEFAULT_HTTP_TIMEOUT, DEFAULT_MAX_SOURCE_RESPONSE_BYTES, DEFAULT_SSRF_PROTECTION,
    DEFAULT_USER_AGENT,
};
use super::fetch::{default_fetcher, FetchRequest, Fetcher};
use super::mf2;

/// Pattern to strip URLs from titles — mirrors `_URL_PATTERN`.
static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*https?://\S+\s*$").unwrap());

static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// Maximum length of a derived excerpt, in characters — mirrors
/// `_EXCERPT_MAX_LENGTH`.
const EXCERPT_MAX_LENGTH: usize = 240;

/// Reduce an HTML fragment to whitespace-collapsed plain text —
/// mirrors `_html_to_text`.
fn html_to_text(html: &str) -> String {
    let text = {
        let fragment = Html::parse_fragment(html);
        fragment.root_element().text().collect::<Vec<_>>().join(" ")
    };
    WS_RE.replace_all(&text, " ").trim().to_string()
}

/// Truncate `text` at a word boundary within `limit` characters —
/// mirrors `_truncate_text`.
fn truncate_text(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let cut: String = text.chars().take(limit).collect();
    match cut.rfind(' ') {
        Some(idx) if !cut[..idx].trim_end().is_empty() => {
            format!("{}…", cut[..idx].trim_end())
        }
        _ => format!("{cut}…"),
    }
}

/// Derive a plain-text excerpt from (possibly HTML) content —
/// mirrors `_make_excerpt`.
fn make_excerpt(content: Option<&str>) -> Option<String> {
    let content = content?;
    if content.is_empty() {
        return None;
    }
    let excerpt = html_to_text(content);
    if excerpt.is_empty() {
        None
    } else {
        Some(truncate_text(&excerpt, EXCERPT_MAX_LENGTH))
    }
}

/// Remove trailing URLs from title strings — mirrors `_clean_title`.
fn clean_title(title: Option<String>) -> Option<String> {
    let title = title?;
    let cleaned = URL_PATTERN.replace(&title, "").trim().to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Options for [`WebmentionsRequestParser`] — mirrors the Python
/// kwargs, including their defaults.
pub struct ParserOptions {
    /// The base URL of the server, used to validate target URLs.
    pub base_url: Option<String>,
    /// Additional base URLs for multi-domain setups.
    pub base_urls: Vec<String>,
    /// HTTP timeout for fetching source URLs, in seconds.
    pub http_timeout: f64,
    /// `User-Agent` for fetching source URLs.
    pub user_agent: String,
    /// Maximum response body size when fetching source pages.
    pub max_source_response_bytes: u64,
    /// Validate fetched URLs (and redirect hops) against
    /// private/loopback/reserved addresses.
    pub ssrf_protection: bool,
    /// Injectable fetch backend — defaults to the guarded/plain
    /// fetchers selected by `ssrf_protection`.
    pub fetcher: Option<Fetcher>,
}

impl Default for ParserOptions {
    fn default() -> Self {
        ParserOptions {
            base_url: None,
            base_urls: Vec::new(),
            http_timeout: DEFAULT_HTTP_TIMEOUT,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            max_source_response_bytes: DEFAULT_MAX_SOURCE_RESPONSE_BYTES,
            ssrf_protection: DEFAULT_SSRF_PROTECTION,
            fetcher: None,
        }
    }
}

/// Parses a Webmention request — mirrors `WebmentionsRequestParser`.
pub struct WebmentionsRequestParser {
    base_urls: Vec<String>,
    http_timeout: Duration,
    user_agent: String,
    max_source_response_bytes: u64,
    fetcher: Fetcher,
}

impl WebmentionsRequestParser {
    /// Create a parser — mirrors `WebmentionsRequestParser.__init__`.
    ///
    /// `base_url` and `base_urls` are merged, like the Python
    /// constructor does.
    pub fn new(options: ParserOptions) -> Self {
        let mut base_urls = options.base_urls;
        if let Some(base) = options.base_url {
            if !base_urls.contains(&base) {
                base_urls.push(base);
            }
        }
        let fetcher = options
            .fetcher
            .unwrap_or_else(|| default_fetcher(options.ssrf_protection));
        WebmentionsRequestParser {
            base_urls,
            http_timeout: Duration::from_secs_f64(options.http_timeout),
            user_agent: options.user_agent,
            max_source_response_bytes: options.max_source_response_bytes,
            fetcher,
        }
    }

    /// Parse a Webmention — mirrors `WebmentionsRequestParser.parse`.
    ///
    /// `source`/`target` accept `None`/empty and fail with
    /// [`WebmentionError::InvalidValue`], mirroring the `ValueError`
    /// the Python method raises for missing URLs.
    pub fn parse(&self, source: Option<&str>, target: Option<&str>) -> Result<Webmention> {
        let (source, target) = match (source, target) {
            (Some(s), Some(t)) if !s.is_empty() && !t.is_empty() => (s, t),
            _ => {
                return Err(WebmentionError::InvalidValue(
                    "Missing source or target URL".to_string(),
                ))
            }
        };

        // Check that the target domain is one of this server's domains
        if !self.base_urls.is_empty() {
            let target_domain = url::Url::parse(target)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_lowercase()));
            let allowed: Vec<String> = self
                .base_urls
                .iter()
                .filter_map(|u| {
                    url::Url::parse(u)
                        .ok()
                        .and_then(|p| p.host_str().map(|h| h.to_lowercase()))
                })
                .collect();
            match target_domain {
                Some(d) if allowed.contains(&d) => {}
                _ => {
                    return Err(WebmentionError::InvalidValue(
                        "Target URL domain does not match server domain".to_string(),
                    ))
                }
            }
        }

        let resp = (self.fetcher)(&FetchRequest {
            url: source.to_string(),
            timeout: self.http_timeout,
            user_agent: self.user_agent.clone(),
            max_bytes: self.max_source_response_bytes,
            data: None,
        })?;

        if matches!(resp.status, 404 | 410) {
            return Err(WebmentionError::gone(
                Some(source.to_string()),
                Some(target.to_string()),
                Some("Source URL not found".to_string()),
            ));
        }

        if !resp.is_success() {
            return Err(WebmentionError::Http(format!(
                "source fetch failed with status {}",
                resp.status
            )));
        }

        let text = resp.text();

        // Check that the target URL is included in the source content.
        // The Webmention REC requires an exact match of the target URL.
        if !source_mentions_target(&text, target) {
            return Err(WebmentionError::gone(
                Some(source.to_string()),
                Some(target.to_string()),
                Some("Target URL not found in source content".to_string()),
            ));
        }

        let mut mention = Webmention::new(source, target, WebmentionDirection::In);
        self.parse_source_payload(&mut mention, &text, source, target);
        Ok(mention)
    }

    /// Fill `mention` from the fetched source HTML — mirrors
    /// `_parse_source_payload`.
    fn parse_source_payload(
        &self,
        mention: &mut Webmention,
        html: &str,
        source_url: &str,
        target_url: &str,
    ) {
        if let Some(entry) = extract_h_entry(html, source_url) {
            fill_from_h_entry(mention, &entry, target_url);
        }
        fill_from_html_fallbacks(mention, html);
        if mention.excerpt.is_none() {
            mention.excerpt = make_excerpt(mention.content.as_deref());
        }
    }
}

fn source_mentions_target(source_body: &str, target_url: &str) -> bool {
    static HREF_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("[href]").unwrap());
    static SRC_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("[src]").unwrap());

    let doc = Html::parse_document(source_body);
    if doc
        .select(&HREF_SEL)
        .any(|el| el.value().attr("href") == Some(target_url))
    {
        return true;
    }
    if doc
        .select(&SRC_SEL)
        .any(|el| el.value().attr("src") == Some(target_url))
    {
        return true;
    }
    source_body.contains(target_url)
}

/// Find the first `h-entry` in the mf2 parse — mirrors
/// `_extract_h_entry`.
fn extract_h_entry(html: &str, source_url: &str) -> Option<Value> {
    let parsed = mf2::parse(html, source_url)?;
    let items = parsed.get("items")?.as_array()?;

    let is_h_entry = |item: &Value| {
        item.get("type")
            .and_then(Value::as_array)
            .is_some_and(|t| t.iter().any(|v| v.as_str() == Some("h-entry")))
    };

    items
        .iter()
        .find(|item| is_h_entry(item))
        .or_else(|| {
            items.iter().find_map(|item| {
                item.get("children")
                    .and_then(Value::as_array)
                    .and_then(|children| children.iter().find(|c| is_h_entry(c)))
            })
        })
        .cloned()
}

/// Mirrors `_first_str`: extract a string from an mf2 property value.
fn first_str(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => {
            for key in ["value", "url"] {
                if let Some(v) = o.get(key).and_then(Value::as_str) {
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
            None
        }
        Value::Array(arr) => {
            let v0 = arr.first()?;
            match v0 {
                Value::String(s) => Some(s.clone()),
                Value::Object(o) => {
                    for key in ["value", "url"] {
                        if let Some(v) = o.get(key).and_then(Value::as_str) {
                            if !v.is_empty() {
                                return Some(v.to_string());
                            }
                        }
                    }
                    None
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn prop<'a>(props: &'a Value, key: &str) -> Option<&'a Value> {
    props.get(key).filter(|v| !v.is_null())
}

fn prop_list(props: &Value, key: &str) -> Vec<Value> {
    props
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Extract the author of a Webmention — mirrors `_extract_author`.
fn extract_author(properties: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let author = prop(properties, "author").cloned().unwrap_or(Value::Null);
    let author = match &author {
        Value::Array(arr) => arr.first().cloned().unwrap_or(Value::Null),
        v => v.clone(),
    };

    match &author {
        Value::String(s) => {
            if s.starts_with("http://") || s.starts_with("https://") {
                (None, Some(s.clone()), None)
            } else {
                (Some(s.clone()), None, None)
            }
        }
        Value::Object(o) => {
            let props = o.get("properties").cloned().unwrap_or(Value::Null);
            (
                first_str(props.get("name")),
                first_str(props.get("url")),
                first_str(props.get("photo")),
            )
        }
        _ => (None, None, None),
    }
}

/// Extract a normalized location — mirrors `_extract_location`.
fn extract_location(properties: &Value) -> Value {
    let location = match prop(properties, "location") {
        None => return Value::Null,
        Some(l) => l.clone(),
    };
    let location = match &location {
        Value::Array(arr) => arr.first().cloned().unwrap_or(Value::Null),
        v => v.clone(),
    };

    match &location {
        Value::String(s) => json!({"name": Value::Null, "url": s}),
        Value::Object(o) => {
            let props = o.get("properties").cloned().unwrap_or(Value::Null);
            json!({
                "type": o.get("type").cloned().unwrap_or(Value::Null),
                "name": first_str(props.get("name")),
                "url": first_str(props.get("url")),
                "latitude": first_str(props.get("latitude")),
                "longitude": first_str(props.get("longitude")),
            })
        }
        _ => Value::Null,
    }
}

fn meta_mut(mention: &mut Webmention) -> &mut Map<String, Value> {
    if !mention.metadata.is_object() {
        mention.metadata = Value::Object(Map::new());
    }
    mention.metadata.as_object_mut().unwrap()
}

fn mf2_mut(mention: &mut Webmention) -> &mut Map<String, Value> {
    let meta = meta_mut(mention);
    let entry = meta
        .entry("mf2".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().unwrap()
}

/// Mirrors `_fill_mf2_metadata`.
fn fill_mf2_metadata(mention: &mut Webmention, entry: &Value, props: &Value) {
    let mf2 = mf2_mut(mention);
    mf2.insert(
        "type".into(),
        entry.get("type").cloned().unwrap_or(Value::Null),
    );
    mf2.insert("url".into(), first_str(props.get("url")).into());
    mf2.insert("uid".into(), first_str(props.get("uid")).into());
    mf2.insert("summary".into(), first_str(props.get("summary")).into());
    mf2.insert(
        "category".into(),
        Value::Array(prop_list(props, "category")),
    );
    mf2.insert(
        "syndication".into(),
        Value::Array(prop_list(props, "syndication")),
    );
    mf2.insert("rsvp".into(), first_str(props.get("rsvp")).into());
    mf2.insert(
        "bookmark_of".into(),
        Value::Array(prop_list(props, "bookmark-of")),
    );
    mf2.insert("like_of".into(), Value::Array(prop_list(props, "like-of")));
    mf2.insert(
        "repost_of".into(),
        Value::Array(prop_list(props, "repost-of")),
    );
    mf2.insert(
        "in_reply_to".into(),
        Value::Array(prop_list(props, "in-reply-to")),
    );
    mf2.insert(
        "follow_of".into(),
        Value::Array(prop_list(props, "follow-of")),
    );
    mf2.insert(
        "quotation_of".into(),
        Value::Array(prop_list(props, "quotation-of")),
    );
    mf2.insert("photo".into(), Value::Array(prop_list(props, "photo")));
    mf2.insert(
        "featured".into(),
        Value::Array(prop_list(props, "featured")),
    );
    mf2.insert("video".into(), Value::Array(prop_list(props, "video")));
    mf2.insert("audio".into(), Value::Array(prop_list(props, "audio")));
    mf2.insert(
        "location".into(),
        Value::Array(prop_list(props, "location")),
    );
    mf2.insert("photo_url".into(), first_str(props.get("photo")).into());
    mf2.insert(
        "featured_url".into(),
        first_str(props.get("featured")).into(),
    );
    mf2.insert("video_url".into(), first_str(props.get("video")).into());
    mf2.insert("audio_url".into(), first_str(props.get("audio")).into());
    mf2.insert("location_normalized".into(), extract_location(props));
}

/// Mirrors `_fill_core_fields_from_entry`.
fn fill_core_fields_from_entry(mention: &mut Webmention, props: &Value) {
    if mention.title.is_none() {
        mention.title = clean_title(first_str(props.get("name")));
    }

    if mention.published.is_none() {
        if let Some(published) = first_str(props.get("published")) {
            mention.published = parse_dt_str(&published);
        }
    }

    if mention.excerpt.is_none() {
        if let Some(summary) = first_str(props.get("summary")) {
            mention.excerpt = Some(summary);
        }
    }

    if mention.content.is_none() {
        if let Some(Value::Array(content)) = prop(props, "content") {
            if let Some(c0) = content.first() {
                mention.content = match c0 {
                    Value::Object(o) => o
                        .get("html")
                        .and_then(Value::as_str)
                        .or_else(|| o.get("value").and_then(Value::as_str))
                        .map(String::from),
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                };
            }
        }
    }

    if mention.content.is_none() {
        mention.content = first_str(props.get("content"));
    }
}

/// Mirrors `_fill_author_from_entry`.
fn fill_author_from_entry(mention: &mut Webmention, props: &Value) {
    if mention.author_name.is_some()
        || mention.author_url.is_some()
        || mention.author_photo.is_some()
    {
        return;
    }
    let (name, url, photo) = extract_author(props);
    mention.author_name = name;
    mention.author_url = url;
    mention.author_photo = photo;
}

/// Mirrors `_infer_mention_type_from_entry`.
fn infer_mention_type_from_entry(mention: &mut Webmention, props: &Value, target_url: &str) {
    if mention.mention_type != WebmentionType::Unknown {
        return;
    }

    let contains = |key: &str| -> bool {
        prop_list(props, key)
            .iter()
            .any(|v| v.as_str() == Some(target_url))
    };

    let raw = if contains("like-of") {
        "like-of"
    } else if contains("repost-of") {
        "repost-of"
    } else if contains("bookmark-of") {
        "bookmark-of"
    } else if contains("in-reply-to") {
        "in-reply-to"
    } else if contains("follow-of") {
        "follow-of"
    } else if first_str(props.get("rsvp")).is_some() {
        "rsvp"
    } else {
        "mention"
    };

    mention.mention_type_raw = Some(raw.to_string());
    mention.mention_type = WebmentionType::from_raw(Some(raw));
}

/// Mirrors `_fill_comments_from_entry`.
fn fill_comments_from_entry(mention: &mut Webmention, props: &Value) {
    let comments = prop_list(props, "comment");
    if comments.is_empty() {
        return;
    }
    let has_comments = mention
        .metadata
        .as_object()
        .is_some_and(|m| m.contains_key("comments"));
    if !has_comments {
        meta_mut(mention).insert("comments".into(), extract_comments(&comments));
    }
}

/// Mirrors `_extract_comments`.
fn extract_comments(comments: &[Value]) -> Value {
    let mut extracted = Vec::new();
    for comment in comments {
        match comment {
            Value::String(s) => extracted.push(json!({"url": s})),
            Value::Object(o) => {
                let c_props = o.get("properties").cloned().unwrap_or(Value::Null);
                let (author_name, author_url, author_photo) = extract_author(&c_props);
                let published = first_str(c_props.get("published"));
                let mut content = None;
                if let Some(Value::Array(c_content)) = prop(&c_props, "content") {
                    if let Some(c0) = c_content.first() {
                        content = match c0 {
                            Value::Object(co) => co
                                .get("html")
                                .and_then(Value::as_str)
                                .or_else(|| co.get("value").and_then(Value::as_str))
                                .map(String::from),
                            Value::String(s) => Some(s.clone()),
                            _ => None,
                        };
                    }
                }
                if content.is_none() {
                    content = first_str(c_props.get("content"));
                }
                extracted.push(json!({
                    "type": o.get("type").cloned().unwrap_or(Value::Null),
                    "name": first_str(c_props.get("name")),
                    "url": first_str(c_props.get("url")),
                    "published": published,
                    "content": content,
                    "author": {
                        "name": author_name,
                        "url": author_url,
                        "photo": author_photo,
                    },
                }));
            }
            _ => {}
        }
    }
    Value::Array(extracted)
}

/// Mirrors `_fill_from_h_entry`.
fn fill_from_h_entry(mention: &mut Webmention, entry: &Value, target_url: &str) {
    let props = entry.get("properties").cloned().unwrap_or(Value::Null);
    fill_mf2_metadata(mention, entry, &props);
    fill_core_fields_from_entry(mention, &props);
    fill_author_from_entry(mention, &props);
    infer_mention_type_from_entry(mention, &props, target_url);
    fill_comments_from_entry(mention, &props);
}

fn meta_content(doc: &Html, selector: &Selector) -> Option<String> {
    doc.select(selector)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.to_string())
}

/// Fill a Webmention from plain-HTML fallbacks — mirrors
/// `_fill_from_html_fallbacks`.
fn fill_from_html_fallbacks(mention: &mut Webmention, html: &str) {
    static OG_TITLE: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse(r#"meta[property="og:title"]"#).unwrap());
    static TW_TITLE: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse(r#"meta[name="twitter:title"]"#).unwrap());
    static TITLE_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("title").unwrap());
    static META_AUTHOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse(r#"meta[name="author"]"#).unwrap());
    static META_PUB: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse(r#"meta[property="article:published_time"]"#).unwrap());
    static OG_DESC: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse(r#"meta[property="og:description"]"#).unwrap());

    let doc = Html::parse_document(html);

    if mention.title.is_none() {
        if let Some(t) = meta_content(&doc, &OG_TITLE) {
            mention.title = clean_title(Some(t));
        } else if let Some(t) = meta_content(&doc, &TW_TITLE) {
            mention.title = clean_title(Some(t));
        } else if let Some(title_el) = doc.select(&TITLE_SEL).next() {
            let s = title_el.text().collect::<String>().trim().to_string();
            if !s.is_empty() {
                mention.title = clean_title(Some(s));
            }
        }
    }

    if mention.author_name.is_none() {
        mention.author_name = meta_content(&doc, &META_AUTHOR);
    }

    if mention.published.is_none() {
        if let Some(pub_str) = meta_content(&doc, &META_PUB) {
            mention.published = parse_dt_str(&pub_str);
        }
    }

    if mention.content.is_none() {
        mention.content = meta_content(&doc, &OG_DESC);
    }

    if mention.excerpt.is_none() {
        mention.excerpt = make_excerpt(mention.content.as_deref());
    }
}
