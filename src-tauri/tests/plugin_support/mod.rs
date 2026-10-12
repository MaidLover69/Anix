//! A loopback web server and fixtures shared by the plugin tests.
#![allow(dead_code)]

use anix_lib::error::AppError;
use anix_lib::plugins::engine::{EngineLimits, EngineSpec, PluginRuntime};
use anix_lib::web_session::{
    self, Asked, DocumentState, Outcome, PageAccess, SessionState, SiteSession, SiteSpec, WebRequest, WebResponse,
};
use std::time::Instant;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct Req {
    pub method: String,
    /// Path with the query string.
    pub path: String,
    /// By lower-case name.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Req {
    pub fn header(&self, name: &str) -> &str {
        self.headers.get(name).map(String::as_str).unwrap_or("")
    }

    pub fn route(&self) -> &str {
        self.path.split('?').next().unwrap_or("")
    }

    pub fn query(&self, name: &str) -> String {
        let query = self.path.split_once('?').map(|(_, q)| q).unwrap_or("");
        url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default()
    }
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".to_string(), content_type.to_string())],
            body: body.into(),
        }
    }
    pub fn text(body: &str) -> Self {
        Self::new(200, "text/plain; charset=utf-8", body)
    }
    pub fn html(body: &str) -> Self {
        Self::new(200, "text/html; charset=utf-8", body)
    }
    pub fn json(value: Value) -> Self {
        Self::new(200, "application/json", value.to_string())
    }
    pub fn status(status: u16) -> Self {
        Self::new(status, "text/plain", format!("status {}", status))
    }
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
}

/// A web server on a loopback port that answers with whatever the handler returns.
pub struct Site {
    pub addr: SocketAddr,
    hits: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Req> {
    let mut data: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&data[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = data[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(Req {
        method,
        path,
        headers,
        body,
    })
}

impl Site {
    pub async fn start(handler: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let hits: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(handler);
        let seen = hits.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let (handler, seen) = (handler.clone(), seen.clone());
                tokio::spawn(async move {
                    let Some(request) = read_request(&mut stream).await else {
                        return;
                    };
                    seen.lock().unwrap().push(format!("{} {}", request.method, request.path));
                    let response = handler(&request);
                    let mut head = format!("HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n", response.status, response.body.len());
                    for (name, value) in &response.headers {
                        head.push_str(&format!("{}: {}\r\n", name, value));
                    }
                    head.push_str("\r\n");
                    let _ = stream.write_all(head.as_bytes()).await;
                    if request.method != "HEAD" {
                        let _ = stream.write_all(&response.body).await;
                    }
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self { addr, hits, task }
    }

    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }

    pub fn count(&self, needle: &str) -> usize {
        self.hits().iter().filter(|hit| hit.contains(needle)).count()
    }
}

/// The in-app browser as it is without an app: never available.
pub struct NoSession;

#[async_trait]
impl SiteSession for NoSession {
    fn available(&self) -> bool {
        false
    }
    fn state(&self, _site: &'static SiteSpec) -> SessionState {
        SessionState::Cold
    }
    async fn fetch(
        &self,
        _site: &'static SiteSpec,
        _request: &WebRequest,
        _timeout: Duration,
    ) -> Result<WebResponse, AppError> {
        Err(AppError::Other("no session".to_string()))
    }
}

/// A stand-in for the in-app browser that answers every path with one body.
pub struct FakeSession {
    pub body: String,
    pub asked: Mutex<Vec<String>>,
    pub refuse: bool,
}

impl FakeSession {
    pub fn answering(body: &str) -> Arc<Self> {
        Arc::new(Self {
            body: body.to_string(),
            asked: Mutex::new(Vec::new()),
            refuse: false,
        })
    }
    pub fn refusing() -> Arc<Self> {
        Arc::new(Self {
            body: String::new(),
            asked: Mutex::new(Vec::new()),
            refuse: true,
        })
    }
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait]
impl SiteSession for FakeSession {
    fn available(&self) -> bool {
        true
    }
    fn state(&self, _site: &'static SiteSpec) -> SessionState {
        SessionState::Cold
    }
    async fn fetch(
        &self,
        site: &'static SiteSpec,
        request: &WebRequest,
        _timeout: Duration,
    ) -> Result<WebResponse, AppError> {
        self.asked.lock().unwrap().push(request.path.clone());
        if self.refuse {
            return Err(AppError::VerificationRequired {
                site: site.key.to_string(),
                origin: site.origins[0].to_string(),
            });
        }
        Ok(WebResponse {
            status: 200,
            origin: site.origins[0].to_string(),
            url: format!("{}{}", site.origins[0], request.path),
            redirected: false,
            headers: HashMap::from([("content-type".to_string(), "application/json".to_string())]),
            body: self.body.clone().into_bytes(),
        })
    }
}

/// The cookie a `guarded` site wants to see; only the browser stand-in has it.
pub const PASS_COOKIE: &str = "passed=1";

/// The page a guarded site answers a visitor with who has not passed its check.
pub const CHECK_PAGE: &str =
    "<!DOCTYPE html><html><head><title>Just a moment...</title></head><body><script>window._cf_chl_opt={}</script></body></html>";

/// Wraps a handler so that requests without the cookie get the site's check page (HTTP 403)
/// for its front page and every path under `/guarded/`.
pub fn guarded(handler: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> impl Fn(&Req) -> Resp + Send + Sync + 'static {
    move |request: &Req| {
        let checked = request.route() == "/" || request.route().starts_with("/guarded/");
        if checked && !request.header("cookie").contains(PASS_COOKIE) {
            return Resp::new(403, "text/html; charset=utf-8", CHECK_PAGE);
        }
        handler(request)
    }
}

/// A stand-in for the in-app browser that behaves like one towards loopback sites: each origin
/// has a session of its own that is refused until `verify` (the user passing the check) and
/// then makes real requests carrying the cookie the site wants, with the method, headers and
/// body it was given, following redirects, and handing back status, headers and bytes. What an
/// outcome means is decided by the app's own `web_session::ask`, from the request's outcome and
/// the document the "window" is on (the site's front page as this visitor gets it).
pub struct BrowserStandIn {
    client: reqwest::Client,
    /// Origins whose check the user passed; the cookie is sent only to these.
    verified: Mutex<HashSet<String>>,
    warm: Mutex<HashSet<String>>,
    refused: Mutex<HashSet<String>>,
    /// Origins that answered in an earlier run of the app.
    answered_before: Mutex<HashSet<String>>,
    asked: Mutex<Vec<(String, WebRequest)>>,
    /// How long the first request to an origin takes (a window being created, the site loading).
    start_delay: Mutex<Duration>,
    /// Origins whose check page aborts the requests made in it, as a check page that reloads
    /// itself does: they fail like a network error instead of being answered.
    check_aborts: Mutex<HashSet<String>>,
    /// Paths whose requests fail inside the page whatever page it is.
    failing_paths: Mutex<HashSet<String>>,
}

/// One origin's page in the stand-in.
struct StandInPage<'a> {
    browser: &'a BrowserStandIn,
    timeout: Duration,
}

impl BrowserStandIn {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            client: reqwest::Client::builder().no_proxy().build().expect("client"),
            verified: Mutex::new(HashSet::new()),
            warm: Mutex::new(HashSet::new()),
            refused: Mutex::new(HashSet::new()),
            answered_before: Mutex::new(HashSet::new()),
            asked: Mutex::new(Vec::new()),
            start_delay: Mutex::new(Duration::ZERO),
            check_aborts: Mutex::new(HashSet::new()),
            failing_paths: Mutex::new(HashSet::new()),
        })
    }

    /// While `origin` shows its check page, requests made in the page fail instead of being
    /// answered.
    pub fn set_check_page_aborts(&self, origin: &str) {
        self.check_aborts.lock().unwrap().insert(origin.to_string());
    }

    /// Requests for `path` fail inside the page, on any page.
    pub fn set_failing_path(&self, path: &str) {
        self.failing_paths.lock().unwrap().insert(path.to_string());
    }

    fn is_verified(&self, origin: &str) -> bool {
        self.verified.lock().unwrap().contains(origin)
    }

    /// What the verification window's probe finds for a request that was refused.
    pub async fn probe(&self, origin: &str, refused: &WebRequest) -> Option<bool> {
        let page = StandInPage { browser: self, timeout: Duration::from_secs(5) };
        web_session::verification_probe(&page, origin, refused, Instant::now() + Duration::from_secs(5)).await
    }

    /// The user passes the check of `origin` in its window.
    pub fn verify(&self, origin: &str) {
        self.verified.lock().unwrap().insert(origin.to_string());
        self.refused.lock().unwrap().remove(origin);
    }

    /// The site stops accepting what the session earned.
    pub fn revoke(&self, origin: &str) {
        self.verified.lock().unwrap().remove(origin);
    }

    /// The app is started again: no windows, the webview's cookies still on disk.
    pub fn restart(&self) {
        let warm: Vec<String> = self.warm.lock().unwrap().drain().collect();
        self.answered_before.lock().unwrap().extend(warm);
        self.refused.lock().unwrap().clear();
    }

    pub fn set_start_delay(&self, delay: Duration) {
        *self.start_delay.lock().unwrap() = delay;
    }

    /// Every request made through the stand-in: the origin and the request.
    pub fn asked(&self) -> Vec<(String, WebRequest)> {
        self.asked.lock().unwrap().clone()
    }

    pub fn asked_paths(&self, origin: &str) -> Vec<String> {
        self.asked()
            .into_iter()
            .filter(|(asked, _)| asked == origin)
            .map(|(_, request)| format!("{} {}", request.method, request.path))
            .collect()
    }
}

#[async_trait]
impl SiteSession for BrowserStandIn {
    fn available(&self) -> bool {
        true
    }

    fn state(&self, site: &'static SiteSpec) -> SessionState {
        let origin = site.origins[0];
        if self.refused.lock().unwrap().contains(origin) {
            SessionState::Refused(origin.to_string())
        } else if self.warm.lock().unwrap().contains(origin) {
            SessionState::Warm
        } else if self.answered_before.lock().unwrap().contains(origin) {
            SessionState::Resumable
        } else {
            SessionState::Cold
        }
    }

    async fn fetch(
        &self,
        site: &'static SiteSpec,
        request: &WebRequest,
        timeout: Duration,
    ) -> Result<WebResponse, AppError> {
        let origin = site.origins[0].to_string();
        assert!(request.problem().is_none(), "not a request a page can make: {:?}", request);
        self.asked.lock().unwrap().push((origin.clone(), request.clone()));

        let starting = !self.warm.lock().unwrap().contains(&origin);
        let delay = *self.start_delay.lock().unwrap();
        if starting && !delay.is_zero() {
            if delay >= timeout {
                tokio::time::sleep(timeout).await;
                return Err(AppError::Network(format!("{} is still loading in the in-app browser", site.label)));
            }
            tokio::time::sleep(delay).await;
        }

        let page = StandInPage { browser: self, timeout };
        match web_session::ask(&page, &origin, request, Instant::now() + timeout).await {
            Asked::Answer(response) => {
                self.warm.lock().unwrap().insert(origin);
                Ok(response)
            }
            Asked::Refused => {
                self.warm.lock().unwrap().remove(&origin);
                self.answered_before.lock().unwrap().remove(&origin);
                self.refused.lock().unwrap().insert(origin.clone());
                Err(AppError::VerificationRequired {
                    site: site.key.to_string(),
                    origin,
                })
            }
            Asked::RequestFailed(reason) => Err(AppError::Network(format!(
                "the request to {} failed in the in-app browser ({})",
                site.label, reason
            ))),
            Asked::Other(outcome) => Err(AppError::Network(format!("{:?} in the in-app browser", outcome))),
        }
    }
}

#[async_trait]
impl PageAccess for StandInPage<'_> {
    async fn request(&self, origin: &str, request: &WebRequest, _deadline: Instant) -> Outcome {
        let browser = self.browser;
        let verified = browser.is_verified(origin);
        let aborted = !verified && browser.check_aborts.lock().unwrap().contains(origin);
        let route = request.path.split('?').next().unwrap_or_default().to_string();
        if aborted || browser.failing_paths.lock().unwrap().contains(&route) {
            return Outcome::Failed("TypeError: Load failed".to_string());
        }
        let mut outgoing = browser
            .client
            .request(reqwest::Method::from_bytes(request.method.as_bytes()).unwrap(), format!("{}{}", origin, request.path))
            .timeout(self.timeout);
        for (name, value) in &request.headers {
            outgoing = outgoing.header(name, value);
        }
        // What the browser adds by itself: the session's cookies, the page as referrer.
        if verified {
            outgoing = outgoing.header("cookie", PASS_COOKIE);
        }
        let referrer = request.referrer.clone().unwrap_or_else(|| "/".to_string());
        outgoing = outgoing.header("referer", format!("{}{}", origin, referrer));
        if let Some(body) = &request.body {
            outgoing = outgoing.body(body.clone());
        }
        let Ok(response) = outgoing.send().await else {
            return Outcome::Failed("TypeError: Load failed".to_string());
        };
        let status = response.status().as_u16();
        let url = response.url().to_string();
        let mut headers: HashMap<String, String> = HashMap::new();
        for (name, value) in response.headers() {
            // A page's script is never shown the cookies a site sets.
            if name.as_str() != "set-cookie" {
                headers.insert(name.as_str().to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned());
            }
        }
        let Ok(body) = response.bytes().await else {
            return Outcome::Failed("TypeError: Load failed".to_string());
        };
        Outcome::Response(WebResponse {
            status,
            origin: origin.to_string(),
            redirected: url != format!("{}{}", origin, request.path),
            url,
            headers,
            body: body.to_vec(),
        })
    }

    /// The document of the window: the site's front page as this visitor is served it.
    async fn document(&self, origin: &str) -> DocumentState {
        let mut outgoing = self.browser.client.get(format!("{}/", origin)).timeout(self.timeout);
        if self.browser.is_verified(origin) {
            outgoing = outgoing.header("cookie", PASS_COOKIE);
        }
        let Ok(response) = outgoing.send().await else {
            return DocumentState::Unreadable;
        };
        let markup = response.text().await.unwrap_or_default();
        let title = markup
            .split_once("<title>")
            .and_then(|(_, rest)| rest.split_once("</title>"))
            .map(|(title, _)| title.to_string())
            .unwrap_or_default();
        match web_session::document_barrier(&title, &markup) {
            Some(kind) => DocumentState::Barrier(kind),
            None => DocumentState::Site,
        }
    }
}

pub fn spec(id: &str, source: &str) -> EngineSpec {
    EngineSpec {
        id: id.to_string(),
        source: source.to_string(),
        typescript: false,
        preferences: HashMap::new(),
        limits: EngineLimits::default(),
        session: Arc::new(NoSession),
        allow_private_network: true,
        log: Some(Arc::new(|_, _| {})),
    }
}

pub fn runtime(id: &str, source: &str) -> Arc<PluginRuntime> {
    Arc::new(PluginRuntime::new(spec(id, source)))
}

pub fn reference_repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../anime_repo")
        .join(relative)
}
