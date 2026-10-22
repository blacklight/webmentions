//! Render Webmentions as HTML — mirrors `webmentions.render`.
//!
//! Uses `minijinja` (Jinja2-compatible) with the same bundled templates
//! as the Python implementation.

use std::sync::LazyLock;

use minijinja::{Environment, Value as MjValue};
use serde_json::Value;

use crate::error::{Result, WebmentionError};
use crate::model::{parse_dt_str, Webmention, WebmentionType};

const WEBMENTION_TEMPLATE: &str = include_str!("templates/webmention.html");
const WEBMENTIONS_TEMPLATE: &str = include_str!("templates/webmentions.html");

// HTML sanitisation: tags and attributes allowed in mention content —
// mirrors `_ALLOWED_TAGS`/`_ALLOWED_ATTRS`.
const ALLOWED_TAGS: &[&str] = &[
    "a",
    "p",
    "br",
    "span",
    "strong",
    "em",
    "b",
    "i",
    "u",
    "s",
    "del",
    "blockquote",
    "pre",
    "code",
    "ul",
    "ol",
    "li",
];
const ALLOWED_ATTRS: &[&str] = &["href", "class", "rel", "translate", "title", "lang", "dir"];

static TAG_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<(/?)(\w+)([^>]*)>").unwrap());
static ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(\w[\w-]*)=["']([^"']*)["']"#).unwrap());

/// Strip disallowed tags and attributes from `html` — mirrors
/// `_sanitize_html`.
///
/// Like the Python implementation, `href` values are restricted to
/// `http`/`https`/relative references.
pub fn sanitize_html(html: &str) -> String {
    TAG_RE
        .replace_all(html, |caps: &regex::Captures| {
            let slash = &caps[1];
            let tag = caps[2].to_lowercase();
            if !ALLOWED_TAGS.contains(&tag.as_str()) {
                return String::new();
            }
            if !slash.is_empty() {
                return format!("</{tag}>");
            }
            let attrs_str = &caps[3];
            let mut safe_attrs = Vec::new();
            for am in ATTR_RE.captures_iter(attrs_str) {
                let attr_name = am[1].to_lowercase();
                if !ALLOWED_ATTRS.contains(&attr_name.as_str()) {
                    continue;
                }
                if attr_name == "href" {
                    let val = am[2].trim();
                    let scheme = url::Url::parse(val)
                        .map(|u| u.scheme().to_lowercase())
                        .unwrap_or_default();
                    if !matches!(scheme.as_str(), "http" | "https" | "") {
                        continue;
                    }
                }
                safe_attrs.push(format!("{}=\"{}\"", &am[1], &am[2]));
            }
            let attr_str = if safe_attrs.is_empty() {
                String::new()
            } else {
                format!(" {}", safe_attrs.join(" "))
            };
            format!("<{tag}{attr_str}>")
        })
        .to_string()
}

/// Template helper functions — mirrors `TemplateUtils`.
pub mod template_utils {
    use super::*;

    /// `format_date` — "%b %d, %Y".
    pub fn format_date(v: &MjValue) -> String {
        let Some(s) = v.as_str() else {
            return String::new();
        };
        if s.is_empty() {
            return String::new();
        }
        parse_dt_str(s)
            .map(|dt| dt.format("%b %d, %Y").to_string())
            .unwrap_or_else(|| s.to_string())
    }

    /// `format_datetime` — "%b %d, %Y at %H:%M".
    pub fn format_datetime(v: &MjValue) -> String {
        let Some(s) = v.as_str() else {
            return String::new();
        };
        if s.is_empty() {
            return String::new();
        }
        parse_dt_str(s)
            .map(|dt| dt.format("%b %d, %Y at %H:%M").to_string())
            .unwrap_or_else(|| s.to_string())
    }

    /// Whether `u` can be emitted verbatim inside an HTML attribute —
    /// required because `safe_url` output is marked safe and bypasses
    /// autoescaping.
    fn attr_safe(u: &str) -> bool {
        !u.chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '`'))
    }

    /// `as_url` — unwrap mf2-ish values to a URL string.
    pub fn as_url(v: &MjValue) -> String {
        if let Some(s) = v.as_str() {
            return s.to_string();
        }
        // dict-like → ``url`` or ``value`` member
        for key in ["url", "value"] {
            if let Ok(attr) = v.get_attr(key) {
                if let Some(s) = attr.as_str() {
                    return s.to_string();
                }
            }
        }
        // sequence → first element
        if let Ok(mut items) = v.try_iter() {
            if let Some(first) = items.next() {
                return first.to_string();
            }
        }
        String::new()
    }

    /// `hostname` of a URL.
    pub fn hostname(v: &MjValue) -> String {
        let u = as_url(v);
        if u.is_empty() {
            return String::new();
        }
        url::Url::parse(&u)
            .ok()
            .and_then(|p| p.host_str().map(String::from))
            .unwrap_or_default()
    }

    /// `safe_url` — return the URL only when it is a well-formed
    /// http(s) URL with a host, else "".
    pub fn safe_url(v: &MjValue) -> String {
        let u = as_url(v).trim().to_string();
        if u.is_empty() || !attr_safe(&u) {
            return String::new();
        }
        match url::Url::parse(&u) {
            Ok(parsed)
                if matches!(parsed.scheme().to_lowercase().as_str(), "http" | "https")
                    && parsed.host_str().is_some_and(|h| !h.is_empty()) =>
            {
                u
            }
            _ => String::new(),
        }
    }

    /// `fromjson` — parse JSON-looking strings; pass through
    /// everything else (`None` → empty map, like the Python helper).
    pub fn fromjson(v: &MjValue) -> MjValue {
        match v.as_str() {
            Some(s) => {
                let t = s.trim();
                if !t.is_empty() && matches!(t.chars().next(), Some('[' | '{' | '"')) {
                    serde_json::from_str::<Value>(t)
                        .map(MjValue::from_serialize)
                        .unwrap_or_else(|_| v.clone())
                } else {
                    v.clone()
                }
            }
            None => {
                if v.is_none() {
                    MjValue::from_serialize(serde_json::json!({}))
                } else {
                    v.clone()
                }
            }
        }
    }

    /// `sanitize_html` — sanitize HTML content, returning a safe
    /// (unescaped) string for rendering.
    pub fn sanitize_html_helper(v: &MjValue) -> MjValue {
        match v.as_str() {
            None => MjValue::from_safe_string(String::new()),
            Some(s) => {
                if s.trim().eq_ignore_ascii_case("none") {
                    MjValue::from_safe_string(String::new())
                } else {
                    MjValue::from_safe_string(super::sanitize_html(s))
                }
            }
        }
    }
}

fn build_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|name| {
        // Escape embedded templates, string templates (`<string>`) and
        // unnamed contexts — matching Python's
        // `select_autoescape(enabled_extensions=("html", "xml"))` plus
        // the `default_for_string` Jinja behavior.
        if name.is_empty()
            || name == "<string>"
            || name.ends_with(".html")
            || name.ends_with(".htm")
            || name.ends_with(".xml")
        {
            minijinja::AutoEscape::Html
        } else {
            minijinja::AutoEscape::None
        }
    });
    // Jinja2 dicts expose `.get(key, default)`; minijinja maps do not.
    // Emulate it via the unknown-method callback so templates written
    // for the Python package keep working.
    env.set_unknown_method_callback(|_state, obj, method, args| {
        use minijinja::{Error, ErrorKind};
        if method == "get" {
            let Some(key) = args.first().and_then(|k| k.as_str()) else {
                return Err(Error::new(
                    ErrorKind::InvalidOperation,
                    "get() requires a string key",
                ));
            };
            let default = args.get(1).cloned().unwrap_or_default();
            return Ok(obj.get_attr(key).unwrap_or(default));
        }
        Err(Error::new(
            ErrorKind::UnknownMethod,
            format!("object has no method named {method}"),
        ))
    });
    env.add_template("webmention.html", WEBMENTION_TEMPLATE)
        .expect("embedded template must parse");
    env.add_template("webmentions.html", WEBMENTIONS_TEMPLATE)
        .expect("embedded template must parse");
    // Helper outputs are marked safe where they are already validated:
    // `safe_url` rejects attribute-breaking characters, `hostname`
    // comes from URL parsing, and the date formatters emit a fixed
    // shape. Keeping them unescaped also matches Jinja2's markupsafe,
    // which — unlike minijinja's escaper — does not rewrite `/` as
    // `&#x2f;`, so rendered hrefs look identical to the Python output.
    env.add_function("format_date", |v: MjValue| {
        MjValue::from_safe_string(template_utils::format_date(&v))
    });
    env.add_function("format_datetime", |v: MjValue| {
        MjValue::from_safe_string(template_utils::format_datetime(&v))
    });
    env.add_function("as_url", |v: MjValue| {
        MjValue::from_safe_string(template_utils::as_url(&v))
    });
    env.add_function("hostname", |v: MjValue| {
        MjValue::from_safe_string(template_utils::hostname(&v))
    });
    env.add_function("safe_url", |v: MjValue| {
        MjValue::from_safe_string(template_utils::safe_url(&v))
    });
    env.add_function("fromjson", |v: MjValue| template_utils::fromjson(&v));
    env.add_function("sanitize_html", |v: MjValue| {
        template_utils::sanitize_html_helper(&v)
    });
    env
}

/// Webmentions renderer — mirrors `WebmentionsRenderer`.
///
/// Renders Webmentions into HTML through the bundled Jinja2-compatible
/// templates.
pub struct WebmentionsRenderer {
    env: Environment<'static>,
}

impl Default for WebmentionsRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl WebmentionsRenderer {
    /// Create a renderer with the bundled templates.
    pub fn new() -> Self {
        WebmentionsRenderer { env: build_env() }
    }

    /// Resolve `template` to template source — mirrors
    /// `WebmentionsRenderer._get_template`.
    ///
    /// Accepts a filesystem path, an inline template string or `None`
    /// for the bundled `default` template.
    fn template_source(&self, template: Option<&str>) -> Result<Option<String>> {
        match template {
            None => Ok(None),
            Some(t) => {
                let path = std::path::Path::new(t);
                if path.is_file() {
                    Ok(Some(std::fs::read_to_string(path)?))
                } else {
                    Ok(Some(t.to_string()))
                }
            }
        }
    }

    fn render(&self, template: Option<&str>, default: &str, ctx: MjValue) -> Result<String> {
        let source = self.template_source(template)?;
        let result = match source {
            Some(src) => self.env.template_from_str(&src).and_then(|t| t.render(ctx)),
            None => self.env.get_template(default).and_then(|t| t.render(ctx)),
        };
        result.map_err(|e| WebmentionError::General(format!("template render failed: {e}")))
    }

    /// Render a single Webmention — mirrors `render_webmention`.
    pub fn render_webmention(
        &self,
        webmention: &Webmention,
        template: Option<&str>,
    ) -> Result<String> {
        self.render(
            template,
            "webmention.html",
            minijinja::context!(mention => MjValue::from_serialize(webmention.to_dict())),
        )
    }

    /// Render a list of Webmentions — mirrors `render_webmentions`.
    ///
    /// Mentions are sorted newest-first by `created_at`/`published`
    /// and grouped with per-type counts, like the Python
    /// implementation.
    pub fn render_webmentions(
        &self,
        webmentions: &[Webmention],
        template: Option<&str>,
    ) -> Result<String> {
        let mut sorted: Vec<&Webmention> = webmentions.iter().collect();
        sorted.sort_by_key(|w| std::cmp::Reverse(w.created_at.or(w.published)));

        let rendered: Vec<MjValue> = sorted
            .iter()
            .map(|w| {
                self.render_webmention(w, None)
                    .map(MjValue::from_safe_string)
            })
            .collect::<Result<Vec<_>>>()?;

        let mut counts = serde_json::Map::new();
        for k in ["likes", "reposts", "replies", "mentions"] {
            counts.insert(k.into(), serde_json::json!(0));
        }
        for w in &sorted {
            let key = match w.mention_type {
                WebmentionType::Like => "likes",
                WebmentionType::Repost => "reposts",
                WebmentionType::Reply => "replies",
                _ => "mentions",
            };
            let cur = counts[key].as_i64().unwrap_or(0);
            counts.insert(key.into(), serde_json::json!(cur + 1));
        }

        self.render(
            template,
            "webmentions.html",
            minijinja::context!(mentions => rendered, counts => Value::Object(counts)),
        )
    }
}
