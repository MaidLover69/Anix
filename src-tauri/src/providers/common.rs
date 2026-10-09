//! Small helpers shared by everything that deals with streaming sources.

use once_cell::sync::Lazy;
use reqwest::header::HeaderMap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Formats an episode number without losing a fractional part: 12 -> "12", 12.5 -> "12.5".
pub fn fmt_ep(number: f64) -> String {
    if number.fract().abs() < 1e-9 {
        format!("{}", number.round() as i64)
    } else {
        let text = format!("{:.3}", number);
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Reads a JSON value that sites serialise either as a string or as a number.
pub fn json_id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Reads a JSON number that may be serialised as a string ("12.5").
pub fn json_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Once a site refused the plain client, a lookup that asks the in-app browser anyway does not
/// try the plain client again for this long.
const PLAIN_BLOCK_MEMORY: Duration = Duration::from_secs(10 * 60);

static PLAIN_BLOCKED: Lazy<Mutex<HashMap<&'static str, Instant>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn plain_blocked_recently(site: &'static str) -> bool {
    let guard = PLAIN_BLOCKED.lock().unwrap_or_else(|p| p.into_inner());
    guard.get(site).map_or(false, |at| at.elapsed() < PLAIN_BLOCK_MEMORY)
}

pub fn note_plain_blocked(site: &'static str) {
    let mut guard = PLAIN_BLOCKED.lock().unwrap_or_else(|p| p.into_inner());
    guard.insert(site, Instant::now());
}

/// Request headers a page may set on its own same-origin request. The browser supplies the
/// rest (Referer, Origin, cookies, user agent, client hints) itself.
pub fn session_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    const BROWSER_OWNED: [&str; 9] = [
        "referer",
        "origin",
        "cookie",
        "host",
        "user-agent",
        "accept-encoding",
        "connection",
        "content-length",
        "dnt",
    ];
    headers
        .iter()
        .filter(|(name, _)| {
            let name = name.as_str();
            !BROWSER_OWNED.contains(&name) && !name.starts_with("sec-") && !name.starts_with("proxy-")
        })
        .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.to_string())))
        .collect()
}

/// Path and query of a URL, the part a same-origin request names.
pub fn path_and_query(url: &reqwest::Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{}", url.path(), query),
        None => url.path().to_string(),
    }
}

pub fn is_hls_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.to_ascii_lowercase().contains(".m3u8")
}

/// Normalises URLs that sites emit without a scheme or with a doubled slash after it.
pub fn fix_url(url: &str) -> String {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("https:///") {
        format!("https://{}", rest)
    } else if url.starts_with("//") {
        format!("https:{}", url)
    } else {
        url.to_string()
    }
}

pub fn subtitle_format(url: &str, declared: Option<&str>) -> String {
    let from_url = url
        .split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('.').next())
        .map(|ext| ext.to_ascii_lowercase());
    declared
        .map(|d| d.trim().to_ascii_lowercase())
        .into_iter()
        .chain(from_url)
        .find(|ext| matches!(ext.as_str(), "vtt" | "srt" | "ass" | "ssa"))
        .unwrap_or_else(|| "vtt".to_string())
}
