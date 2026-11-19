//! Loopback HTTP server for the #8 download-engine tests. Every request runs on its own thread so
//! the server can count how many are in flight at once.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Req {
    pub path: String,
    pub range: Option<String>,
    pub if_range: Option<String>,
    /// Requests in flight (this one included) when it arrived.
    pub active: usize,
    /// 1-based count of requests to this path so far.
    pub nth: usize,
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay_ms: u64,
}

impl Reply {
    pub fn ok(body: Vec<u8>) -> Self {
        Reply { status: 200, headers: vec![], body, delay_ms: 0 }
    }
    pub fn status(code: u16) -> Self {
        Reply { status: code, headers: vec![], body: b"x".to_vec(), delay_ms: 0 }
    }
    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
    pub fn delay(mut self, ms: u64) -> Self {
        self.delay_ms = ms;
        self
    }
}

pub struct TestServer {
    pub base: String,
    pub log: Arc<Mutex<Vec<Req>>>,
    pub max_active: Arc<AtomicUsize>,
    server: Arc<tiny_http::Server>,
}

impl TestServer {
    pub fn start(handler: impl Fn(&Req) -> Reply + Send + Sync + 'static) -> Self {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("bind"));
        let port = server.server_addr().to_ip().expect("ip").port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(handler);
        let (s, l, a, m) = (server.clone(), log.clone(), active.clone(), max_active.clone());
        std::thread::spawn(move || {
            for request in s.incoming_requests() {
                let (l, a, m, h) = (l.clone(), a.clone(), m.clone(), handler.clone());
                std::thread::spawn(move || serve(request, l, a, m, h));
            }
        });
        TestServer { base: format!("http://127.0.0.1:{}", port), log, max_active, server }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub fn requests(&self) -> Vec<Req> {
        self.log.lock().unwrap().clone()
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_active.load(Ordering::SeqCst)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

fn header_of(request: &tiny_http::Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

fn serve(
    request: tiny_http::Request,
    log: Arc<Mutex<Vec<Req>>>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    handler: Arc<dyn Fn(&Req) -> Reply + Send + Sync>,
) {
    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
    max_active.fetch_max(now, Ordering::SeqCst);
    let path = request.url().to_string();
    let nth = {
        let mut log = log.lock().unwrap();
        let nth = log.iter().filter(|r| r.path == path).count() + 1;
        let req = Req {
            path,
            range: header_of(&request, "range"),
            if_range: header_of(&request, "if-range"),
            active: now,
            nth,
        };
        log.push(req.clone());
        req
    };
    let reply = handler(&nth);
    if reply.delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(reply.delay_ms));
    }
    let mut response = tiny_http::Response::from_data(reply.body).with_status_code(reply.status);
    for (k, v) in reply.headers {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            response.add_header(h);
        }
    }
    let _ = request.respond(response);
    active.fetch_sub(1, Ordering::SeqCst);
}

/// A believable MPEG-TS segment: `0x47` at every 188-byte packet start, the index in the payload.
pub fn ts_segment(index: usize) -> Vec<u8> {
    let mut data = vec![index as u8; 188 * 6];
    for packet in 0..6 {
        data[packet * 188] = 0x47;
    }
    data
}

/// A believable fMP4 fragment: size field, then a `moof` box.
pub fn fmp4_segment() -> Vec<u8> {
    let mut data = vec![0, 0, 0, 24];
    data.extend_from_slice(b"moof");
    data.extend_from_slice(&[0u8; 100]);
    data
}
