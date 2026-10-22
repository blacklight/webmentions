//! Data model — mirrors `webmentions._model`.
//!
//! Python `dict` ↔ `serde_json::Value`; `datetime` ↔
//! `chrono::DateTime<Utc>`; the str-enum members are serialized to their
//! Python ``.value`` strings so `to_dict` output is wire-compatible.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::error::{Result, WebmentionError};

/// Direction of a Webmention — mirrors `WebmentionDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebmentionDirection {
    /// An incoming mention (`"incoming"`).
    In,
    /// An outgoing mention (`"outgoing"`).
    Out,
}

impl WebmentionDirection {
    /// The Python ``.value`` string.
    pub fn as_str(&self) -> &'static str {
        match self {
            WebmentionDirection::In => "incoming",
            WebmentionDirection::Out => "outgoing",
        }
    }

    /// Parse a raw direction — mirrors `WebmentionDirection.from_raw`.
    ///
    /// Accepts both the enum value (`"incoming"`, `"outgoing"`) and the
    /// member name in any case (`"IN"`, `"out"`, …).
    pub fn from_raw(raw: &str) -> Result<Self> {
        match raw.trim().to_lowercase().as_str() {
            "incoming" | "in" => Ok(WebmentionDirection::In),
            "outgoing" | "out" => Ok(WebmentionDirection::Out),
            other => Err(WebmentionError::InvalidValue(format!(
                "Unknown direction: {other}"
            ))),
        }
    }
}

impl Serialize for WebmentionDirection {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WebmentionDirection {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::from_raw(&s).map_err(serde::de::Error::custom)
    }
}

/// Status of a Webmention — mirrors `WebmentionStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebmentionStatus {
    /// Awaiting moderation (`"pending"`).
    Pending,
    /// Verified and stored (`"confirmed"`).
    Confirmed,
    /// Soft-deleted (`"deleted"`).
    Deleted,
}

impl WebmentionStatus {
    /// The Python ``.value`` string.
    pub fn as_str(&self) -> &'static str {
        match self {
            WebmentionStatus::Pending => "pending",
            WebmentionStatus::Confirmed => "confirmed",
            WebmentionStatus::Deleted => "deleted",
        }
    }

    /// Parse a raw status (enum value or member name, any case).
    pub fn from_raw(raw: &str) -> Result<Self> {
        match raw.trim().to_lowercase().as_str() {
            "pending" => Ok(WebmentionStatus::Pending),
            "confirmed" => Ok(WebmentionStatus::Confirmed),
            "deleted" => Ok(WebmentionStatus::Deleted),
            other => Err(WebmentionError::InvalidValue(format!(
                "Unknown status: {other}"
            ))),
        }
    }
}

impl Serialize for WebmentionStatus {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WebmentionStatus {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::from_raw(&s).map_err(serde::de::Error::custom)
    }
}

/// Supported content text formats — mirrors `ContentTextFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentTextFormat {
    /// HTML content (`"html"`).
    Html,
    /// Markdown content (`"markdown"`).
    Markdown,
    /// Plain text content (`"text"`).
    Text,
}

impl ContentTextFormat {
    /// The Python ``.value`` string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentTextFormat::Html => "html",
            ContentTextFormat::Markdown => "markdown",
            ContentTextFormat::Text => "text",
        }
    }
}

/// Type of a Webmention — mirrors `WebmentionType`.
///
/// The list is not exhaustive (the Webmention recommendation provides
/// no static list); these are the commonly supported Microformats
/// types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebmentionType {
    /// Unclassified (`"unknown"`).
    Unknown,
    /// Plain mention (`"mention"`).
    Mention,
    /// Reply (`"reply"`).
    Reply,
    /// Like (`"like"`).
    Like,
    /// Repost (`"repost"`).
    Repost,
    /// Bookmark (`"bookmark"`).
    Bookmark,
    /// RSVP (`"rsvp"`).
    Rsvp,
    /// Follow (`"follow"`).
    Follow,
}

impl WebmentionType {
    /// The Python ``.value`` string.
    pub fn as_str(&self) -> &'static str {
        match self {
            WebmentionType::Unknown => "unknown",
            WebmentionType::Mention => "mention",
            WebmentionType::Reply => "reply",
            WebmentionType::Like => "like",
            WebmentionType::Repost => "repost",
            WebmentionType::Bookmark => "bookmark",
            WebmentionType::Rsvp => "rsvp",
            WebmentionType::Follow => "follow",
        }
    }

    /// Parse a raw mention type — mirrors `WebmentionType.from_raw`.
    ///
    /// `None`/empty → [`WebmentionType::Unknown`]; the Microformats
    /// property names (`in-reply-to`, `like-of`, …) are aliased to the
    /// corresponding types; anything else → `Unknown`.
    pub fn from_raw(raw: Option<&str>) -> Self {
        let Some(raw) = raw else {
            return WebmentionType::Unknown;
        };
        match raw.trim().to_lowercase().as_str() {
            "in-reply-to" | "reply" => WebmentionType::Reply,
            "like-of" | "like" => WebmentionType::Like,
            "repost-of" | "repost" => WebmentionType::Repost,
            "bookmark-of" | "bookmark" => WebmentionType::Bookmark,
            "rsvp" => WebmentionType::Rsvp,
            "follow-of" | "follow" => WebmentionType::Follow,
            "mention" => WebmentionType::Mention,
            "" => WebmentionType::Unknown,
            _ => WebmentionType::Unknown,
        }
    }
}

impl Serialize for WebmentionType {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// Parse a raw value into an optional [`DateTime<Utc>`] — mirrors the
/// `_parse_dt` helper in `Webmention.build`.
///
/// Accepts ISO-8601 strings (naive values are interpreted as UTC) and
/// Unix timestamps (numbers).
pub fn parse_dt(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::Null => None,
        Value::String(s) if !s.trim().is_empty() => parse_dt_str(s.trim()),
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .and_then(|ts| Utc.timestamp_opt(ts, 0).single()),
        _ => None,
    }
}

/// Parse an ISO-8601 string — mirrors `datetime.fromisoformat`.
///
/// Naive datetimes (no timezone offset) are interpreted as UTC, like
/// the Python code does via `dt.replace(tzinfo=timezone.utc)`.
pub fn parse_dt_str(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(naive.and_utc());
        }
    }
    // Microformats dt-* values occasionally carry a space-separated
    // offset ("2020-01-01 12:00:00+0000") or a trailing "Z" without the
    // "T" separator — try a couple of forgiving shapes before giving up.
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M%:z",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    None
}

/// Serialize a datetime — mirrors `datetime.isoformat()`.
pub fn dt_to_iso(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.f%:z").to_string()
}

/// Data class representing a Webmention — mirrors `Webmention`.
#[derive(Debug, Clone, PartialEq)]
pub struct Webmention {
    /// The URL that mentioned the target.
    pub source: String,
    /// The URL that was mentioned.
    pub target: String,
    /// Incoming or outgoing.
    pub direction: WebmentionDirection,
    /// Title of the source page, if extracted.
    pub title: Option<String>,
    /// Short plain-text excerpt, if derived.
    pub excerpt: Option<String>,
    /// (Possibly HTML) content of the mention.
    pub content: Option<String>,
    /// Author display name.
    pub author_name: Option<String>,
    /// Author URL.
    pub author_url: Option<String>,
    /// Author photo/avatar URL.
    pub author_photo: Option<String>,
    /// Publication timestamp extracted from the source.
    pub published: Option<DateTime<Utc>>,
    /// Moderation status.
    pub status: WebmentionStatus,
    /// Classified mention type.
    pub mention_type: WebmentionType,
    /// The raw Microformats property name the type was inferred from.
    pub mention_type_raw: Option<String>,
    /// Extra structured metadata (mf2 payload, comments, …).
    pub metadata: Value,
    /// Storage timestamp.
    pub created_at: Option<DateTime<Utc>>,
    /// Last-update timestamp.
    pub updated_at: Option<DateTime<Utc>>,
}

impl Webmention {
    /// Create a bare mention with `source`, `target` and `direction` —
    /// the equivalent of `Webmention(source=…, target=…, direction=…)`.
    pub fn new(
        source: impl Into<String>,
        target: impl Into<String>,
        direction: WebmentionDirection,
    ) -> Self {
        Webmention {
            source: source.into(),
            target: target.into(),
            direction,
            title: None,
            excerpt: None,
            content: None,
            author_name: None,
            author_url: None,
            author_photo: None,
            published: None,
            status: WebmentionStatus::Confirmed,
            mention_type: WebmentionType::Unknown,
            mention_type_raw: None,
            metadata: Value::Object(Map::new()),
            created_at: None,
            updated_at: None,
        }
    }

    /// A dictionary representation — mirrors `Webmention.to_dict`.
    ///
    /// Enums serialize to their ``.value``, datetimes to ``isoformat()``
    /// strings.
    pub fn to_dict(&self) -> Value {
        let mut m = Map::new();
        m.insert("source".into(), Value::String(self.source.clone()));
        m.insert("target".into(), Value::String(self.target.clone()));
        m.insert(
            "direction".into(),
            Value::String(self.direction.as_str().to_string()),
        );
        m.insert("title".into(), opt_str(&self.title));
        m.insert("excerpt".into(), opt_str(&self.excerpt));
        m.insert("content".into(), opt_str(&self.content));
        m.insert("author_name".into(), opt_str(&self.author_name));
        m.insert("author_url".into(), opt_str(&self.author_url));
        m.insert("author_photo".into(), opt_str(&self.author_photo));
        m.insert("published".into(), opt_dt(&self.published));
        m.insert(
            "status".into(),
            Value::String(self.status.as_str().to_string()),
        );
        m.insert(
            "mention_type".into(),
            Value::String(self.mention_type.as_str().to_string()),
        );
        m.insert("mention_type_raw".into(), opt_str(&self.mention_type_raw));
        m.insert("metadata".into(), self.metadata.clone());
        m.insert("created_at".into(), opt_dt(&self.created_at));
        m.insert("updated_at".into(), opt_dt(&self.updated_at));
        Value::Object(m)
    }

    /// A hash value based on `(source, target, direction)` — mirrors
    /// `Webmention.__hash__`.
    pub fn hash_value(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.source.hash(&mut h);
        self.target.hash(&mut h);
        self.direction.as_str().hash(&mut h);
        h.finish()
    }

    /// Build a [`Webmention`] from a dictionary — mirrors
    /// `Webmention.build`.
    ///
    /// Unknown keys are ignored, `status` defaults to `Confirmed` and
    /// `mention_type` defaults to `Mention` (matching the Python
    /// semantics where a missing type means a plain mention on this
    /// code path).
    pub fn build(data: &Value, direction: WebmentionDirection) -> Result<Self> {
        let obj = data.as_object();
        let str_field = |key: &str| -> Option<String> {
            obj.and_then(|o| o.get(key))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        };

        let source = str_field("source")
            .ok_or_else(|| WebmentionError::InvalidValue("source is required".to_string()))?;
        let target = str_field("target")
            .ok_or_else(|| WebmentionError::InvalidValue("target is required".to_string()))?;

        let mention_type = match str_field("mention_type") {
            Some(raw) => WebmentionType::from_raw(Some(&raw)),
            None => WebmentionType::Mention,
        };

        let status = match str_field("status") {
            Some(raw) => WebmentionStatus::from_raw(&raw)?,
            None => WebmentionStatus::Confirmed,
        };

        let get =
            |key: &str| -> Option<&Value> { obj.and_then(|o| o.get(key)).filter(|v| !v.is_null()) };

        let metadata = match get("metadata") {
            Some(Value::Object(o)) => Value::Object(o.clone()),
            Some(v) => v.clone(),
            None => Value::Object(Map::new()),
        };

        // Mirrors the Python implementation, which always derives the
        // raw type from the resolved enum value on this code path.
        let mention_type_raw = Some(mention_type.as_str().to_string());

        Ok(Webmention {
            source,
            target,
            direction,
            title: str_field("title"),
            excerpt: str_field("excerpt"),
            content: str_field("content"),
            author_name: str_field("author_name"),
            author_url: str_field("author_url"),
            author_photo: str_field("author_photo"),
            published: get("published").and_then(parse_dt),
            status,
            mention_type,
            mention_type_raw,
            metadata,
            created_at: get("created_at").and_then(parse_dt),
            updated_at: get("updated_at").and_then(parse_dt),
        })
    }
}

impl Serialize for Webmention {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.to_dict().serialize(s)
    }
}

fn opt_str(v: &Option<String>) -> Value {
    v.as_ref()
        .map(|s| Value::String(s.clone()))
        .unwrap_or(Value::Null)
}

fn opt_dt(v: &Option<DateTime<Utc>>) -> Value {
    v.as_ref()
        .map(|d| Value::String(dt_to_iso(d)))
        .unwrap_or(Value::Null)
}
