//! The network a plugin sees: `fetch` answered by the app's shared HTTP clients (so the
//! DNS-over-HTTPS setting applies), with limits, and with the in-app browser session as the
//! second attempt when a site answers with its anti-bot page.

use crate::clients::http::{http_client, no_redirect_client};
use crate::error::AppError;
use crate::providers::common::{
    declared_charset, note_plain_blocked, path_and_query, plain_blocked_recently, session_headers,
    session_referrer,
};
use crate::web_session::{
    self, detect_barrier, SessionChoice, SessionState, SessionUse, SiteSession, SiteSpec, WebRequest,
    MAX_REQUEST_BODY_BYTES, SESSION_METHODS,
};
use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, LOCATION};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Limit for one request when the plugin names none.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
/// The longest limit a plugin may ask for.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest response body handed to a plugin.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// Largest request body a plugin may send.
pub const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
/// Limit for a request through a session that already answers.
const SESSION_REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
/// Limit for a request through a session that has to be started first: its window is created,
/// the site loaded and, when the site runs a check of its own on a fresh page, that waited for.
pub const SESSION_START_TIMEOUT: Duration = Duration::from_secs(25);
/// Bodies up to this size are also decoded for `text()` when they declare a character set
/// other than UTF-8.
const MAX_DECODED_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_NOTES: usize = 64;
const MAX_WAITS: usize = 64;
/// A connection the network resets is tried this many more times, after these pauses.
pub const RESET_RETRY_PAUSES: [Duration; 3] =
    [Duration::from_millis(400), Duration::from_millis(900), Duration::from_millis(1800)];

/// An error with every cause it wraps, for telling what happened underneath.
fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut current = error.source();
    while let Some(cause) = current {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        current = cause.source();
    }
    text
}

/// True for a failed request the network cut off (a reset or a connection closed before the
/// answer), which is worth repeating after a pause, unlike a refusal or a timeout.
pub fn is_connection_reset(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    ["connection reset", "reset by peer", "os error 104", "connection closed before message completed", "broken pipe"]
        .iter()
        .any(|sign| lower.contains(sign))
}

/// Request headers the HTTP client sets itself.
const CLIENT_OWNED_HEADERS: [&str; 6] = [
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "accept-encoding",
    "upgrade",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireRequest {
    pub url: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub redirect: String,
    #[serde(default)]
    pub has_body: bool,
    #[serde(default)]
    pub timeout_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireResponseMeta {
    pub status: u16,
    pub status_text: String,
    pub url: String,
    pub redirected: bool,
    pub headers: Vec<(String, String)>,
    /// The body as text, when it declares a character set that is not UTF-8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WireResponse {
    pub meta: WireResponseMeta,
    pub body: Vec<u8>,
}

/// What a plugin's requests ran into, kept so a failed call can be reported for what it was.
#[derive(Debug, Clone, PartialEq)]
pub enum NetNote {
    /// A site answered with its anti-bot page and the in-app browser was not asked.
    Blocked,
    /// The in-app browser was refused as well; the user can verify for this site.
    NeedsVerification { site: String, origin: String },
    /// A site answered with its anti-bot page and the in-app browser did not get an answer
    /// either, for this reason.
    SessionFailed(String),
    Unreachable(String),
    Answered,
}

/// Per-plugin network state shared by its requests.
pub struct NetContext {
    plugin_id: String,
    session: Arc<dyn SiteSession>,
    /// Loopback and private addresses are refused unless this is set (tests, the dev runner).
    allow_private: bool,
    notes: Mutex<Vec<(Instant, NetNote)>>,
    /// When requests waited for an in-app browser session: start and, once over, end.
    waits: Mutex<Vec<(Instant, Option<Instant>)>>,
}

/// Marks the end of a wait for a session, also when the request is dropped.
struct WaitGuard<'a> {
    net: &'a NetContext,
    started: Instant,
}

impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        let mut waits = self.net.waits.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(wait) = waits.iter_mut().find(|(at, end)| *at == self.started && end.is_none()) {
            wait.1 = Some(Instant::now());
        }
    }
}

static SITE_PLUGINS: Lazy<Mutex<HashMap<String, BTreeSet<String>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Ids of the plugins whose requests met the anti-bot page of the site with this key.
pub fn plugins_for_site(site_key: &str) -> Vec<String> {
    let guard = SITE_PLUGINS.lock().unwrap_or_else(|p| p.into_inner());
    guard
        .get(site_key)
        .map(|ids| ids.iter().cloned().collect())
        .unwrap_or_default()
}

fn status_text(status: u16) -> String {
    match reqwest::StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()) {
        Some(reason) => format!("{} {}", status, reason),
        None => status.to_string(),
    }
}

fn is_private_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_private_address(IpAddr::V4(v4)),
            None => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        },
    }
}

fn private_target(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(ip)) => is_private_address(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_private_address(IpAddr::V6(ip)),
        None => true,
    }
}

impl NetContext {
    pub fn new(plugin_id: &str, session: Arc<dyn SiteSession>, allow_private: bool) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            session,
            allow_private,
            notes: Mutex::new(Vec::new()),
            waits: Mutex::new(Vec::new()),
        }
    }

    fn start_wait(&self) -> WaitGuard<'_> {
        let started = Instant::now();
        let mut waits = self.waits.lock().unwrap_or_else(|p| p.into_inner());
        if waits.len() >= MAX_WAITS {
            waits.retain(|(_, end)| end.is_none());
            let excess = (waits.len() + 1).saturating_sub(MAX_WAITS).min(waits.len());
            waits.drain(..excess);
        }
        waits.push((started, None));
        WaitGuard { net: self, started }
    }

    /// How long this plugin's requests have been waiting for an in-app browser session since
    /// `since`. Callers add it to their time limits, so that starting a session (creating its
    /// window and loading the site) does not use up the time the plugin itself is given.
    pub fn session_time_since(&self, since: Instant) -> Duration {
        let now = Instant::now();
        let waits = self.waits.lock().unwrap_or_else(|p| p.into_inner());
        let total: Duration = waits
            .iter()
            .map(|(start, end)| end.unwrap_or(now).saturating_duration_since((*start).max(since)))
            .sum();
        total.min(now.saturating_duration_since(since))
    }

    /// True while a request of this plugin waits for an in-app browser session.
    pub fn waiting_for_session(&self) -> bool {
        let waits = self.waits.lock().unwrap_or_else(|p| p.into_inner());
        waits.iter().any(|(_, end)| end.is_none())
    }

    fn note(&self, note: NetNote) {
        let mut guard = self.notes.lock().unwrap_or_else(|p| p.into_inner());
        if guard.len() >= MAX_NOTES {
            let excess = guard.len() + 1 - MAX_NOTES;
            guard.drain(..excess);
        }
        guard.push((Instant::now(), note));
    }

    /// The error a call that failed after `since` should be reported as, when its requests met
    /// something more telling than what the plugin said: a site waiting for verification, an
    /// anti-bot page, or nothing but failed connections.
    pub fn failure_since(&self, since: Instant) -> Option<AppError> {
        let guard = self.notes.lock().unwrap_or_else(|p| p.into_inner());
        let recent: Vec<&NetNote> = guard
            .iter()
            .filter(|(at, _)| *at >= since)
            .map(|(_, note)| note)
            .collect();
        for note in &recent {
            if let NetNote::NeedsVerification { site, origin } = note {
                return Some(AppError::VerificationRequired {
                    site: site.clone(),
                    origin: origin.clone(),
                });
            }
        }
        if let Some(NetNote::SessionFailed(reason)) =
            recent.iter().find(|note| matches!(note, NetNote::SessionFailed(_)))
        {
            return Some(AppError::Network(format!(
                "blocked by anti-bot check, and the in-app browser did not get through ({})",
                reason
            )));
        }
        if recent.iter().any(|note| matches!(note, NetNote::Blocked)) {
            return Some(AppError::CaptchaRequired);
        }
        let answered = recent.iter().any(|note| matches!(note, NetNote::Answered));
        if !answered {
            if let Some(NetNote::Unreachable(reason)) =
                recent.iter().find(|note| matches!(note, NetNote::Unreachable(_)))
            {
                return Some(AppError::Unreachable(reason.clone()));
            }
        }
        None
    }

    fn check_target(&self, url: &Url) -> Result<(), String> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(format!("only http and https URLs can be fetched, not {}:", url.scheme()));
        }
        if url.host_str().map_or(true, str::is_empty) {
            return Err("the URL has no host".to_string());
        }
        if !self.allow_private && private_target(url) {
            return Err(format!(
                "private network access denied: {}",
                url.host_str().unwrap_or_default()
            ));
        }
        Ok(())
    }

    fn request_headers(&self, request: &WireRequest) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in &request.headers {
            let lower = name.trim().to_ascii_lowercase();
            if lower.is_empty() || CLIENT_OWNED_HEADERS.contains(&lower.as_str()) {
                continue;
            }
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(lower.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(name, value);
            }
        }
        headers
    }

    async fn plain(
        &self,
        url: &Url,
        method: &Method,
        headers: &HeaderMap,
        body: Option<&[u8]>,
        redirect: &str,
        timeout: Duration,
    ) -> Result<WireResponse, String> {
        let client = if redirect == "follow" {
            http_client()
        } else {
            no_redirect_client()
        }
        .map_err(|e| e.reason())?;

        let mut builder = client
            .request(method.clone(), url.clone())
            .headers(headers.clone())
            .timeout(timeout);
        if let Some(body) = body {
            builder = builder.body(body.to_vec());
        }
        let started = Instant::now();
        let mut pauses = RESET_RETRY_PAUSES.iter();
        let mut response = loop {
            let Some(request) = builder.try_clone() else {
                break builder.send().await;
            };
            // Every try shares the one time limit.
            match request.timeout(timeout.saturating_sub(started.elapsed())).send().await {
                // A request that changes something is not sent twice.
                Err(e) if method.is_idempotent() && is_connection_reset(&error_chain(&e)) => match pauses.next() {
                    // Some networks reset about every other connection to a site; the next one
                    // usually gets through.
                    Some(pause) if started.elapsed() + *pause < timeout => {
                        crate::clients::http::note_unreachable_host(url.host_str().unwrap_or_default(), "connection reset");
                        tracing::info!("{}: connection reset, trying again", url.host_str().unwrap_or_default());
                        tokio::time::sleep(*pause).await;
                    }
                    _ => break Err(e),
                },
                other => break other,
            }
        }
        .map_err(|e| {
            let error = AppError::from(e);
            if error.is_connection_failure() {
                self.note(NetNote::Unreachable(error.reason()));
                crate::clients::http::note_unreachable_host(url.host_str().unwrap_or_default(), &error.reason());
            }
            error.reason()
        })?;

        if !self.allow_private {
            if let Some(remote) = response.remote_addr() {
                if is_private_address(remote.ip()) {
                    return Err(format!(
                        "private network access denied: {} resolves to a private address",
                        response.url().host_str().unwrap_or_default()
                    ));
                }
            }
        }

        let status = response.status();
        if redirect == "error" && status.is_redirection() {
            let target = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("another address");
            return Err(format!("redirect mode error: received redirect to {}", target));
        }

        let final_url = response.url().clone();
        let header_pairs: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();

        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let chunk = response.chunk().await.map_err(|e| AppError::from(e).reason())?;
            let Some(chunk) = chunk else { break };
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(format!(
                    "{} answered with more than {} MB",
                    final_url.host_str().unwrap_or("the site"),
                    MAX_RESPONSE_BYTES / (1024 * 1024)
                ));
            }
            bytes.extend_from_slice(&chunk);
        }

        Ok(with_text(WireResponse {
            meta: WireResponseMeta {
                status: status.as_u16(),
                status_text: status_text(status.as_u16()),
                redirected: final_url != *url,
                url: final_url.to_string(),
                headers: header_pairs,
                text: None,
            },
            body: bytes,
        }))
    }

    /// The request as a page of the site could make it, when it can: a same-origin request
    /// with a method and body a page may send, following redirects as a page does.
    fn session_request(
        &self,
        url: &Url,
        method: &Method,
        headers: &HeaderMap,
        body: Option<&[u8]>,
        redirect: &str,
    ) -> Option<(&'static SiteSpec, WebRequest)> {
        if redirect != "follow" || !SESSION_METHODS.contains(&method.as_str()) {
            return None;
        }
        if body.map_or(false, |body| body.len() > MAX_REQUEST_BODY_BYTES) {
            return None;
        }
        let body = body.filter(|_| !matches!(method.as_str(), "GET" | "HEAD"));
        let site = web_session::origin_of(url).and_then(|origin| web_session::site_for_origin(&origin))?;
        let request = WebRequest {
            method: method.as_str().to_string(),
            path: path_and_query(url),
            headers: session_headers(headers),
            body: body.map(<[u8]>::to_vec),
            referrer: session_referrer(headers, url),
        };
        request.problem().is_none().then_some((site, request))
    }

    /// Makes one request for the plugin. `access` says whether an in-app browser session may be
    /// created when the site answers with its anti-bot page; one that is already warm is asked
    /// either way.
    pub async fn perform(
        &self,
        request: WireRequest,
        body: Vec<u8>,
        access: SessionUse,
    ) -> Result<WireResponse, String> {
        let url = Url::parse(request.url.trim()).map_err(|_| format!("invalid URL: {}", request.url))?;
        self.check_target(&url)?;

        let method_name = if request.method.is_empty() { "GET" } else { request.method.as_str() };
        let method = Method::from_bytes(method_name.to_ascii_uppercase().as_bytes())
            .map_err(|_| format!("invalid request method: {}", request.method))?;
        let redirect = match request.redirect.as_str() {
            "" | "follow" => "follow",
            "manual" => "manual",
            "error" => "error",
            other => return Err(format!("invalid redirect option: {}", other)),
        };
        if body.len() > MAX_REQUEST_BYTES {
            return Err("the request body is too large".to_string());
        }
        let timeout = match request.timeout_seconds {
            Some(seconds) if seconds.is_finite() && seconds > 0.0 => {
                Duration::from_secs_f64(seconds.min(MAX_TIMEOUT.as_secs_f64()))
            }
            _ => DEFAULT_TIMEOUT,
        };
        let headers = self.request_headers(&request);
        let body = request.has_body.then_some(body.as_slice());

        // A refused request is repeated inside the site's own page when a page could make it.
        let through_session = self.session_request(&url, &method, &headers, body, redirect);
        let site = through_session.as_ref().map(|(site, _)| *site);

        let skip_plain = match site {
            Some(site) if plain_blocked_recently(site.key) => {
                matches!(web_session::choose(self.session.as_ref(), site, access), SessionChoice::Ask)
            }
            _ => false,
        };

        let mut refused: Option<WireResponse> = None;
        if !skip_plain {
            let response = self.plain(&url, &method, &headers, body, redirect, timeout).await?;
            let lowered: HashMap<String, String> = response
                .meta
                .headers
                .iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
                .collect();
            let head = String::from_utf8_lossy(&response.body[..response.body.len().min(32 * 1024)]);
            if detect_barrier(response.meta.status, &lowered, &head).is_none() {
                self.note(NetNote::Answered);
                if let Some(site) = site {
                    web_session::clear_plain_refusal(site);
                }
                return Ok(response);
            }
            match site {
                Some(site) => note_plain_blocked(site.key),
                None => {
                    self.note(NetNote::Blocked);
                    return Ok(response);
                }
            }
            refused = Some(response);
        }

        let Some((site, asked)) = through_session else {
            return refused.ok_or_else(|| "blocked by anti-bot check".to_string());
        };
        {
            let mut guard = SITE_PLUGINS.lock().unwrap_or_else(|p| p.into_inner());
            guard
                .entry(site.key.to_string())
                .or_default()
                .insert(self.plugin_id.clone());
        }
        let give_up = |this: &Self, note: NetNote, reason: &str| -> Result<WireResponse, String> {
            this.note(note);
            refused.clone().ok_or_else(|| reason.to_string())
        };

        match web_session::choose(self.session.as_ref(), site, access) {
            SessionChoice::Keep => {
                // Listed for Verify, so the user can get the site going again by hand.
                if let Some(origin) = web_session::origin_of(&url) {
                    web_session::note_plain_refusal(site, &origin, &asked);
                }
                return give_up(self, NetNote::Blocked, "blocked by anti-bot check");
            }
            SessionChoice::Refused(AppError::VerificationRequired { site, origin }) => {
                return give_up(self, NetNote::NeedsVerification { site, origin }, "needs verification");
            }
            SessionChoice::Refused(_) => return give_up(self, NetNote::Blocked, "blocked by anti-bot check"),
            SessionChoice::Ask => {}
        }

        let warm = self.session.state(site) == SessionState::Warm;
        let session_timeout = if warm {
            SESSION_REQUEST_TIMEOUT.min(timeout.max(Duration::from_secs(5)))
        } else {
            SESSION_START_TIMEOUT
        };
        tracing::info!(
            "{} refuses the HTTP client; plugin {} asks {} {} through the in-app browser ({})",
            site.label,
            self.plugin_id,
            asked.method,
            url.path(),
            if warm { "session in use" } else { "starting the session" }
        );
        let answer = {
            let _waiting = self.start_wait();
            self.session.fetch(site, &asked, session_timeout).await
        };
        match answer {
            Ok(answer) => {
                self.note(NetNote::Answered);
                let mut header_pairs: Vec<(String, String)> = answer.headers.into_iter().collect();
                header_pairs.sort();
                Ok(with_text(WireResponse {
                    meta: WireResponseMeta {
                        status: answer.status,
                        status_text: status_text(answer.status),
                        url: answer.url,
                        redirected: answer.redirected,
                        headers: header_pairs,
                        text: None,
                    },
                    body: answer.body,
                }))
            }
            Err(AppError::VerificationRequired { site, origin }) => {
                tracing::info!("plugin {}: {} needs the user to verify", self.plugin_id, origin);
                give_up(self, NetNote::NeedsVerification { site, origin }, "needs verification")
            }
            Err(other) => {
                tracing::warn!(
                    "in-app browser request for plugin {} failed: {}",
                    self.plugin_id,
                    other.reason()
                );
                give_up(self, NetNote::SessionFailed(other.reason()), "blocked by anti-bot check")
            }
        }
    }
}

/// Adds the decoded text to an answer that declares a character set other than UTF-8, so that
/// `text()` and `json()` read what the site meant; the bytes stay as they were sent.
fn with_text(mut response: WireResponse) -> WireResponse {
    let content_type = response
        .meta
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.as_str());
    if let Some(encoding) = declared_charset(content_type) {
        if encoding != encoding_rs::UTF_8 && response.body.len() <= MAX_DECODED_TEXT_BYTES {
            let (text, _, _) = encoding.decode(&response.body);
            response.meta.text = Some(text.into_owned());
        }
    }
    response
}
