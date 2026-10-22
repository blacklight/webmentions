//! Microformats2 parsing adapter — the Rust equivalent of `mf2py.parse`.
//!
//! Delegates to the `microformats` crate and re-serializes its typed
//! `Document` into the plain mf2 JSON shape `mf2py` returns
//! (`{"items": [{"type": [...], "properties": {...}, "children": [...]}]}`),
//! so the request parser can operate on `serde_json::Value` exactly
//! like the Python implementation operates on dicts.

use serde_json::Value;

/// Parse `html` into an mf2 JSON document — mirrors
/// `mf2py.parse(doc=html, url=base_url)`.
///
/// Returns `None` when the document cannot be parsed or `base_url` is
/// not a valid URL (matching the Python `except Exception: return
/// None` guard).
pub(crate) fn parse(html: &str, base_url: &str) -> Option<Value> {
    let base = url::Url::parse(base_url).ok()?;
    let doc = microformats::from_html(html, &base).ok()?;
    serde_json::to_value(&doc).ok()
}
