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
/// rest (Referer, Origin, cookies, user agent, client hints) itself and refuses to have them set.
pub fn session_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    const BROWSER_OWNED: [&str; 19] = [
        "accept-charset",
        "accept-encoding",
        "connection",
        "content-length",
        "cookie",
        "cookie2",
        "date",
        "dnt",
        "expect",
        "host",
        "keep-alive",
        "origin",
        "referer",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "user-agent",
        "via",
    ];
    headers
        .iter()
        .filter(|(name, _)| {
            let name = name.as_str();
            !BROWSER_OWNED.contains(&name)
                && !name.starts_with("sec-")
                && !name.starts_with("proxy-")
                && !name.starts_with("access-control-")
        })
        .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.to_string())))
        .collect()
}

/// The path a page can name as its request's referrer for a plugin's `Referer` header: only an
/// address on the same origin as the request qualifies.
pub fn session_referrer(headers: &HeaderMap, request_url: &reqwest::Url) -> Option<String> {
    let referer = headers.get(reqwest::header::REFERER)?.to_str().ok()?;
    let referer = reqwest::Url::parse(referer).ok()?;
    (referer.origin() == request_url.origin()).then(|| path_and_query(&referer))
}

/// The text of a body in the character set its `Content-Type` declares (UTF-8 when it declares
/// none or one that is unknown). A byte order mark decides over the declaration.
pub fn decode_text(content_type: Option<&str>, body: &[u8]) -> String {
    let encoding = declared_charset(content_type).unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(body);
    text.into_owned()
}

/// The character set named by a `Content-Type` value, when it names one that is known.
pub fn declared_charset(content_type: Option<&str>) -> Option<&'static encoding_rs::Encoding> {
    content_type?
        .split(';')
        .skip(1)
        .filter_map(|part| part.split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("charset"))
        .and_then(|(_, label)| encoding_rs::Encoding::for_label(label.trim().trim_matches(['"', '\'']).as_bytes()))
}

/// How much longer one plugin call may take when its requests wait for an in-app browser
/// session (a window being created, the site loading in it).
pub const SESSION_CALL_ALLOWANCE: Duration = Duration::from_secs(30);
/// The same for a provider's whole attempt at a play request, which may start the sessions of
/// two origins one after the other (a site's API and its player page).
pub const SESSION_LOOKUP_ALLOWANCE: Duration = Duration::from_secs(60);

/// Runs `work` under a time limit of `base`, plus the time `waited` says was spent waiting for
/// an in-app browser session since the start, up to `allowance`. `None` when the limit ran out.
pub async fn with_session_allowance<F: std::future::Future>(
    work: F,
    base: Duration,
    allowance: Duration,
    waited: impl Fn(Instant) -> Duration,
) -> Option<F::Output> {
    tokio::pin!(work);
    let started = Instant::now();
    loop {
        let limit = base + waited(started).min(allowance);
        let left = limit.saturating_sub(started.elapsed());
        if left.is_zero() {
            return None;
        }
        tokio::select! {
            output = &mut work => return Some(output),
            _ = tokio::time::sleep(left.min(Duration::from_millis(200))) => {}
        }
    }
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
