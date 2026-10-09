//! Loopback proxy for media requests.
//!
//! The webview cannot set `Referer`/`Origin` on its own requests, and its page origin differs
//! between `tauri dev` (`http://localhost:1420`) and a packaged build (`tauri://localhost`),
//! so stream CDNs see different requests in the two cases. The player therefore loads streams
//! and subtitles from this proxy, which fetches them with the headers the provider expects,
//! rewrites HLS playlists so nested requests come back here too, and answers with CORS headers.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use once_cell::sync::{Lazy, OnceCell};
use regex::Regex;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use url::Url;

const WORKER_THREADS: usize = 12;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_PLAYLIST_BYTES: usize = 8 * 1024 * 1024;
const MAX_SUBTITLE_BYTES: usize = 16 * 1024 * 1024;

static BASE_URL: OnceCell<String> = OnceCell::new();

static URI_ATTRIBUTE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"URI="([^"]*)""#).expect("Failed to compile URI attribute regex"));

static SRT_TIMESTAMP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\d{1,2}:\d{2}:\d{2}),(\d{3})").expect("Failed to compile SRT timestamp regex")
});

static ASS_OVERRIDE_TAG: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{[^}]*\}").expect("Failed to compile ASS override regex"));

/// Headers the proxy sets itself or that must not be forwarded from a stream's header map.
const BLOCKED_REQUEST_HEADERS: [&str; 8] = [
    "host",
    "connection",
    "content-length",
    "transfer-encoding",
    "accept-encoding",
    "range",
    "upgrade",
    "te",
];

/// What the proxy needs to fetch one upstream resource the way the provider expects.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpstreamParams {
    pub referer: Option<String>,
    pub origin: Option<String>,
    /// The stream's custom headers, kept in their encoded form so nested URLs can carry them.
    pub headers_b64: Option<String>,
}

impl UpstreamParams {
    fn from_query(query: &HashMap<String, String>) -> Self {
        Self {
            referer: query.get("referer").cloned().filter(|v| !v.is_empty()),
            origin: query.get("origin").cloned().filter(|v| !v.is_empty()),
            headers_b64: query.get("h").cloned().filter(|v| !v.is_empty()),
        }
    }

    /// Decodes the custom header map; malformed input yields no headers.
    pub fn custom_headers(&self) -> HashMap<String, String> {
        self.headers_b64
            .as_deref()
            .and_then(|encoded| URL_SAFE_NO_PAD.decode(encoded.trim_end_matches('=')).ok())
            .and_then(|bytes| serde_json::from_slice::<HashMap<String, String>>(&bytes).ok())
            .unwrap_or_default()
    }

    /// The request headers to send upstream, one value per header name.
    pub fn request_headers(&self) -> Vec<(String, String)> {
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut set = |name: &str, value: String| {
            if let Some(existing) = headers.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                existing.1 = value;
            } else {
                headers.push((name.to_string(), value));
            }
        };

        if let Some(ref referer) = self.referer {
            set("Referer", referer.clone());
        }
        let origin = self.origin.clone().or_else(|| {
            self.referer
                .as_deref()
                .and_then(|r| Url::parse(r).ok())
                .map(|u| u.origin().ascii_serialization())
        });
        if let Some(origin) = origin {
            set("Origin", origin);
        }
        for (name, value) in self.custom_headers() {
            let is_blocked = BLOCKED_REQUEST_HEADERS
                .iter()
                .any(|blocked| name.eq_ignore_ascii_case(blocked));
            let is_clean = !name.is_empty()
                && name.bytes().all(|b| b.is_ascii_graphic() && b != b':')
                && !value.chars().any(|c| c.is_control());
            if !is_blocked && is_clean {
                set(&name, value);
            }
        }
        headers
    }
}

/// Only absolute http(s) URLs may be fetched through the proxy.
pub fn parse_target_url(raw: &str) -> Result<Url, String> {
    let parsed = Url::parse(raw.trim()).map_err(|e| format!("Invalid URL: {}", e))?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(parsed),
        scheme => Err(format!("Unsupported URL scheme: {}", scheme)),
    }
}

/// The proxy URL for `target`, carrying the same upstream parameters.
pub fn proxied_url(proxy_base: &str, target: &Url, params: &UpstreamParams) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("url", target.as_str());
    if let Some(ref referer) = params.referer {
        query.append_pair("referer", referer);
    }
    if let Some(ref origin) = params.origin {
        query.append_pair("origin", origin);
    }
    if let Some(ref headers) = params.headers_b64 {
        query.append_pair("h", headers);
    }
    format!("{}/proxy?{}", proxy_base, query.finish())
}

/// Rewrites every URI in an HLS playlist (variant/segment lines and `URI="..."` attributes of
/// keys, init segments, alternate renditions and I-frame playlists) to go through the proxy.
pub fn rewrite_playlist(
    text: &str,
    playlist_url: &Url,
    proxy_base: &str,
    params: &UpstreamParams,
) -> String {
    let rewrite = |reference: &str| -> Option<String> {
        let reference = reference.trim();
        if reference.is_empty() || reference.starts_with("data:") {
            return None;
        }
        let absolute = playlist_url.join(reference).ok()?;
        match absolute.scheme() {
            "http" | "https" => Some(proxied_url(proxy_base, &absolute, params)),
            _ => None,
        }
    };

    let mut out = String::with_capacity(text.len() * 2);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            out.push('\n');
        } else if trimmed.starts_with('#') {
            let replaced = URI_ATTRIBUTE.replace_all(trimmed, |caps: &regex::Captures| {
                match rewrite(&caps[1]) {
                    Some(url) => format!("URI=\"{}\"", url),
                    None => caps[0].to_string(),
                }
            });
            out.push_str(&replaced);
            out.push('\n');
        } else {
            out.push_str(&rewrite(trimmed).unwrap_or_else(|| trimmed.to_string()));
            out.push('\n');
        }
    }
    out
}

fn looks_like_playlist(url: &Url, content_type: &str) -> bool {
    let content_type = content_type.to_ascii_lowercase();
    content_type.contains("mpegurl") || url.path().to_ascii_lowercase().ends_with(".m3u8")
}

/// True when a body begins with the HLS playlist tag (after an optional BOM and whitespace).
pub fn starts_with_playlist_tag(body: &[u8]) -> bool {
    let body = body.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(body);
    let start = body.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(body.len());
    body[start..].starts_with(b"#EXTM3U")
}

/// SubRip to WebVTT: a header plus `.` instead of `,` in the millisecond separator.
pub fn srt_to_vtt(srt: &str) -> String {
    let body = SRT_TIMESTAMP.replace_all(srt.trim_start_matches('\u{feff}'), "$1.$2");
    format!("WEBVTT\n\n{}\n", body.replace("\r\n", "\n").trim())
}

fn ass_time_to_vtt(time: &str) -> Option<String> {
    // ASS: H:MM:SS.cc
    let mut parts = time.trim().split(':');
    let hours: u32 = parts.next()?.parse().ok()?;
    let minutes: u32 = parts.next()?.parse().ok()?;
    let (seconds, centis) = parts.next()?.split_once('.').unwrap_or(("0", "0"));
    let seconds: u32 = seconds.parse().ok()?;
    let centis: u32 = centis.get(..2).unwrap_or(centis).parse().ok()?;
    Some(format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, centis * 10))
}

/// Advanced SubStation Alpha to WebVTT: dialogue timing and text only, styling dropped.
pub fn ass_to_vtt(ass: &str) -> String {
    let mut start_idx = 1;
    let mut end_idx = 2;
    let mut text_idx = 9;
    let mut cues: Vec<(String, String, String)> = Vec::new();

    for line in ass.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if let Some(format) = line.strip_prefix("Format:") {
            let fields: Vec<String> = format.split(',').map(|f| f.trim().to_lowercase()).collect();
            if let (Some(s), Some(e), Some(t)) = (
                fields.iter().position(|f| f == "start"),
                fields.iter().position(|f| f == "end"),
                fields.iter().position(|f| f == "text"),
            ) {
                start_idx = s;
                end_idx = e;
                text_idx = t;
            }
        } else if let Some(dialogue) = line.strip_prefix("Dialogue:") {
            let fields: Vec<&str> = dialogue.splitn(text_idx + 1, ',').collect();
            if fields.len() <= text_idx {
                continue;
            }
            let (Some(start), Some(end)) = (
                ass_time_to_vtt(fields[start_idx]),
                ass_time_to_vtt(fields[end_idx]),
            ) else {
                continue;
            };
            let text = ASS_OVERRIDE_TAG
                .replace_all(fields[text_idx], "")
                .replace("\\N", "\n")
                .replace("\\n", "\n")
                .replace("\\h", " ");
            let text = text.trim();
            if !text.is_empty() {
                cues.push((start, end, text.to_string()));
            }
        }
    }

    cues.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::from("WEBVTT\n\n");
    for (start, end, text) in cues {
        out.push_str(&format!("{} --> {}\n{}\n\n", start, end, text));
    }
    out
}

/// Converts a fetched subtitle body to WebVTT, whatever format the provider served.
pub fn subtitle_to_vtt(body: &str) -> String {
    let trimmed = body.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with("WEBVTT") {
        trimmed.to_string()
    } else if trimmed.contains("[Script Info]") || trimmed.contains("\nDialogue:") {
        ass_to_vtt(trimmed)
    } else {
        srt_to_vtt(trimmed)
    }
}

fn random_token() -> String {
    // RandomState is seeded from the OS; two independent states give 128 unpredictable bits.
    let part = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(std::process::id() as u128);
        hasher.finish()
    };
    format!("{:016x}{:016x}", part(), part())
}

fn header(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

fn cors_headers() -> Vec<Header> {
    [
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"),
        ("Access-Control-Allow-Headers", "Range, Content-Type"),
        (
            "Access-Control-Expose-Headers",
            "Content-Length, Content-Range, Accept-Ranges, Content-Type",
        ),
        ("Access-Control-Max-Age", "86400"),
    ]
    .iter()
    .filter_map(|(name, value)| header(name, value))
    .collect()
}

fn respond_text(request: Request, status: u16, message: &str) {
    let mut headers = cors_headers();
    headers.extend(header("Content-Type", "text/plain; charset=utf-8"));
    let body = message.as_bytes().to_vec();
    let length = body.len();
    let response = Response::new(
        StatusCode(status),
        headers,
        std::io::Cursor::new(body),
        Some(length),
        None,
    );
    let _ = request.respond(response);
}

fn respond_bytes(request: Request, status: u16, content_type: &str, body: Vec<u8>) {
    let mut headers = cors_headers();
    headers.extend(header("Content-Type", content_type));
    headers.extend(header("Cache-Control", "no-store"));
    let length = body.len();
    let response = Response::new(
        StatusCode(status),
        headers,
        std::io::Cursor::new(body),
        Some(length),
        None,
    );
    let _ = request.respond(response);
}

/// Streams an upstream body to the webview without buffering the whole file.
struct BodyReader {
    response: reqwest::Response,
    buffer: Vec<u8>,
    position: usize,
}

impl Read for BodyReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.position >= self.buffer.len() {
            match tauri::async_runtime::block_on(self.response.chunk()) {
                Ok(Some(chunk)) => {
                    self.buffer = chunk.to_vec();
                    self.position = 0;
                }
                Ok(None) => return Ok(0),
                Err(e) => return Err(std::io::Error::other(e)),
            }
        }
        let available = &self.buffer[self.position..];
        let count = available.len().min(out.len());
        out[..count].copy_from_slice(&available[..count]);
        self.position += count;
        Ok(count)
    }
}

async fn read_limited(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > limit {
            return Err("Upstream response is too large".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn fetch_upstream(
    target: &Url,
    params: &UpstreamParams,
    range: Option<&str>,
    head: bool,
) -> Result<reqwest::Response, String> {
    let client = crate::clients::http::download_client();
    let mut request = if head {
        client.head(target.clone())
    } else {
        client.get(target.clone())
    };
    request = request.header("Accept", "*/*");
    for (name, value) in params.request_headers() {
        request = request.header(name, value);
    }
    if let Some(range) = range {
        request = request.header("Range", range);
    }
    match tokio::time::timeout(CONNECT_TIMEOUT, request.send()).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(e)) => Err(format!("Upstream request failed: {}", e)),
        Err(_) => Err("Upstream request timed out".to_string()),
    }
}

fn request_header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

fn handle_proxy(request: Request, target: Url, params: UpstreamParams, proxy_base: &str) {
    let head = *request.method() == Method::Head;
    let range = request_header(&request, "Range");

    let upstream = tauri::async_runtime::block_on(fetch_upstream(
        &target,
        &params,
        range.as_deref(),
        head,
    ));
    let response = match upstream {
        Ok(response) => response,
        Err(message) => {
            tracing::warn!("stream proxy: {} ({})", message, target);
            return respond_text(request, 502, &message);
        }
    };

    let status = response.status().as_u16();
    let final_url = response.url().clone();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    if !response.status().is_success() {
        tracing::warn!("stream proxy: upstream answered {} for {}", status, target);
        return respond_text(request, status, &format!("Upstream answered HTTP {}", status));
    }

    if !head && looks_like_playlist(&final_url, &content_type) {
        let body = tauri::async_runtime::block_on(read_limited(response, MAX_PLAYLIST_BYTES));
        return match body {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                let rewritten = rewrite_playlist(&text, &final_url, proxy_base, &params);
                respond_bytes(
                    request,
                    200,
                    "application/vnd.apple.mpegurl",
                    rewritten.into_bytes(),
                )
            }
            Err(message) => respond_text(request, 502, &message),
        };
    }

    let mut headers = cors_headers();
    headers.extend(header("Content-Type", &content_type));
    for name in ["Content-Range", "Accept-Ranges", "Last-Modified", "ETag"] {
        if let Some(value) = response.headers().get(name).and_then(|v| v.to_str().ok()) {
            headers.extend(header(name, value));
        }
    }
    let length = response.content_length().map(|l| l as usize);

    if head {
        let _ = request.respond(Response::new(
            StatusCode(status),
            headers,
            std::io::empty(),
            length,
            None,
        ));
        return;
    }

    // Some providers serve playlists without the usual extension or content type.
    let mut response = response;
    let first_chunk = match tauri::async_runtime::block_on(response.chunk()) {
        Ok(chunk) => chunk.map(|c| c.to_vec()).unwrap_or_default(),
        Err(e) => return respond_text(request, 502, &format!("Upstream read failed: {}", e)),
    };
    if status == 200 && starts_with_playlist_tag(&first_chunk) {
        let rest = tauri::async_runtime::block_on(read_limited(response, MAX_PLAYLIST_BYTES));
        return match rest {
            Ok(rest) => {
                let mut bytes = first_chunk;
                bytes.extend_from_slice(&rest);
                let text = String::from_utf8_lossy(&bytes);
                let rewritten = rewrite_playlist(&text, &final_url, proxy_base, &params);
                respond_bytes(
                    request,
                    200,
                    "application/vnd.apple.mpegurl",
                    rewritten.into_bytes(),
                )
            }
            Err(message) => respond_text(request, 502, &message),
        };
    }

    let reader = BodyReader {
        response,
        buffer: first_chunk,
        position: 0,
    };
    // A dropped connection (seek, episode change) ends the copy; that is not an error.
    let _ = request.respond(Response::new(StatusCode(status), headers, reader, length, None));
}

fn handle_subtitle(request: Request, target: Url, params: UpstreamParams) {
    let result = tauri::async_runtime::block_on(async {
        let response = fetch_upstream(&target, &params, None, false).await?;
        if !response.status().is_success() {
            return Err(format!("Upstream answered HTTP {}", response.status().as_u16()));
        }
        read_limited(response, MAX_SUBTITLE_BYTES).await
    });
    match result {
        Ok(bytes) => {
            let vtt = subtitle_to_vtt(&String::from_utf8_lossy(&bytes));
            respond_bytes(request, 200, "text/vtt; charset=utf-8", vtt.into_bytes())
        }
        Err(message) => {
            tracing::warn!("stream proxy: subtitle {} ({})", message, target);
            respond_text(request, 502, &message)
        }
    }
}

fn handle_request(request: Request, token: &str, proxy_base: &str) {
    if *request.method() == Method::Options {
        let _ = request.respond(Response::new(
            StatusCode(204),
            cors_headers(),
            std::io::empty(),
            Some(0),
            None,
        ));
        return;
    }
    if !matches!(request.method(), Method::Get | Method::Head) {
        return respond_text(request, 405, "Method not allowed");
    }

    let Ok(parsed) = Url::parse(&format!("http://127.0.0.1{}", request.url())) else {
        return respond_text(request, 400, "Bad request");
    };
    let mut segments = parsed.path().trim_start_matches('/').splitn(2, '/');
    let (request_token, endpoint) = (segments.next().unwrap_or(""), segments.next().unwrap_or(""));
    // The token keeps other local processes and web pages from using this as an open proxy.
    if request_token != token {
        return respond_text(request, 403, "Forbidden");
    }

    let query: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
    let target = match query.get("url").map(|raw| parse_target_url(raw)) {
        Some(Ok(target)) => target,
        Some(Err(message)) => return respond_text(request, 400, &message),
        None => return respond_text(request, 400, "Missing url parameter"),
    };
    let params = UpstreamParams::from_query(&query);

    match endpoint {
        "proxy" => handle_proxy(request, target, params, proxy_base),
        "subtitle" => handle_subtitle(request, target, params),
        _ => respond_text(request, 404, "Not found"),
    }
}

/// Starts the proxy on a random loopback port. Safe to call more than once.
pub fn start() -> Result<String, String> {
    BASE_URL
        .get_or_try_init(|| {
            let server = Server::http("127.0.0.1:0")
                .map_err(|e| format!("Failed to start stream proxy: {}", e))?;
            let port = server
                .server_addr()
                .to_ip()
                .map(|addr| addr.port())
                .ok_or_else(|| "Stream proxy has no TCP address".to_string())?;
            let token = random_token();
            let base = format!("http://127.0.0.1:{}/{}", port, token);

            let server = Arc::new(server);
            for index in 0..WORKER_THREADS {
                let server = server.clone();
                let token = token.clone();
                let base = base.clone();
                std::thread::Builder::new()
                    .name(format!("stream-proxy-{}", index))
                    .spawn(move || {
                        while let Ok(request) = server.recv() {
                            handle_request(request, &token, &base);
                        }
                    })
                    .map_err(|e| format!("Failed to start stream proxy worker: {}", e))?;
            }

            tracing::info!("stream proxy listening on 127.0.0.1:{}", port);
            Ok(base)
        })
        .cloned()
}

#[tauri::command]
pub async fn get_stream_proxy_base() -> Result<String, String> {
    start()
}
