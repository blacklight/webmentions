//! Rust port of the webmentions test suite — mirrors the behaviors
//! covered by `tests/python`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{TimeZone, Utc};
use serde_json::json;
use webmentions_sdk::handlers::constants::{
    DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES, DEFAULT_MAX_SOURCE_RESPONSE_BYTES,
};
use webmentions_sdk::handlers::fetch::Fetcher;
use webmentions_sdk::handlers::incoming::IncomingOptions;
use webmentions_sdk::handlers::outgoing::OutgoingOptions;
use webmentions_sdk::handlers::parser::ParserOptions;
use webmentions_sdk::server::{append_link_header, dispatch, DispatchRequest};
use webmentions_sdk::storage::file::{ContentChangeType, FileSystemMonitor, FileSystemWatcher};
use webmentions_sdk::*;
use webmentions_sdk::{FetchRequest, FetchResponse};

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// In-memory storage used by handler/processor tests.
#[derive(Default)]
struct MemoryStorage {
    mentions: Mutex<Vec<Webmention>>,
}

impl MemoryStorage {
    fn all(&self) -> Vec<Webmention> {
        self.mentions.lock().unwrap().clone()
    }
}

impl WebmentionsStorage for MemoryStorage {
    fn store_webmention(&self, mention: &Webmention) -> Result<()> {
        let mut all = self.mentions.lock().unwrap();
        all.retain(|m| {
            !(m.source == mention.source
                && m.target == mention.target
                && m.direction == mention.direction)
        });
        all.push(mention.clone());
        Ok(())
    }

    fn delete_webmention(
        &self,
        source: &str,
        target: &str,
        direction: WebmentionDirection,
    ) -> Result<()> {
        self.mentions
            .lock()
            .unwrap()
            .retain(|m| !(m.source == source && m.target == target && m.direction == direction));
        Ok(())
    }

    fn retrieve_webmentions(
        &self,
        resource: &str,
        direction: WebmentionDirection,
    ) -> Result<Vec<Webmention>> {
        Ok(self
            .mentions
            .lock()
            .unwrap()
            .iter()
            .filter(|m| {
                m.direction == direction
                    && m.status == WebmentionStatus::Confirmed
                    && if direction == WebmentionDirection::Out {
                        m.source == resource
                    } else {
                        m.target == resource
                    }
            })
            .cloned()
            .collect())
    }
}

/// A scripted fetcher — maps URLs to canned responses and records the
/// requests it received.
struct StubFetch {
    responses: HashMap<String, FetchResponse>,
    calls: Arc<Mutex<Vec<FetchRequest>>>,
}

impl StubFetch {
    fn new() -> Self {
        StubFetch {
            responses: HashMap::new(),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with(mut self, url: &str, status: u16, body: &str) -> Self {
        self.responses.insert(
            url.to_string(),
            FetchResponse {
                status,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
                url: url.to_string(),
            },
        );
        self
    }

    fn with_headers(
        mut self,
        url: &str,
        status: u16,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Self {
        self.responses.insert(
            url.to_string(),
            FetchResponse {
                status,
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_lowercase(), v.to_string()))
                    .collect(),
                body: body.as_bytes().to_vec(),
                url: url.to_string(),
            },
        );
        self
    }

    fn fetcher(self) -> (Fetcher, Arc<Mutex<Vec<FetchRequest>>>) {
        let calls = self.calls;
        let calls_handle = calls.clone();
        let responses = self.responses;
        (
            Arc::new(move |req: &FetchRequest| {
                calls.lock().unwrap().push(req.clone());
                responses
                    .get(&req.url)
                    .cloned()
                    .ok_or_else(|| WebmentionError::Http(format!("no stub for {}", req.url)))
            }),
            calls_handle,
        )
    }
}

fn parser_with_fetcher(fetcher: Fetcher) -> WebmentionsRequestParser {
    WebmentionsRequestParser::new(ParserOptions {
        fetcher: Some(fetcher),
        ..Default::default()
    })
}

/// Recorded requests that carried a POST body.
fn post_calls(calls: &Mutex<Vec<FetchRequest>>) -> Vec<FetchRequest> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.data.is_some())
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------
// model — mirrors test_model_build_unit.py
// ---------------------------------------------------------------------

#[test]
fn direction_from_raw_accepts_values_and_names() {
    assert_eq!(
        WebmentionDirection::from_raw("incoming").unwrap(),
        WebmentionDirection::In
    );
    assert_eq!(
        WebmentionDirection::from_raw("OUTGOING").unwrap(),
        WebmentionDirection::Out
    );
    assert_eq!(
        WebmentionDirection::from_raw("in").unwrap(),
        WebmentionDirection::In
    );
    assert_eq!(
        WebmentionDirection::from_raw("OUT").unwrap(),
        WebmentionDirection::Out
    );
    assert!(WebmentionDirection::from_raw("bogus").is_err());
}

#[test]
fn mention_type_from_raw_aliases() {
    assert_eq!(
        WebmentionType::from_raw(Some("in-reply-to")),
        WebmentionType::Reply
    );
    assert_eq!(
        WebmentionType::from_raw(Some("like-of")),
        WebmentionType::Like
    );
    assert_eq!(
        WebmentionType::from_raw(Some("repost")),
        WebmentionType::Repost
    );
    assert_eq!(
        WebmentionType::from_raw(Some("bookmark-of")),
        WebmentionType::Bookmark
    );
    assert_eq!(WebmentionType::from_raw(Some("rsvp")), WebmentionType::Rsvp);
    assert_eq!(
        WebmentionType::from_raw(Some("follow-of")),
        WebmentionType::Follow
    );
    assert_eq!(
        WebmentionType::from_raw(Some("other")),
        WebmentionType::Unknown
    );
    assert_eq!(WebmentionType::from_raw(None), WebmentionType::Unknown);
}

#[test]
fn build_requires_source_and_target() {
    assert!(Webmention::build(&json!({}), WebmentionDirection::In).is_err());
    assert!(Webmention::build(&json!({"source": "https://a"}), WebmentionDirection::In).is_err());
    assert!(Webmention::build(&json!({"target": "https://b"}), WebmentionDirection::In).is_err());
}

#[test]
fn build_defaults_and_dt_parsing() {
    let wm = Webmention::build(
        &json!({
            "source": "https://s.example",
            "target": "https://t.example",
            "title": "Hi",
            "published": "2024-01-02T03:04:05+00:00",
            "created_at": 1704164645,
            "extra": "ignored",
        }),
        WebmentionDirection::In,
    )
    .unwrap();
    assert_eq!(wm.title.as_deref(), Some("Hi"));
    assert_eq!(wm.status, WebmentionStatus::Confirmed);
    assert_eq!(wm.mention_type, WebmentionType::Mention);
    assert_eq!(wm.mention_type_raw.as_deref(), Some("mention"));
    assert_eq!(
        wm.published,
        Some(Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap())
    );
    assert_eq!(wm.created_at.unwrap().timestamp(), 1704164645);
}

#[test]
fn to_dict_serializes_enums_and_datetimes() {
    let mut wm = Webmention::new("https://s", "https://t", WebmentionDirection::Out);
    wm.published = Some(Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap());
    wm.status = WebmentionStatus::Pending;
    wm.mention_type = WebmentionType::Like;
    let d = wm.to_dict();
    assert_eq!(d["direction"], "outgoing");
    assert_eq!(d["status"], "pending");
    assert_eq!(d["mention_type"], "like");
    assert_eq!(d["published"], "2024-01-02T03:04:05+00:00");
    assert_eq!(d["title"], serde_json::Value::Null);
}

#[test]
fn webmention_hash_matches_python_contract() {
    let a = Webmention::new("https://s", "https://t", WebmentionDirection::In);
    let b = Webmention::new("https://s", "https://t", WebmentionDirection::In);
    let c = Webmention::new("https://s", "https://t", WebmentionDirection::Out);
    assert_eq!(a.hash_value(), b.hash_value());
    assert_ne!(a.hash_value(), c.hash_value());
}

// ---------------------------------------------------------------------
// fetch — mirrors test_fetch_unit.py (policy checks only; DNS-stubbed
// coverage lives in the Python suite)
// ---------------------------------------------------------------------

#[test]
fn check_url_host_rejects_private_and_bad_input() {
    use webmentions_sdk::handlers::fetch::check_url_host;
    assert!(check_url_host("ftp://example.com/x").is_err());
    assert!(check_url_host("not a url").is_err());
    assert!(check_url_host("http://169.254.169.254/latest").is_err());
    assert!(check_url_host("http://127.0.0.1/x").is_err());
    assert!(check_url_host("http://10.0.0.5/x").is_err());
    assert!(check_url_host("http://192.168.1.1/x").is_err());
    assert!(check_url_host("http://[::1]/x").is_err());
    assert!(check_url_host("http://[fd00::1]/x").is_err());
}

// ---------------------------------------------------------------------
// parser — mirrors test_request_parser_unit.py
// ---------------------------------------------------------------------

#[test]
fn parse_requires_source_and_target() {
    let parser = parser_with_fetcher(StubFetch::new().fetcher().0);
    assert!(parser.parse(None, Some("https://t")).is_err());
    assert!(parser.parse(Some("https://s"), None).is_err());
    assert!(parser.parse(Some(""), Some("https://t")).is_err());
}

#[test]
fn parse_validates_target_domain() {
    let (fetcher, calls) = StubFetch::new().fetcher();
    let parser = WebmentionsRequestParser::new(ParserOptions {
        base_url: Some("https://example.com".to_string()),
        fetcher: Some(fetcher),
        ..Default::default()
    });
    let err = parser
        .parse(
            Some("https://src.example.net/post"),
            Some("https://evil.example.net/page"),
        )
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("Target URL domain does not match server domain"));
    // The fetch must never happen when validation fails.
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn parse_accepts_target_from_multiple_base_urls() {
    let target = "https://activitypub.example.com/article/test";
    let (fetcher, _) = StubFetch::new()
        .with(
            "https://source.example.net/post",
            200,
            &format!("<html><body><a href=\"{target}\">t</a></body></html>"),
        )
        .fetcher();
    let parser = WebmentionsRequestParser::new(ParserOptions {
        base_urls: vec![
            "https://blog.example.com".to_string(),
            "https://activitypub.example.com".to_string(),
        ],
        fetcher: Some(fetcher),
        ..Default::default()
    });
    let mention = parser
        .parse(Some("https://source.example.net/post"), Some(target))
        .unwrap();
    assert_eq!(mention.target, target);
}

#[test]
fn parse_base_url_and_base_urls_are_merged() {
    let target = "https://legacy.example.com/article/test";
    let (fetcher, _) = StubFetch::new()
        .with(
            "https://source.example.net/post",
            200,
            &format!("<html><body><a href=\"{target}\">t</a></body></html>"),
        )
        .fetcher();
    let parser = WebmentionsRequestParser::new(ParserOptions {
        base_url: Some("https://legacy.example.com".to_string()),
        base_urls: vec!["https://blog.example.com".to_string()],
        fetcher: Some(fetcher),
        ..Default::default()
    });
    assert!(parser
        .parse(Some("https://source.example.net/post"), Some(target))
        .is_ok());
}

#[test]
fn parse_gone_when_source_404() {
    let (fetcher, _) = StubFetch::new()
        .with("https://s.example/p", 404, "")
        .fetcher();
    let parser = parser_with_fetcher(fetcher);
    let err = parser
        .parse(Some("https://s.example/p"), Some("https://t.example/x"))
        .unwrap_err();
    assert!(err.is_gone());
}

#[test]
fn parse_gone_when_target_not_mentioned() {
    let (fetcher, _) = StubFetch::new()
        .with(
            "https://s.example/p",
            200,
            "<html><body><a href=\"https://other.example/x\">o</a></body></html>",
        )
        .fetcher();
    let parser = parser_with_fetcher(fetcher);
    let err = parser
        .parse(Some("https://s.example/p"), Some("https://t.example/x"))
        .unwrap_err();
    assert!(err.is_gone());
}

#[test]
fn parse_detects_target_via_src_and_raw_text() {
    let target = "https://t.example/x";
    for body in [
        format!("<img src=\"{target}\">"),
        format!("plain text mentioning {target} inline"),
    ] {
        let (fetcher, _) = StubFetch::new().with("https://s/p", 200, &body).fetcher();
        let parser = parser_with_fetcher(fetcher);
        assert!(
            parser.parse(Some("https://s/p"), Some(target)).is_ok(),
            "body should mention target: {body}"
        );
    }
}

#[test]
fn parse_fills_from_h_entry() {
    let target = "https://t.example/x";
    let html = format!(
        r#"<article class="h-entry">
          <h1 class="p-name">My Post https://stray.example</h1>
          <span class="u-author h-card">
            <a class="u-url" href="https://a.example">
              <img class="u-photo" src="https://a.example/me.png">
              <span class="p-name">Jane Doe</span>
            </a>
          </span>
          <time class="dt-published" datetime="2024-03-01T10:00:00+00:00"></time>
          <div class="e-content"><p>See <a href="{target}">this</a></p></div>
          <a class="u-in-reply-to" href="{target}"></a>
        </article>"#
    );
    let (fetcher, _) = StubFetch::new().with("https://s/p", 200, &html).fetcher();
    let parser = parser_with_fetcher(fetcher);
    let m = parser.parse(Some("https://s/p"), Some(target)).unwrap();

    assert_eq!(m.title.as_deref(), Some("My Post"));
    assert_eq!(m.author_name.as_deref(), Some("Jane Doe"));
    assert_eq!(m.author_url.as_deref(), Some("https://a.example/"));
    assert!(m.author_photo.is_some());
    assert_eq!(m.mention_type, WebmentionType::Reply);
    assert_eq!(m.mention_type_raw.as_deref(), Some("in-reply-to"));
    assert!(m.content.unwrap_or_default().contains("See"));
    assert!(m.published.is_some());
    let mf2 = &m.metadata["mf2"];
    assert_eq!(mf2["type"], json!(["h-entry"]));
    assert_eq!(mf2["in_reply_to"], json!([target]));
}

#[test]
fn parse_infers_like_and_rsvp_and_defaults_to_mention() {
    let target = "https://t.example/x";
    for (prop, expected, raw) in [
        ("u-like-of", WebmentionType::Like, "like-of"),
        ("u-repost-of", WebmentionType::Repost, "repost-of"),
        ("u-bookmark-of", WebmentionType::Bookmark, "bookmark-of"),
        ("u-follow-of", WebmentionType::Follow, "follow-of"),
    ] {
        let html = format!(
            r#"<article class="h-entry"><a class="{prop}" href="{target}"></a>
               <a href="{target}">t</a></article>"#
        );
        let (fetcher, _) = StubFetch::new().with("https://s/p", 200, &html).fetcher();
        let parser = parser_with_fetcher(fetcher);
        let m = parser.parse(Some("https://s/p"), Some(target)).unwrap();
        assert_eq!(m.mention_type, expected, "prop {prop}");
        assert_eq!(m.mention_type_raw.as_deref(), Some(raw));
    }

    // Plain link page without an h-entry → type stays "unknown",
    // matching the Python parser (the mention-type inference only
    // runs on h-entry properties).
    let html = format!("<a href=\"{target}\">t</a>");
    let (fetcher, _) = StubFetch::new().with("https://s/p", 200, &html).fetcher();
    let parser = parser_with_fetcher(fetcher);
    let m = parser.parse(Some("https://s/p"), Some(target)).unwrap();
    assert_eq!(m.mention_type, WebmentionType::Unknown);

    // h-entry with no type properties → "mention"
    let html = format!("<article class=\"h-entry\"><a href=\"{target}\">t</a></article>");
    let (fetcher, _) = StubFetch::new().with("https://s/p", 200, &html).fetcher();
    let parser = parser_with_fetcher(fetcher);
    let m = parser.parse(Some("https://s/p"), Some(target)).unwrap();
    assert_eq!(m.mention_type, WebmentionType::Mention);
}

#[test]
fn parse_html_fallbacks_fill_title_author_content() {
    let target = "https://t.example/x";
    let html = format!(
        r#"<html><head>
          <meta property="og:title" content="OG Title https://junk.example">
          <meta name="author" content="Fallback Author">
          <meta property="article:published_time" content="2024-02-03T04:05:06+00:00">
          <meta property="og:description" content="OG description">
        </head><body><a href="{target}">t</a></body></html>"#
    );
    let (fetcher, _) = StubFetch::new().with("https://s/p", 200, &html).fetcher();
    let parser = parser_with_fetcher(fetcher);
    let m = parser.parse(Some("https://s/p"), Some(target)).unwrap();
    assert_eq!(m.title.as_deref(), Some("OG Title"));
    assert_eq!(m.author_name.as_deref(), Some("Fallback Author"));
    assert_eq!(m.content.as_deref(), Some("OG description"));
    assert_eq!(
        m.published,
        Some(Utc.with_ymd_and_hms(2024, 2, 3, 4, 5, 6).unwrap())
    );
    assert_eq!(m.excerpt.as_deref(), Some("OG description"));
}

#[test]
fn parse_extracts_comments_and_mf2_metadata() {
    let target = "https://t.example/x";
    let html = format!(
        r#"<article class="h-entry">
          <a class="u-in-reply-to" href="{target}"></a>
          <a class="u-syndication" href="https://mastodon.example/@j/1"></a>
          <span class="p-category">rust</span>
          <div class="u-comment h-cite">
            <span class="p-name">First!</span>
            <a class="u-url" href="https://c.example/1"></a>
            <a class="u-author h-card" href="https://bob.example"><span class="p-name">Bob</span></a>
            <time class="dt-published" datetime="2024-03-02"></time>
            <div class="e-content"><p>Nice post</p></div>
          </div>
        </article>"#
    );
    let (fetcher, _) = StubFetch::new().with("https://s/p", 200, &html).fetcher();
    let parser = parser_with_fetcher(fetcher);
    let m = parser.parse(Some("https://s/p"), Some(target)).unwrap();
    let comments = m.metadata["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["name"], "First!");
    assert_eq!(comments[0]["author"]["name"], "Bob");
    let mf2 = &m.metadata["mf2"];
    assert_eq!(mf2["category"], json!(["rust"]));
    assert_eq!(mf2["syndication"], json!(["https://mastodon.example/@j/1"]));
}

#[test]
fn parse_derives_excerpt_from_content() {
    let target = "https://t.example/x";
    let long = "word ".repeat(120);
    let html = format!(
        r#"<article class="h-entry"><div class="e-content"><p>{long}</p></div>
          <a href="{target}">t</a></article>"#
    );
    let (fetcher, _) = StubFetch::new().with("https://s/p", 200, &html).fetcher();
    let parser = parser_with_fetcher(fetcher);
    let m = parser.parse(Some("https://s/p"), Some(target)).unwrap();
    let excerpt = m.excerpt.unwrap();
    assert!(excerpt.ends_with('…'));
    assert!(excerpt.chars().count() <= 241);
}

// ---------------------------------------------------------------------
// incoming processor — mirrors test_handler_unit.py
// ---------------------------------------------------------------------

#[test]
fn incoming_processes_and_stores_mention() {
    let target = "https://t.example/x";
    let (fetcher, _) = StubFetch::new()
        .with(
            "https://s.example/p",
            200,
            &format!("<a href=\"{target}\">t</a>"),
        )
        .fetcher();
    let storage = Arc::new(MemoryStorage::default());
    let processed: Arc<Mutex<Vec<Webmention>>> = Arc::new(Mutex::new(Vec::new()));
    let processed2 = processed.clone();
    let proc = webmentions_sdk::handlers::incoming::IncomingWebmentionsProcessor::new(
        storage.clone(),
        IncomingOptions {
            fetcher: Some(fetcher),
            on_mention_processed: Some(Arc::new(move |m| {
                processed2.lock().unwrap().push(m.clone());
            })),
            ..Default::default()
        },
    );
    let m = proc
        .process_incoming_webmention(Some("https://s.example/p"), Some(target))
        .unwrap()
        .unwrap();
    assert_eq!(m.status, WebmentionStatus::Confirmed);
    assert!(m.created_at.is_some() && m.updated_at.is_some());
    assert_eq!(processed.lock().unwrap().len(), 1);
    assert_eq!(storage.all().len(), 1);
}

#[test]
fn incoming_deletes_on_gone() {
    let (fetcher, _) = StubFetch::new()
        .with("https://s.example/p", 410, "")
        .fetcher();
    let storage = Arc::new(MemoryStorage::default());
    storage
        .store_webmention(&Webmention::new(
            "https://s.example/p",
            "https://t.example/x",
            WebmentionDirection::In,
        ))
        .unwrap();
    let deleted: Arc<Mutex<Vec<Webmention>>> = Arc::new(Mutex::new(Vec::new()));
    let deleted2 = deleted.clone();
    let proc = webmentions_sdk::handlers::incoming::IncomingWebmentionsProcessor::new(
        storage.clone(),
        IncomingOptions {
            fetcher: Some(fetcher),
            on_mention_deleted: Some(Arc::new(move |m| {
                deleted2.lock().unwrap().push(m.clone());
            })),
            ..Default::default()
        },
    );
    let res = proc
        .process_incoming_webmention(Some("https://s.example/p"), Some("https://t.example/x"))
        .unwrap();
    assert!(res.is_none());
    assert_eq!(deleted.lock().unwrap().len(), 1);
    assert!(storage.all().is_empty());
}

#[test]
fn incoming_returns_none_for_missing_args() {
    let proc = webmentions_sdk::handlers::incoming::IncomingWebmentionsProcessor::new(
        Arc::new(MemoryStorage::default()),
        IncomingOptions {
            fetcher: Some(StubFetch::new().fetcher().0),
            ..Default::default()
        },
    );
    assert!(proc
        .process_incoming_webmention(None, Some("https://t"))
        .unwrap()
        .is_none());
}

#[test]
fn incoming_wraps_invalid_values_in_webmention_error() {
    let (fetcher, _) = StubFetch::new().fetcher();
    let proc = webmentions_sdk::handlers::incoming::IncomingWebmentionsProcessor::new(
        Arc::new(MemoryStorage::default()),
        IncomingOptions {
            base_url: Some("https://example.com".to_string()),
            fetcher: Some(fetcher),
            ..Default::default()
        },
    );
    let err = proc
        .process_incoming_webmention(Some("https://s/p"), Some("https://evil.example/x"))
        .unwrap_err();
    assert!(matches!(err, WebmentionError::Webmention { .. }));
}

// ---------------------------------------------------------------------
// outgoing processor — mirrors test_outgoing_processor_unit.py
// ---------------------------------------------------------------------

fn outgoing_with_fetcher(
    storage: Arc<dyn WebmentionsStorage>,
    fetcher: Fetcher,
    options: OutgoingOptions,
) -> webmentions_sdk::handlers::outgoing::OutgoingWebmentionsProcessor {
    let mut options = options;
    options.fetcher = Some(fetcher);
    webmentions_sdk::handlers::outgoing::OutgoingWebmentionsProcessor::new(storage, options)
}

#[test]
fn outgoing_extracts_targets_from_html() {
    let (fetcher, calls) = StubFetch::new()
        .with(
            "https://t1.example",
            200,
            r#"<a href="/webmentions" rel="webmention">"#,
        )
        .fetcher();
    let proc = outgoing_with_fetcher(
        Arc::new(MemoryStorage::default()),
        fetcher,
        OutgoingOptions::default(),
    );
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some(
            r##"<p><a href="https://t1.example/page">t1</a>
               <img src="https://cdn.example/i.png">
               <a href="#frag">no</a></p>"##,
        ),
        Some(ContentTextFormat::Html),
    )
    .unwrap();
    let urls: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    // Both the link and the img src are discovery candidates.
    assert!(urls.contains(&"https://t1.example/page".to_string()));
    assert!(urls.contains(&"https://cdn.example/i.png".to_string()));
}

#[test]
fn outgoing_extracts_targets_from_markdown_and_text() {
    let (fetcher, calls) = StubFetch::new().fetcher();
    let proc = outgoing_with_fetcher(
        Arc::new(MemoryStorage::default()),
        fetcher,
        OutgoingOptions::default(),
    );
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some(
            "see [a](https://a.example/p) and <https://b.example/x> \
             and bare https://c.example/y, trailing https://d.example/z.",
        ),
        Some(ContentTextFormat::Markdown),
    )
    .unwrap();
    let urls: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    for u in [
        "https://a.example/p",
        "https://b.example/x",
        "https://c.example/y",
        "https://d.example/z",
    ] {
        assert!(urls.contains(&u.to_string()), "missing {u}");
    }
}

#[test]
fn outgoing_skips_local_targets_when_enabled() {
    let (fetcher, calls) = StubFetch::new().fetcher();
    let proc = outgoing_with_fetcher(
        Arc::new(MemoryStorage::default()),
        fetcher,
        OutgoingOptions {
            base_url: Some("https://me.example".to_string()),
            exclude_local_targets: true,
            ..Default::default()
        },
    );
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some("local https://me.example/other remote https://r.example/x"),
        Some(ContentTextFormat::Text),
    )
    .unwrap();
    let urls: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert!(!urls.contains(&"https://me.example/other".to_string()));
    assert!(urls.contains(&"https://r.example/x".to_string()));
}

#[test]
fn outgoing_discovers_endpoint_via_link_header() {
    let (fetcher, calls) = StubFetch::new()
        .with_headers(
            "https://t.example/page",
            200,
            &[(
                "Link",
                "</wm-endpoint>; rel=\"webmention\", </other>; rel=\"canonical\"",
            )],
            "<html></html>",
        )
        .with("https://t.example/wm-endpoint", 202, "")
        .fetcher();
    let storage = Arc::new(MemoryStorage::default());
    let proc = outgoing_with_fetcher(storage.clone(), fetcher, OutgoingOptions::default());
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some("<a href=\"https://t.example/page\">t</a>"),
        Some(ContentTextFormat::Html),
    )
    .unwrap();
    let posts = post_calls(&calls);
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].url, "https://t.example/wm-endpoint");
    let data = posts[0].data.as_ref().unwrap();
    assert_eq!(data["source"], "https://me.example/post");
    assert_eq!(data["target"], "https://t.example/page");
    assert_eq!(storage.all().len(), 1);
}

#[test]
fn outgoing_discovers_endpoint_via_link_tag() {
    let (fetcher, calls) = StubFetch::new()
        .with(
            "https://t.example/page",
            200,
            r#"<html><head><link rel="webmention" href="/wm"></head></html>"#,
        )
        .with("https://t.example/wm", 200, "")
        .fetcher();
    let proc = outgoing_with_fetcher(
        Arc::new(MemoryStorage::default()),
        fetcher,
        OutgoingOptions::default(),
    );
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some("<a href=\"https://t.example/page\">t</a>"),
        Some(ContentTextFormat::Html),
    )
    .unwrap();
    let posts = post_calls(&calls);
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].url, "https://t.example/wm");
}

#[test]
fn outgoing_discovers_endpoint_via_anchor() {
    let (fetcher, calls) = StubFetch::new()
        .with(
            "https://t.example/page",
            200,
            r#"<a rel="webmention" href="https://wm.example/ep">wm</a>"#,
        )
        .with("https://wm.example/ep", 200, "")
        .fetcher();
    let proc = outgoing_with_fetcher(
        Arc::new(MemoryStorage::default()),
        fetcher,
        OutgoingOptions::default(),
    );
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some("<a href=\"https://t.example/page\">t</a>"),
        Some(ContentTextFormat::Html),
    )
    .unwrap();
    let posts = post_calls(&calls);
    assert_eq!(posts[0].url, "https://wm.example/ep");
}

#[test]
fn outgoing_skips_discovery_for_non_html_and_oversized() {
    let (fetcher, calls) = StubFetch::new()
        .with_headers(
            "https://t.example/img.png",
            200,
            &[("Content-Type", "image/png")],
            "binary",
        )
        .with_headers(
            "https://t.example/big",
            200,
            &[
                ("Content-Type", "text/html"),
                ("Content-Length", "99999999"),
            ],
            "<a rel=\"webmention\" href=\"/x\">",
        )
        .fetcher();
    let proc = outgoing_with_fetcher(
        Arc::new(MemoryStorage::default()),
        fetcher,
        OutgoingOptions::default(),
    );
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some("<a href=\"https://t.example/img.png\">i</a><a href=\"https://t.example/big\">b</a>"),
        Some(ContentTextFormat::Html),
    )
    .unwrap();
    // No POST was attempted for either target.
    assert!(calls.lock().unwrap().iter().all(|r| r.data.is_none()));
}

#[test]
fn outgoing_notifies_removed_targets() {
    let target = "https://gone.example/x";
    let (fetcher, _) = StubFetch::new()
        .with(
            target,
            200,
            r#"<a rel="webmention" href="https://gone.example/wm"></a>"#,
        )
        .with("https://gone.example/wm", 200, "")
        .fetcher();
    let storage = Arc::new(MemoryStorage::default());
    let mut old = Webmention::new("https://me.example/post", target, WebmentionDirection::Out);
    old.published = Some(Utc::now());
    storage.store_webmention(&old).unwrap();
    let deleted: Arc<Mutex<Vec<Webmention>>> = Arc::new(Mutex::new(Vec::new()));
    let deleted2 = deleted.clone();
    let options = OutgoingOptions {
        on_mention_deleted: Some(Arc::new(move |m| {
            deleted2.lock().unwrap().push(m.clone());
        })),
        ..Default::default()
    };
    let proc = outgoing_with_fetcher(storage.clone(), fetcher, options);
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some("no more links"),
        Some(ContentTextFormat::Text),
    )
    .unwrap();
    assert_eq!(deleted.lock().unwrap().len(), 1);
    assert!(storage.all().is_empty());
}

#[test]
fn outgoing_ignores_self_mentions() {
    let (fetcher, calls) = StubFetch::new().fetcher();
    let proc = outgoing_with_fetcher(
        Arc::new(MemoryStorage::default()),
        fetcher,
        OutgoingOptions::default(),
    );
    proc.process_outgoing_webmentions(
        "https://me.example/post",
        Some("self https://me.example/post"),
        Some(ContentTextFormat::Text),
    )
    .unwrap();
    assert!(calls.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------
// db storage — mirrors test_db_storage_integration.py
// ---------------------------------------------------------------------

#[test]
fn db_storage_roundtrip_and_idempotent_update() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = SqliteWebmentionsStorage::new(tmp.path().join("wm.sqlite")).unwrap();
    let published = Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
    let mut wm = Webmention::new("https://e/s", "https://e/t", WebmentionDirection::In);
    wm.author_name = Some("John".into());
    wm.author_url = Some("https://e/j".into());
    wm.author_photo = Some("https://e/j.jpg".into());
    wm.mention_type = WebmentionType::Mention;
    wm.published = Some(published);
    storage.store_webmention(&wm).unwrap();

    let got = storage
        .retrieve_webmentions("https://e/t", WebmentionDirection::In)
        .unwrap();
    assert_eq!(got.len(), 1);
    let stored = &got[0];
    assert_eq!(stored.author_name.as_deref(), Some("John"));
    assert_eq!(stored.published, Some(published));
    assert!(stored.created_at.is_some() && stored.updated_at.is_some());

    // Upsert updates fields, keeps created_at.
    let mut updated = wm.clone();
    updated.author_name = Some("Jane".into());
    updated.title = Some("New".into());
    storage.store_webmention(&updated).unwrap();
    let got2 = storage
        .retrieve_webmentions("https://e/t", WebmentionDirection::In)
        .unwrap();
    assert_eq!(got2.len(), 1);
    assert_eq!(got2[0].author_name.as_deref(), Some("Jane"));
    assert_eq!(got2[0].title.as_deref(), Some("New"));
    assert_eq!(got2[0].created_at, stored.created_at);
}

#[test]
fn db_storage_delete_and_direction_filter() {
    let storage = SqliteWebmentionsStorage::new(":memory:").unwrap();
    let mut wm = Webmention::new("https://s", "https://t", WebmentionDirection::Out);
    wm.published = Some(Utc::now());
    storage.store_webmention(&wm).unwrap();
    // OUT mentions are retrieved by source.
    assert_eq!(
        storage
            .retrieve_webmentions("https://s", WebmentionDirection::Out)
            .unwrap()
            .len(),
        1
    );
    assert!(storage
        .retrieve_webmentions("https://t", WebmentionDirection::Out)
        .unwrap()
        .is_empty());
    storage
        .delete_webmention("https://s", "https://t", WebmentionDirection::Out)
        .unwrap();
    assert!(storage
        .retrieve_webmentions("https://s", WebmentionDirection::Out)
        .unwrap()
        .is_empty());
}

#[test]
fn db_storage_skips_pending_mentions() {
    let storage = SqliteWebmentionsStorage::new(":memory:").unwrap();
    let mut wm = Webmention::new("https://s", "https://t", WebmentionDirection::In);
    wm.status = WebmentionStatus::Pending;
    storage.store_webmention(&wm).unwrap();
    assert!(storage
        .retrieve_webmentions("https://t", WebmentionDirection::In)
        .unwrap()
        .is_empty());
}

// ---------------------------------------------------------------------
// renderer — mirrors test_renderer_unit.py
// ---------------------------------------------------------------------

fn minimal_mention() -> Webmention {
    let mut wm = Webmention::new(
        "https://source.example/post",
        "https://target.example/article",
        WebmentionDirection::In,
    );
    wm.title = Some("A title".into());
    wm.author_name = Some("Alice".into());
    wm.author_url = Some("https://source.example/".into());
    wm.created_at = Some(Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 0).unwrap());
    wm
}

#[test]
fn renderer_renders_default_template() {
    let r = WebmentionsRenderer::new();
    let html = r.render_webmention(&minimal_mention(), None).unwrap();
    assert!(html.contains(r#"<div class="wm-mention">"#));
    assert!(html.contains("A title"));
    assert!(html.contains("source.example"));
    assert!(html.contains("Jan 02, 2024 at 03:04"));
}

#[test]
fn renderer_renders_template_string_and_file() {
    let r = WebmentionsRenderer::new();
    let mut wm = minimal_mention();
    wm.title = Some("Hello".into());
    let html = r
        .render_webmention(&wm, Some("<p>{{ mention.title }}</p>"))
        .unwrap();
    assert_eq!(html.trim(), "<p>Hello</p>");

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "<h1>{{ mention.title }}</h1>").unwrap();
    let html = r
        .render_webmention(&wm, Some(tmp.path().to_str().unwrap()))
        .unwrap();
    assert_eq!(html.trim(), "<h1>Hello</h1>");
}

#[test]
fn renderer_renders_mf2_fields_and_sanitizes_urls() {
    let r = WebmentionsRenderer::new();
    let mut wm = minimal_mention();
    wm.metadata = serde_json::json!({"mf2": {"like_of": ["https://example.com/liked"]}});
    let html = r.render_webmention(&wm, None).unwrap();
    assert!(html.contains("https://example.com/liked"));
    assert!(html.contains("example.com"));

    let mut bad = minimal_mention();
    bad.source = "javascript:alert(1)".into();
    bad.author_url = Some("javascript:alert(2)".into());
    bad.author_photo = Some("javascript:alert(3)".into());
    bad.metadata = serde_json::json!({"mf2": {
        "like_of": ["javascript:alert(4)", "https://example.com/ok"],
        "photo": ["javascript:alert(5)", "https://example.com/p.jpg"],
    }});
    let html = r.render_webmention(&bad, None).unwrap();
    assert!(!html.contains("javascript:alert"));
    assert!(html.contains("https://example.com/ok"));
    assert!(html.contains("https://example.com/p.jpg"));
}

#[test]
fn renderer_collapsible_toggle_threshold() {
    let r = WebmentionsRenderer::new();
    let mut wm = minimal_mention();
    wm.content = Some(format!("<p>{}</p>", "x".repeat(1100)));
    let html = r.render_webmention(&wm, None).unwrap();
    assert!(html.contains("wm-mention-collapsible"));
    assert!(html.contains("wm-mention-toggle"));
    assert!(html.contains("wm-mention-expand"));

    wm.content = Some("<p>Short</p>".into());
    let html = r.render_webmention(&wm, None).unwrap();
    assert!(!html.contains("wm-mention-toggle"));

    wm.content = None;
    wm.excerpt = Some("y".repeat(1100));
    let html = r.render_webmention(&wm, None).unwrap();
    assert!(html.contains("wm-mention-collapsible"));
}

#[test]
fn renderer_does_not_render_literal_none_content() {
    let r = WebmentionsRenderer::new();
    let mut wm = minimal_mention();
    wm.content = Some("None".into());
    let html = r.render_webmention(&wm, None).unwrap();
    assert!(!html.contains(">None<"));
}

#[test]
fn renderer_collection_renders_counts() {
    let r = WebmentionsRenderer::new();
    let mut likes = minimal_mention();
    likes.mention_type = WebmentionType::Like;
    let mut reply = minimal_mention();
    reply.mention_type = WebmentionType::Reply;
    let html = r.render_webmentions(&[likes, reply], None).unwrap();
    assert!(html.contains("wm-mentions"));
    assert!(html.contains("wm-mentions-summary"));
    assert!(html.contains("wm-mention-collapsible")); // from the <style>
}

#[test]
fn sanitize_html_strips_unsafe() {
    use webmentions_sdk::render::sanitize_html;
    let out =
        sanitize_html(r#"<a href="javascript:alert(1)">x</a><script>alert(2)</script><p>ok</p>"#);
    assert!(!out.contains("javascript"));
    assert!(!out.contains("script"));
    assert!(out.contains("<p>ok</p>"));
}

// ---------------------------------------------------------------------
// handler façade + server dispatch
// ---------------------------------------------------------------------

#[test]
fn handler_process_incoming_end_to_end() {
    let target = "https://me.example/article";
    let (fetcher, _) = StubFetch::new()
        .with(
            "https://remote.example/post",
            200,
            &format!("<a href=\"{target}\">t</a>"),
        )
        .fetcher();
    let handler = WebmentionsHandler::new(
        Arc::new(MemoryStorage::default()),
        HandlerOptions {
            base_url: Some("https://me.example".to_string()),
            fetcher: Some(fetcher),
            ..Default::default()
        },
    );
    let m = handler
        .process_incoming_webmention(Some("https://remote.example/post"), Some(target))
        .unwrap()
        .unwrap();
    assert_eq!(m.target, target);
    assert_eq!(
        handler
            .retrieve_stored_webmentions(target, WebmentionDirection::In)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn dispatch_post_and_get() {
    let target = "https://me.example/article";
    let (fetcher, _) = StubFetch::new()
        .with(
            "https://r.example/p",
            200,
            &format!("<a href=\"{target}\">x</a>"),
        )
        .fetcher();
    let handler = Arc::new(WebmentionsHandler::new(
        Arc::new(MemoryStorage::default()),
        HandlerOptions {
            base_url: Some("https://me.example".to_string()),
            fetcher: Some(fetcher),
            ..Default::default()
        },
    ));

    let req = DispatchRequest {
        method: "POST".into(),
        path: "/webmentions".into(),
        body: b"source=https%3A%2F%2Fr.example%2Fp&target=https%3A%2F%2Fme.example%2Farticle"
            .to_vec(),
        ..Default::default()
    };
    let resp = dispatch(&handler, &req, "/webmentions");
    assert_eq!(resp.status, 200);

    let req = DispatchRequest {
        method: "GET".into(),
        path: "/webmentions".into(),
        query: [
            ("resource".to_string(), target.to_string()),
            ("direction".to_string(), "incoming".to_string()),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let resp = dispatch(&handler, &req, "/webmentions");
    assert_eq!(resp.status, 200);
    let list: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["source"], "https://r.example/p");

    // Missing params → 400.
    let req = DispatchRequest {
        method: "GET".into(),
        path: "/webmentions".into(),
        ..Default::default()
    };
    assert_eq!(dispatch(&handler, &req, "/webmentions").status, 400);

    // Bad direction → 400.
    let req = DispatchRequest {
        method: "GET".into(),
        path: "/webmentions".into(),
        query: [
            ("resource".to_string(), target.to_string()),
            ("direction".to_string(), "sideways".to_string()),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    assert_eq!(dispatch(&handler, &req, "/webmentions").status, 400);
}

#[test]
fn link_header_helpers() {
    assert_eq!(
        webmentions_sdk::server::webmention_link_header_value("/wm"),
        "</wm>; rel=\"webmention\""
    );
    assert_eq!(append_link_header(None, "a").as_deref(), Some("a"));
    assert_eq!(append_link_header(Some("a"), "a").as_deref(), Some("a"));
    assert_eq!(append_link_header(Some("a"), "b").as_deref(), Some("a, b"));
}

// ---------------------------------------------------------------------
// file watcher/monitor — mirrors test_file_watcher_unit.py and the e2e
// filesystem test
// ---------------------------------------------------------------------

#[test]
fn watcher_emits_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let events: Arc<Mutex<Vec<(ContentChangeType, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let events2 = events.clone();
    let watcher = FileSystemWatcher::new(
        tmp.path(),
        Arc::new(move |c| {
            events2
                .lock()
                .unwrap()
                .push((c.change_type, c.path.clone()))
        }),
        None,
        Some(0.0),
    );
    watcher.start();
    let f = tmp.path().join("post.md");
    std::fs::write(&f, "hello https://a.example/x").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    std::fs::remove_file(&f).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    watcher.stop();
    let evs = events.lock().unwrap();
    assert!(!evs.is_empty(), "expected at least one fs event");
}

#[test]
fn monitor_maps_file_changes_to_outgoing() {
    let target = "https://remote.example/page";
    let (fetcher, calls) = StubFetch::new()
        .with(
            target,
            200,
            r#"<a rel="webmention" href="https://remote.example/wm"></a>"#,
        )
        .with("https://remote.example/wm", 200, "")
        .fetcher();
    let handler = Arc::new(WebmentionsHandler::new(
        Arc::new(MemoryStorage::default()),
        HandlerOptions {
            fetcher: Some(fetcher),
            ..Default::default()
        },
    ));
    let tmp = tempfile::tempdir().unwrap();
    let monitor = FileSystemMonitor::new(
        handler,
        tmp.path(),
        Arc::new(|path| {
            format!(
                "https://me.example/{}",
                std::path::Path::new(path)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
            )
        }),
        None,
        Some(0.0),
    );
    monitor.start();
    std::fs::write(tmp.path().join("post.md"), format!("see [post]({target})")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    monitor.stop();
    let posted = calls
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.data.is_some() && r.url == "https://remote.example/wm");
    assert!(posted, "expected outgoing webmention POST to endpoint");
}

#[test]
fn constants_match_python_defaults() {
    assert_eq!(DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES, 1024 * 1024);
    assert_eq!(DEFAULT_MAX_SOURCE_RESPONSE_BYTES, 1024 * 1024);
}
