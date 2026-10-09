//! A loopback web server and fixtures shared by the plugin tests.
#![allow(dead_code)]

use anilili_lib::error::AppError;
use anilili_lib::plugins::engine::{EngineLimits, EngineSpec, PluginRuntime};
use anilili_lib::web_session::{SessionState, SiteSession, SiteSpec, WebResponse};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
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
        _path: &str,
        _headers: &[(&str, &str)],
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
        path: &str,
        _headers: &[(&str, &str)],
        _timeout: Duration,
    ) -> Result<WebResponse, AppError> {
        self.asked.lock().unwrap().push(path.to_string());
        if self.refuse {
            return Err(AppError::VerificationRequired {
                site: site.key.to_string(),
                origin: site.origins[0].to_string(),
            });
        }
        Ok(WebResponse {
            status: 200,
            origin: site.origins[0].to_string(),
            headers: HashMap::from([("content-type".to_string(), "application/json".to_string())]),
            body: self.body.clone(),
        })
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
