//! Shared by the `account_*_tests`: a loopback server standing in for AniList and
//! MyAnimeList, a settings file in a temp directory and a notifier that records.

#![allow(dead_code)]

use anix_lib::accounts;

use accounts::session::{Context, Notice, Notifier};
use accounts::store::AccountStore;
use anix_lib::clients::anilist::AniListClient;
use anix_lib::clients::mal::MalClient;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Response, Server};

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    /// Path and query.
    pub url: String,
    pub authorization: Option<String>,
    pub body: String,
}

impl Recorded {
    /// The value of one form field of the body.
    pub fn form(&self, key: &str) -> Option<String> {
        url::form_urlencoded::parse(self.body.as_bytes())
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.into_owned())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

/// Answers every request with what `respond` returns (status, JSON body) and remembers it.
pub struct FakeServer {
    pub base: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    server: Arc<Server>,
}

impl FakeServer {
    pub fn start(respond: impl Fn(&Recorded) -> (u16, String) + Send + 'static) -> Self {
        let server = Arc::new(Server::http("127.0.0.1:0").expect("bind loopback"));
        let port = server.server_addr().to_ip().expect("ip address").port();
        let requests = Arc::new(Mutex::new(Vec::new()));

        let worker = Arc::clone(&server);
        let seen = Arc::clone(&requests);
        std::thread::spawn(move || {
            while let Ok(mut request) = worker.recv() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let recorded = Recorded {
                    method: request.method().to_string(),
                    url: request.url().to_string(),
                    authorization: request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("Authorization"))
                        .map(|h| h.value.to_string()),
                    body,
                };
                let (status, text) = respond(&recorded);
                seen.lock().unwrap().push(recorded);
                let response = Response::from_string(text)
                    .with_status_code(status)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
            }
        });

        Self {
            base: format!("http://127.0.0.1:{}", port),
            requests,
            server,
        }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }

    /// Requests whose path and query contain `part`.
    pub fn requests_to(&self, part: &str) -> Vec<Recorded> {
        self.requests().into_iter().filter(|r| r.url.contains(part)).collect()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

/// An address nothing listens on.
pub fn closed_address() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local address").port();
    drop(listener);
    format!("http://127.0.0.1:{}", port)
}

#[derive(Default)]
pub struct Recorder(pub Mutex<Vec<Notice>>);

impl Notifier for Recorder {
    fn notify(&self, notice: Notice) {
        self.0.lock().unwrap().push(notice);
    }
}

impl Recorder {
    pub fn notices(&self) -> Vec<Notice> {
        self.0.lock().unwrap().clone()
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// One test's world: clients pointed at `base`, a private settings file and database.
pub struct World {
    dir: PathBuf,
    pub store: AccountStore,
    pub anilist: AniListClient,
    pub mal: MalClient,
    pub pool: SqlitePool,
    pub recorder: Recorder,
}

impl World {
    /// AniList at `<base>/graphql`, the MAL API at `<base>/v2`, its token endpoint at `<base>/token`.
    pub async fn new(base: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-account-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let client = reqwest::Client::builder().no_proxy().build().expect("http client");
        Self {
            store: AccountStore::at(dir.join("settings.json")),
            anilist: AniListClient::with_base_url(client.clone(), &format!("{}/graphql", base)),
            mal: MalClient::with_base_urls(client, &format!("{}/v2", base), &format!("{}/token", base)),
            pool: anix_lib::db::init_db_in_memory().await.expect("in-memory database"),
            recorder: Recorder::default(),
            dir,
        }
    }

    pub fn ctx(&self) -> Context<'_> {
        Context {
            store: &self.store,
            anilist: &self.anilist,
            mal: &self.mal,
            pool: &self.pool,
            notifier: &self.recorder,
        }
    }

    pub fn settings(&self) -> serde_json::Value {
        let text = std::fs::read_to_string(self.store.path()).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str(&text).expect("settings are JSON")
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn far_future() -> i64 {
    anix_lib::db::current_unix_timestamp() + 30 * 24 * 3600
}
