//! SSRF-guarded HTTP fetch — mirrors `webmentions.handlers._fetch`.
//!
//! A Webmention receiver accepts an arbitrary ``source`` URL that it
//! then fetches, and outgoing delivery follows URLs a remote author's
//! content points at. A bare HTTP GET gives an attacker three things:
//! an internal-network read primitive (loopback, link-local metadata
//! endpoints, RFC1918 services reachable from the process), a redirect
//! hop into that same space even from a safe-looking public URL, and an
//! unbounded body bufferable in process memory.
//!
//! [`fetch_guarded`] closes all three: every redirect hop's hostname is
//! resolved and every resolved address must be globally routable,
//! non-http(s) redirect schemes are refused, and the body is read under
//! a hard byte cap. Policy violations raise
//! [`WebmentionError::InvalidValue`] (terminal rejection, mirroring the
//! parser's own validation errors); DNS failure and socket errors raise
//! [`WebmentionError::Http`] so callers can treat them as transient.
//!
//! Private-network deployments that legitimately mention internal hosts
//! can disable the guard by passing `ssrf_protection=false` on the
//! handler/processors — [`plain_fetch`] is the unguarded equivalent.

use std::collections::HashMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use ureq::ResponseExt;

use crate::error::{Result, WebmentionError};

/// Maximum redirect hops followed for a request — mirrors
/// `_MAX_REDIRECTS`.
pub const MAX_REDIRECTS: u32 = 5;

/// A fetched HTTP response — mirrors the bits of `requests.Response`
/// the callers use (`status_code`, `headers`, `text`, `url`,
/// `is_redirect`, `iter_content`).
#[derive(Debug, Clone)]
pub struct FetchResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers (names lowercased).
    pub headers: HashMap<String, String>,
    /// Body bytes (already read under the byte cap).
    pub body: Vec<u8>,
    /// The final URL after redirects.
    pub url: String,
}

impl FetchResponse {
    /// Whether the status is in the 2xx range.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Whether the response is a redirect (3xx + `Location` header) —
    /// mirrors `resp.is_redirect or resp.is_permanent_redirect`.
    pub fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status) && self.header("location").is_some()
    }

    /// Case-insensitive header lookup — mirrors `resp.headers.get`.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(String::as_str)
    }

    /// The body decoded as text — mirrors `resp.text`.
    ///
    /// `requests` falls back to `ISO-8859-1` for `text/*` responses
    /// without an explicit charset; the same is done here so latin-1
    /// pages don't get mangled. Otherwise UTF-8 (with lossy
    /// replacement) is used.
    pub fn text(&self) -> String {
        let content_type = self.header("content-type").unwrap_or_default();
        let charset = content_type
            .split(';')
            .nth(1)
            .and_then(|p| p.trim().strip_prefix("charset="))
            .map(|c| c.trim_matches('"').to_lowercase());
        if charset.as_deref() == Some("iso-8859-1") || charset.as_deref() == Some("latin-1") {
            return self.body.iter().map(|&b| b as char).collect();
        }
        String::from_utf8_lossy(&self.body).to_string()
    }
}

/// A normalized fetch request handed to a [`Fetcher`].
#[derive(Debug, Clone)]
pub struct FetchRequest {
    /// The URL to fetch.
    pub url: String,
    /// Per-request timeout.
    pub timeout: Duration,
    /// The `User-Agent` header to send.
    pub user_agent: String,
    /// Maximum body bytes to read.
    pub max_bytes: u64,
    /// Form fields — switches the request to POST when present.
    pub data: Option<HashMap<String, String>>,
}

/// Injectable fetch backend used by the parser and the outgoing
/// processor — the seam the Python tests reach through monkeypatching
/// `requests.get`/`fetch_guarded`.
pub type Fetcher = Arc<dyn Fn(&FetchRequest) -> Result<FetchResponse> + Send + Sync>;

/// Return the default fetcher for the given `ssrf_protection` flag —
/// [`fetch_guarded`] when enabled, [`plain_fetch`] otherwise.
pub fn default_fetcher(ssrf_protection: bool) -> Fetcher {
    if ssrf_protection {
        Arc::new(|req| {
            fetch_guarded(
                &req.url,
                req.timeout,
                &req.user_agent,
                req.max_bytes,
                req.data.as_ref(),
                MAX_REDIRECTS,
            )
        })
    } else {
        Arc::new(|req| {
            plain_fetch(
                &req.url,
                req.timeout,
                &req.user_agent,
                req.max_bytes,
                req.data.as_ref(),
            )
        })
    }
}

fn ip_is_public(ip: &IpAddr) -> bool {
    // Mirrors Python's `ip.is_global and not ip.is_multicast` over the
    // IANA special-purpose registries (Rust's `IpAddr::is_global` is
    // still unstable).
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0 // "this network"
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64.0.0/10
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
                || (o[0] == 192 && o[1] == 88 && o[2] == 99) // 6to4 relay
                || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15 bench
                || o[0] >= 224) // multicast + reserved
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return ip_is_public(&IpAddr::V4(mapped));
            }
            let seg = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || seg[0] == 0x0100 // 100::/64 discard
                || (seg[0] & 0xfe00) == 0xfc00 // fc00::/7 unique-local
                || (seg[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || (seg[0] == 0x2001 && seg[1] == 0x0db8)) // documentation
        }
    }
}

/// Resolve `host` to its IP addresses — mirrors `_resolve_ips`.
///
/// DNS failure raises [`WebmentionError::Http`] (transient, worth a
/// retry — the Python equivalent raises `requests.ConnectionError`).
fn resolve_ips(host: &str) -> Result<Vec<IpAddr>> {
    // A literal IP is its own resolution — this also keeps tests using
    // unreachable hostnames free to stub the resolver away.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }

    let portless = host.trim_matches(|c| c == '[' || c == ']');
    if let Ok(ip) = portless.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }

    let addrs: Vec<IpAddr> = (portless, 443)
        .to_socket_addrs()
        .map_err(|e| WebmentionError::Http(format!("DNS resolution failed for {host}: {e}")))?
        .map(|a| a.ip())
        .collect();

    if addrs.is_empty() {
        return Err(WebmentionError::Http(format!(
            "DNS resolution returned no addresses for {host}"
        )));
    }
    Ok(addrs)
}

/// Validate that `url`'s host resolves only to public addresses —
/// mirrors `_check_url_host`.
pub fn check_url_host(url: &str) -> Result<String> {
    let parsed = url::Url::parse(url)
        .map_err(|e| WebmentionError::InvalidValue(format!("invalid URL: {e}")))?;
    let scheme = parsed.scheme().to_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(WebmentionError::InvalidValue(format!(
            "non-http(s) scheme: {scheme}"
        )));
    }
    let host = parsed
        .host_str()
        .map(|h| h.to_lowercase())
        .filter(|h| !h.is_empty())
        .ok_or_else(|| WebmentionError::InvalidValue("URL has no host".to_string()))?;

    let ips = resolve_ips(&host)?;
    if !ips.iter().all(ip_is_public) {
        return Err(WebmentionError::InvalidValue(format!(
            "host {host} resolves to a non-public address"
        )));
    }
    Ok(host)
}

fn build_agent(timeout: Duration, max_redirects: u32) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(max_redirects)
        .build()
        .into()
}

fn response_parts(
    resp: &ureq::http::Response<ureq::Body>,
    url: &str,
) -> (u16, HashMap<String, String>, String) {
    let status = resp.status().as_u16();
    let headers: HashMap<String, String> = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|s| (k.as_str().to_lowercase(), s.to_string()))
        })
        .collect();
    (status, headers, url.to_string())
}

fn empty_response(resp: ureq::http::Response<ureq::Body>, url: &str) -> FetchResponse {
    let (status, headers, url) = response_parts(&resp, url);
    FetchResponse {
        status,
        headers,
        body: Vec::new(),
        url,
    }
}

fn read_response(
    resp: ureq::http::Response<ureq::Body>,
    url: &str,
    max_bytes: u64,
) -> Result<FetchResponse> {
    let (status, headers, url) = response_parts(&resp, url);
    let mut resp = resp;
    let body = resp
        .body_mut()
        .with_config()
        .limit(max_bytes)
        .read_to_vec()
        .map_err(|e| {
            WebmentionError::InvalidValue(format!("response body exceeds {max_bytes} bytes ({e})"))
        })?;
    Ok(FetchResponse {
        status,
        headers,
        body,
        url,
    })
}

fn send(
    agent: &ureq::Agent,
    url: &str,
    user_agent: &str,
    data: Option<&HashMap<String, String>>,
) -> std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    if let Some(form) = data {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form.iter())
            .finish();
        agent
            .post(url)
            .header("User-Agent", user_agent)
            .content_type("application/x-www-form-urlencoded")
            .send(body)
    } else {
        agent.get(url).header("User-Agent", user_agent).call()
    }
}

/// Fetch `url` with per-hop address validation and a body byte cap —
/// mirrors `fetch_guarded`.
///
/// Redirects are followed manually so each hop is re-validated: a
/// public source cannot 302 into private address space, and a redirect
/// to a non-http(s) scheme is refused. `data` switches to POST;
/// 301/302/303 hops drop the body to GET per redirect semantics.
pub fn fetch_guarded(
    url: &str,
    timeout: Duration,
    user_agent: &str,
    max_bytes: u64,
    data: Option<&HashMap<String, String>>,
    max_redirects: u32,
) -> Result<FetchResponse> {
    let agent = build_agent(timeout, 0);
    let mut current = url.to_string();
    let mut body_data = data.cloned();

    for _ in 0..=max_redirects {
        check_url_host(&current)?;
        let resp = send(&agent, &current, user_agent, body_data.as_ref())?;
        let status = resp.status().as_u16();

        if (300..400).contains(&status) {
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let Some(loc) = location else {
                return Ok(empty_response(resp, &current));
            };
            let base = url::Url::parse(&current)
                .map_err(|e| WebmentionError::InvalidValue(format!("invalid URL: {e}")))?;
            let next = base
                .join(&loc)
                .map_err(|e| WebmentionError::InvalidValue(format!("invalid redirect URL: {e}")))?
                .to_string();
            let next_scheme = url::Url::parse(&next)
                .map(|u| u.scheme().to_lowercase())
                .unwrap_or_default();
            if next_scheme != "http" && next_scheme != "https" {
                return Err(WebmentionError::InvalidValue(format!(
                    "redirect to non-http(s) scheme: {next_scheme}"
                )));
            }
            if matches!(status, 301..=303) {
                body_data = None;
            }
            current = next;
            continue;
        }

        // Non-redirect: only success bodies are ever consumed by
        // callers (mirrors `_read_capped` being applied to 2xx/3xx).
        if (200..400).contains(&status) {
            return read_response(resp, &current, max_bytes);
        }
        return Ok(empty_response(resp, &current));
    }

    Err(WebmentionError::InvalidValue(format!(
        "too many redirects fetching {url}"
    )))
}

/// Unguarded fetch — mirrors the `requests.get`/`requests.post` calls
/// used when `ssrf_protection=False`.
///
/// Redirects are followed by the HTTP client itself and the body is
/// read up to `max_bytes` (the Python code paths apply their own
/// discovery caps when they care).
pub fn plain_fetch(
    url: &str,
    timeout: Duration,
    user_agent: &str,
    max_bytes: u64,
    data: Option<&HashMap<String, String>>,
) -> Result<FetchResponse> {
    let agent = build_agent(timeout, 10);
    let resp = send(&agent, url, user_agent, data)?;
    let final_url = resp.get_uri().to_string();
    read_response(resp, &final_url, max_bytes)
}
