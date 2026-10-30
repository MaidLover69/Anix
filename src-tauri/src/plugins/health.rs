//! Settings › Plugins tools: a quick health test per plugin (one search for a well-known
//! title, nothing played) and the directory of extension lists published on the project site.

use crate::error::AppError;
use crate::plugins::engine::PluginError;
use crate::plugins::registry::PluginRegistry;
use crate::web_session::SessionUse;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longest a test search may take.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_DIRECTORY_BYTES: usize = 128 * 1024;
const DIRECTORY_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_DIRECTORY_LISTS: usize = 100;

/// The title every plugin is asked for; it is on practically every anime site.
pub fn test_media() -> Value {
    json!({
        "id": 21,
        "idMal": 21,
        "status": "RELEASING",
        "format": "TV",
        "englishTitle": "One Piece",
        "romajiTitle": "ONE PIECE",
        "episodeCount": Value::Null,
        "absoluteSeasonOffset": 0,
        "synonyms": [],
        "isAdult": false,
        "startDate": { "year": 1999 },
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginHealth {
    pub id: String,
    /// `ok` (results came back), `empty` (answered, nothing found), `timeout`,
    /// `verification` (the site wants its check passed in the in-app browser) or `error`.
    pub status: String,
    pub latency_ms: u64,
    pub results: usize,
    pub error: Option<String>,
    /// When the test ran, in Unix milliseconds; 0 when unknown.
    #[serde(default)]
    pub checked_at_ms: i64,
}

/// How a search answer or failure is reported.
pub fn classify(id: &str, outcome: Result<Value, AppError>, latency: Duration) -> PluginHealth {
    let latency_ms = latency.as_millis().min(u64::MAX as u128) as u64;
    let mut health = PluginHealth {
        id: id.to_string(),
        status: "error".to_string(),
        latency_ms,
        results: 0,
        error: None,
        checked_at_ms: now_ms(),
    };
    match outcome {
        Ok(Value::Array(list)) => {
            health.results = list.len();
            health.status = if list.is_empty() { "empty" } else { "ok" }.to_string();
        }
        Ok(_) => health.error = Some("search did not return a list".to_string()),
        Err(AppError::CaptchaRequired) | Err(AppError::VerificationRequired { .. }) => {
            health.status = "verification".to_string();
            health.error = Some("the site asks for its check in the in-app browser (use Verify)".to_string());
        }
        Err(AppError::Provider { message, .. }) if message.ends_with("timed out") => {
            health.status = "timeout".to_string();
            health.error = Some(message);
        }
        Err(e) => health.error = Some(e.reason()),
    }
    health
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- results kept between runs

/// The latest test result per plugin and when the background check last went through them all.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HealthRecord {
    #[serde(default)]
    pub last_round_ms: i64,
    #[serde(default)]
    pub results: HashMap<String, PluginHealth>,
}

const HEALTH_FILE: &str = "plugin-health.json";
static RECORD: Mutex<Option<HealthRecord>> = Mutex::new(None);

fn record_path() -> Option<PathBuf> {
    crate::db::get_config_dir().ok().map(|dir| dir.join(HEALTH_FILE))
}

pub fn read_record(path: &Path) -> HealthRecord {
    std::fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn loaded(guard: &mut Option<HealthRecord>) -> &mut HealthRecord {
    guard.get_or_insert_with(|| record_path().map(|path| read_record(&path)).unwrap_or_default())
}

/// Reads the kept results without saving them.
fn peek_record<T>(read: impl FnOnce(&HealthRecord) -> T) -> T {
    read(loaded(&mut RECORD.lock().unwrap_or_else(|p| p.into_inner())))
}

/// Changes the kept results and saves them.
fn with_record<T>(change: impl FnOnce(&mut HealthRecord) -> T) -> T {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    let record = loaded(&mut guard);
    let out = change(record);
    if let (Some(path), Ok(data)) = (record_path(), serde_json::to_vec(record)) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            tracing::warn!("could not save the plugin test results");
        }
    }
    out
}

/// Keeps a test result as the plugin's latest.
pub fn remember(health: &PluginHealth) {
    with_record(|record| {
        record.results.insert(health.id.clone(), health.clone());
    });
}

/// The latest test result of every plugin that has one.
pub fn remembered() -> Vec<PluginHealth> {
    let mut list: Vec<PluginHealth> = peek_record(|record| record.results.values().cloned().collect());
    list.sort_by(|a, b| a.id.cmp(&b.id));
    list
}

// ---------------------------------------------------------------- the background check

/// The background check goes through the enabled plugins about once a day.
pub const ROUND_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often it looks whether a round is due.
const TICK: Duration = Duration::from_secs(10 * 60);
/// It waits until nothing was played or listed for this long (playback counts through the
/// stream proxy's requests), and stops between plugins when that changes.
pub const IDLE_FOR: Duration = Duration::from_secs(10 * 60);
pub const EVENT_HEALTH_CHANGED: &str = "plugin-health-changed";

/// Whether a background round should start now: switched on, the last full round a day ago or
/// more, and the app idle (no stream or episode lookup for `IDLE_FOR`).
pub fn round_due(enabled: bool, last_round_ms: i64, now_ms: i64, since_lookup: Option<Duration>) -> bool {
    let idle = since_lookup.map_or(true, |since| since >= IDLE_FOR);
    enabled && idle && now_ms.saturating_sub(last_round_ms) >= ROUND_EVERY.as_millis() as i64
}

fn background_enabled() -> bool {
    crate::commands::settings::saved_settings()
        .get("plugin_health_checks")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Tests the enabled plugins one after another, at most once a day and only while the app is
/// idle; results replace the ones shown in Settings › Plugins as they come in. A round that
/// is interrupted by playback goes on at the next idle moment.
pub fn start_background(registry: PluginRegistry, announce: std::sync::Arc<dyn Fn() + Send + Sync>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            let last = peek_record(|record| record.last_round_ms);
            let since = crate::providers::manager::since_last_lookup();
            if !round_due(background_enabled(), last, now_ms(), since) {
                continue;
            }
            let due_before = now_ms() - ROUND_EVERY.as_millis() as i64;
            let ids: Vec<String> = registry
                .list_plugins()
                .into_iter()
                .filter(|p| p.enabled)
                .map(|p| p.id)
                .collect();
            let mut finished = true;
            for id in ids {
                // Tested in this round already (a round interrupted earlier).
                let fresh = peek_record(|record| record.results.get(&id).map_or(false, |h| h.checked_at_ms > due_before));
                if fresh {
                    continue;
                }
                let busy = crate::providers::manager::since_last_lookup().map_or(false, |since| since < IDLE_FOR);
                if busy || !background_enabled() {
                    finished = false;
                    break;
                }
                let health = test_plugin(&registry, &id).await;
                remember(&health);
                announce();
            }
            if finished {
                with_record(|record| record.last_round_ms = now_ms());
            }
        }
    });
}

/// Runs the test search through one installed plugin. Uses an existing in-app browser session
/// but never opens a new one.
pub async fn test_plugin(registry: &PluginRegistry, id: &str) -> PluginHealth {
    // Prepares the runtimes of every enabled plugin that loaded.
    let _ = registry.providers();
    let Some(runtime) = registry.runtime(id) else {
        let reason = registry
            .list_plugins()
            .into_iter()
            .find(|p| p.id == id)
            .map(|p| match (p.enabled, p.error) {
                (false, _) => "the plugin is switched off".to_string(),
                (true, Some(error)) => error,
                (true, None) => "the plugin is not loaded".to_string(),
            })
            .unwrap_or_else(|| "no such plugin is installed".to_string());
        return PluginHealth {
            id: id.to_string(),
            status: "error".to_string(),
            latency_ms: 0,
            results: 0,
            error: Some(reason),
            checked_at_ms: now_ms(),
        };
    };
    let options = json!({ "media": test_media(), "query": "One Piece", "dub": false, "year": 1999 });
    let started = Instant::now();
    let result = runtime
        .call("search", &[options], TEST_TIMEOUT, SessionUse::Existing)
        .await;
    let latency = started.elapsed();
    let outcome = result.map_err(|error| {
        let net = runtime.running().and_then(|engine| engine.net().failure_since(started));
        net.unwrap_or_else(|| match error {
            PluginError::Timeout => AppError::Provider {
                provider: "plugin",
                message: "search timed out".to_string(),
            },
            other => AppError::Other(other.reason()),
        })
    });
    classify(id, outcome, latency)
}

/// One extension list of the site's directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirectoryList {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub url: String,
    #[serde(default)]
    pub languages: Vec<String>,
}

/// Reads `directory.json`: `{ "lists": [ { name, description, url, languages } ] }`. Entries
/// without a name or an https link are dropped, as are repeated links.
pub fn parse_directory(data: &[u8]) -> Result<Vec<DirectoryList>, String> {
    let json: Value = serde_json::from_slice(data).map_err(|_| "the directory is not JSON".to_string())?;
    let lists = json
        .get("lists")
        .and_then(Value::as_array)
        .ok_or_else(|| "the directory has no \"lists\"".to_string())?;
    let mut out: Vec<DirectoryList> = Vec::new();
    for entry in lists.iter().take(MAX_DIRECTORY_LISTS) {
        let Ok(mut list) = serde_json::from_value::<DirectoryList>(entry.clone()) else {
            continue;
        };
        list.name = list.name.trim().chars().take(80).collect();
        list.description = list.description.trim().chars().take(300).collect();
        list.url = list.url.trim().to_string();
        list.languages = list
            .languages
            .iter()
            .map(|code| code.trim().to_ascii_lowercase())
            .filter(|code| (2..=3).contains(&code.len()) && code.chars().all(|c| c.is_ascii_lowercase()))
            .collect();
        let https = reqwest::Url::parse(&list.url).map_or(false, |url| url.scheme() == "https" && url.host_str().is_some());
        if list.name.is_empty() || !https || out.iter().any(|known| known.url == list.url) {
            continue;
        }
        out.push(list);
    }
    Ok(out)
}

pub fn directory_url(site: &str) -> String {
    format!("{}/directory.json", site.trim().trim_end_matches('/'))
}

/// The site's directory. An address that does not exist (or the site being out of reach)
/// gives an empty list: the directory is optional.
pub async fn fetch_directory(client: &reqwest::Client, site: &str) -> Vec<DirectoryList> {
    let fetched = async {
        let mut response = client
            .get(directory_url(site))
            .timeout(DIRECTORY_TIMEOUT)
            .send()
            .await
            .map_err(|e| AppError::from(e).reason())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status().as_u16()));
        }
        let mut data = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| AppError::from(e).reason())? {
            if data.len() + chunk.len() > MAX_DIRECTORY_BYTES {
                return Err("the directory is too large".to_string());
            }
            data.extend_from_slice(&chunk);
        }
        parse_directory(&data)
    }
    .await;
    fetched.unwrap_or_else(|reason| {
        tracing::info!("No extension directory: {}", reason);
        Vec::new()
    })
}
