//! Installed plugins and the repositories they come from.
//!
//! A repository is a JSON index: an array of plugin manifests (or a single manifest). Everything
//! downloaded is kept under the app's data directory (`plugins/state.json` and
//! `plugins/payloads/`), so installed plugins work offline and across restarts.

use crate::clients::http::http_client;
use crate::plugins::engine::{EngineLimits, EngineSpec, PluginRuntime};
use crate::plugins::provider::{read_settings, PluginProvider, ProviderSettings};
use crate::plugins::userconfig::{self, ConfigOption, SavedConfig, UserConfig};
use crate::providers::AnimeProvider;
use crate::web_session::{app_session, SiteSession};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub const PLUGIN_TYPE: &str = "onlinestream-provider";
/// Emitted to the frontend whenever the set or state of plugins changes.
pub const EVENT_PLUGINS_CHANGED: &str = "plugins-changed";

pub const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PLUGINS_PER_REPO: usize = 200;
const MAX_ID_CHARS: usize = 64;
const MAX_TEXT_CHARS: usize = 600;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Set to `1` to let a repository link be a `file://` directory (for developing plugins
/// before they are published). The directory holds `index.json`; a plugin's code is taken from
/// `src/<id>/<file named like the end of payloadURI>` when that file exists.
pub const DEV_REPOS_ENV: &str = "ANILILI_DEV_PLUGIN_REPOS";
const STATE_FILE: &str = "state.json";
const PAYLOAD_DIR: &str = "payloads";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginRepo {
    pub url: String,
    pub name: String,
    pub plugin_count: usize,
    pub updated_at_ms: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub icon: Option<String>,
    pub lang: String,
    pub repo_url: String,
    pub enabled: bool,
    /// `ready`, `disabled` or `error`.
    pub status: String,
    pub error: Option<String>,
    pub supports_dub: bool,
    pub servers: Vec<String>,
    /// The plugin has settings of its own; see `get_plugin_config`.
    pub configurable: bool,
}

/// One setting of a plugin as the settings form shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginConfigField {
    pub name: String,
    pub label: String,
    /// `text`, `switch` (value `"true"` or `"false"`) or `select`.
    #[serde(rename = "type")]
    pub kind: String,
    pub default: String,
    /// What is in effect: the saved value, or the default.
    pub value: String,
    /// The choices of a `select` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<ConfigOption>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginConfig {
    pub fields: Vec<PluginConfigField>,
    /// The plugin does not load until its settings were saved once.
    pub requires_config: bool,
    /// Settings were saved for the plugin's current configuration version.
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredRepo {
    url: String,
    name: String,
    #[serde(default)]
    updated_at_ms: i64,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredPlugin {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    typescript: bool,
    repo_url: String,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    supports_dub: bool,
    #[serde(default)]
    servers: Vec<String>,
    /// The manifest's `userConfig`.
    #[serde(default)]
    user_config: Option<UserConfig>,
    /// What the user saved for it.
    #[serde(default)]
    saved_config: Option<SavedConfig>,
    /// Changes whenever the payload on disk changes.
    #[serde(default)]
    revision: u64,
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct State {
    #[serde(default)]
    repos: Vec<StoredRepo>,
    #[serde(default)]
    plugins: Vec<StoredPlugin>,
}

/// How a registry reaches the outside; the defaults are what the app uses.
#[derive(Clone)]
pub struct RegistryOptions {
    /// Accept `http://` links and loopback hosts (tests).
    pub allow_insecure: bool,
    /// Let plugin `fetch` reach loopback and private addresses (tests).
    pub allow_private_network: bool,
    pub session: Arc<dyn SiteSession>,
    pub limits: EngineLimits,
}

impl Default for RegistryOptions {
    fn default() -> Self {
        Self {
            allow_insecure: false,
            allow_private_network: false,
            session: app_session(),
            limits: EngineLimits::default(),
        }
    }
}

struct Loaded {
    revision: u64,
    runtime: Arc<PluginRuntime>,
    provider: Arc<PluginProvider>,
}

struct Inner {
    dir: PathBuf,
    options: RegistryOptions,
    state: RwLock<State>,
    loaded: Mutex<HashMap<String, Loaded>>,
    /// Serialises everything that changes the state or the files.
    changes: tokio::sync::Mutex<()>,
    notifier: RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
    revisions: AtomicU64,
}

#[derive(Clone)]
pub struct PluginRegistry {
    inner: Arc<Inner>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Plugin ids are used as file names and provider keys.
pub fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn clipped(text: &str) -> String {
    text.trim().chars().take(MAX_TEXT_CHARS).collect()
}

fn text_field(manifest: &Value, name: &str) -> String {
    manifest.get(name).and_then(Value::as_str).map(clipped).unwrap_or_default()
}

/// The link with a scheme, as it is fetched.
fn with_scheme(link: &str) -> String {
    let link = link.trim();
    if link.contains("://") {
        link.to_string()
    } else {
        format!("https://{}", link)
    }
}

fn same_repo(a: &str, b: &str) -> bool {
    let tidy = |link: &str| {
        with_scheme(link)
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_ascii_lowercase()
    };
    tidy(a) == tidy(b)
}

/// `owner/repo` of a link to a GitHub repository page (not to a file in it).
fn github_repository(url: &Url) -> Option<(String, String)> {
    let host = url.host_str()?.to_ascii_lowercase();
    if host != "github.com" && host != "www.github.com" {
        return None;
    }
    let mut parts = url.path_segments()?.filter(|part| !part.is_empty());
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.trim_end_matches(".git").to_string();
    let rest: Vec<&str> = parts.collect();
    let is_file = rest.first().map_or(false, |kind| matches!(*kind, "blob" | "raw"));
    (!repo.is_empty() && !is_file).then_some((owner, repo))
}

/// Where the index of a repository link may be, in the order to try.
pub fn index_candidates(link: &str) -> Result<Vec<String>, String> {
    let url = Url::parse(&with_scheme(link)).map_err(|_| "That is not a valid link.".to_string())?;
    if url.host_str().map_or(true, str::is_empty) {
        return Err("That is not a valid link.".to_string());
    }
    if let Some((owner, repo)) = github_repository(&url) {
        let mut branches: Vec<String> = Vec::new();
        let parts: Vec<&str> = url.path_segments().map(|p| p.filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        if parts.len() >= 4 && parts[2] == "tree" {
            branches.push(parts[3].to_string());
        }
        for branch in ["main", "master"] {
            if !branches.iter().any(|known| known == branch) {
                branches.push(branch.to_string());
            }
        }
        return Ok(branches
            .into_iter()
            .map(|branch| format!("https://raw.githubusercontent.com/{}/{}/{}/index.json", owner, repo, branch))
            .collect());
    }
    // A link to a file on github.com is read from the raw host.
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host == "github.com" || host == "www.github.com" {
        let parts: Vec<&str> = url.path_segments().map(|p| p.filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        if parts.len() >= 5 && matches!(parts[2], "blob" | "raw") {
            return Ok(vec![format!(
                "https://raw.githubusercontent.com/{}/{}/{}",
                parts[0],
                parts[1],
                parts[3..].join("/")
            )]);
        }
    }
    Ok(vec![url.to_string()])
}

fn repo_name(link: &str) -> String {
    let Ok(url) = Url::parse(&with_scheme(link)) else {
        return link.trim().to_string();
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let parts: Vec<&str> = url.path_segments().map(|p| p.filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    if matches!(host.as_str(), "github.com" | "www.github.com" | "raw.githubusercontent.com") && parts.len() >= 2 {
        return format!("{}/{}", parts[0], parts[1].trim_end_matches(".git"));
    }
    match parts.last() {
        Some(last) if parts.len() > 1 => format!("{}/…/{}", host, last),
        Some(last) => format!("{}/{}", host, last),
        None => host,
    }
}

/// What one index entry asks to be installed.
struct Wanted {
    id: String,
    manifest: Value,
    /// The address the manifest came from, against which a relative `payloadURI` resolves.
    base: Url,
}

impl PluginRegistry {
    /// Opens the registry kept in `dir`, creating it when missing. A state file that cannot be
    /// read is moved aside and the registry starts empty.
    pub fn open(dir: PathBuf, options: RegistryOptions) -> Self {
        let state_path = dir.join(STATE_FILE);
        let state = match std::fs::read_to_string(&state_path) {
            Ok(text) => match serde_json::from_str::<State>(&text) {
                Ok(mut state) => {
                    state.plugins.retain(|plugin| valid_plugin_id(&plugin.id));
                    state
                }
                Err(e) => {
                    tracing::warn!("plugin state {} is unreadable ({}); starting empty", state_path.display(), e);
                    let _ = std::fs::rename(&state_path, state_path.with_extension("json.corrupt"));
                    State::default()
                }
            },
            Err(_) => State::default(),
        };
        let highest = state.plugins.iter().map(|plugin| plugin.revision).max().unwrap_or(0);
        Self {
            inner: Arc::new(Inner {
                dir,
                options,
                state: RwLock::new(state),
                loaded: Mutex::new(HashMap::new()),
                changes: tokio::sync::Mutex::new(()),
                notifier: RwLock::new(None),
                revisions: AtomicU64::new(highest.max(now_ms() as u64)),
            }),
        }
    }

    /// The registry of the running app, in the data directory.
    pub fn open_default() -> Self {
        let dir = crate::db::get_app_dir()
            .map(|dir| dir.join("plugins"))
            .unwrap_or_else(|e| {
                tracing::error!("plugin storage is unavailable: {}", e);
                std::env::temp_dir().join("anilili-plugins")
            });
        Self::open(dir, RegistryOptions::default())
    }

    /// Called after every change; the app emits `plugins-changed` from it.
    pub fn set_notifier(&self, notifier: Arc<dyn Fn() + Send + Sync>) {
        *self.inner.notifier.write().unwrap_or_else(|p| p.into_inner()) = Some(notifier);
    }

    fn notify(&self) {
        let notifier = self.inner.notifier.read().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(notifier) = notifier {
            notifier();
        }
    }

    fn state(&self) -> State {
        self.inner.state.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn payload_path(&self, plugin: &StoredPlugin) -> PathBuf {
        let extension = if plugin.typescript { "ts" } else { "js" };
        self.inner.dir.join(PAYLOAD_DIR).join(format!("{}.{}", plugin.id, extension))
    }

    fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }

    fn store(&self, state: State) {
        let path = self.inner.dir.join(STATE_FILE);
        match serde_json::to_vec_pretty(&state) {
            Ok(data) => {
                if let Err(e) = Self::write_atomic(&path, &data) {
                    tracing::error!("could not save plugin state to {}: {}", path.display(), e);
                }
            }
            Err(e) => tracing::error!("could not serialise plugin state: {}", e),
        }
        *self.inner.state.write().unwrap_or_else(|p| p.into_inner()) = state;
    }

    pub fn list_repos(&self) -> Vec<PluginRepo> {
        let state = self.state();
        state
            .repos
            .iter()
            .map(|repo| PluginRepo {
                url: repo.url.clone(),
                name: repo.name.clone(),
                plugin_count: state.plugins.iter().filter(|p| p.repo_url == repo.url).count(),
                updated_at_ms: repo.updated_at_ms,
                error: repo.error.clone(),
            })
            .collect()
    }

    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.state()
            .plugins
            .iter()
            .map(|plugin| PluginInfo {
                id: plugin.id.clone(),
                name: if plugin.name.is_empty() { plugin.id.clone() } else { plugin.name.clone() },
                version: plugin.version.clone(),
                author: plugin.author.clone(),
                description: plugin.description.clone(),
                icon: plugin.icon.clone(),
                lang: plugin.lang.clone(),
                repo_url: plugin.repo_url.clone(),
                enabled: plugin.enabled,
                status: if !plugin.enabled {
                    "disabled"
                } else if plugin.error.is_some() {
                    "error"
                } else {
                    "ready"
                }
                .to_string(),
                error: plugin.error.clone(),
                supports_dub: plugin.supports_dub,
                servers: plugin.servers.clone(),
                configurable: plugin.user_config.as_ref().map_or(false, |config| !config.fields.is_empty()),
            })
            .collect()
    }

    /// How a site is named to the user: by the installed plugins whose requests met its
    /// anti-bot page, with the host, as in `"AnimeKai (anikai.to)"`.
    pub fn site_label(&self, site_key: &str, host: &str) -> String {
        let ids = crate::plugins::net::plugins_for_site(site_key);
        let names: Vec<String> = self
            .state()
            .plugins
            .iter()
            .filter(|plugin| ids.contains(&plugin.id))
            .map(|plugin| if plugin.name.is_empty() { plugin.id.clone() } else { plugin.name.clone() })
            .collect();
        crate::web_session::label_with_owners(host, &names)
    }

    // ------------------------------------------------------------------ loading

    fn spec_for(&self, plugin: &StoredPlugin) -> Result<EngineSpec, String> {
        let path = self.payload_path(plugin);
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("the downloaded plugin file is missing ({})", e))?;
        let (source, preferences) =
            userconfig::apply(&source, plugin.user_config.as_ref(), plugin.saved_config.as_ref())
                .map_err(|e| e.message().to_string())?;
        Ok(EngineSpec {
            id: plugin.id.clone(),
            source,
            typescript: plugin.typescript,
            preferences,
            limits: self.inner.options.limits,
            session: self.inner.options.session.clone(),
            allow_private_network: self.inner.options.allow_private_network,
            log: None,
        })
    }

    /// Starts the plugin's engine and reads its settings. The outcome is what the stored
    /// `error`, `supports_dub` and `servers` should be.
    async fn load(&self, plugin: &StoredPlugin) -> Result<ProviderSettings, String> {
        let spec = self.spec_for(plugin)?;
        let runtime = Arc::new(PluginRuntime::new(spec));
        let starting = runtime.clone();
        tokio::task::spawn_blocking(move || starting.engine().map(|_| ()))
            .await
            .map_err(|e| format!("the plugin could not be started: {}", e))??;
        let settings = read_settings(&runtime).await?;
        let provider = Arc::new(PluginProvider::new(
            &plugin.id,
            runtime.clone(),
            settings.clone(),
            &plugin.revision.to_string(),
        ));
        self.inner.loaded.lock().unwrap_or_else(|p| p.into_inner()).insert(
            plugin.id.clone(),
            Loaded {
                revision: plugin.revision,
                runtime,
                provider,
            },
        );
        Ok(settings)
    }

    fn unload(&self, id: &str) {
        self.inner.loaded.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    }

    /// Loads one stored plugin and writes the outcome into it.
    async fn refresh_status(&self, plugin: &mut StoredPlugin) {
        match self.load(plugin).await {
            Ok(settings) => {
                plugin.error = None;
                plugin.supports_dub = settings.supports_dub;
                plugin.servers = settings.episode_servers;
            }
            Err(reason) => {
                tracing::warn!("plugin {} is unusable: {}", plugin.id, reason);
                self.unload(&plugin.id);
                plugin.error = Some(reason);
            }
        }
    }

    /// Loads every enabled plugin that is not loaded yet. Run once at startup, in the
    /// background; a plugin that fails to load is marked with the reason.
    pub async fn load_all(&self) {
        let _changing = self.inner.changes.lock().await;
        let mut state = self.state();
        let mut changed = false;
        for plugin in state.plugins.iter_mut().filter(|plugin| plugin.enabled) {
            let already = {
                let loaded = self.inner.loaded.lock().unwrap_or_else(|p| p.into_inner());
                loaded.get(&plugin.id).map_or(false, |l| l.revision == plugin.revision)
            };
            if already {
                continue;
            }
            let before = (plugin.error.clone(), plugin.supports_dub, plugin.servers.clone());
            self.refresh_status(plugin).await;
            changed |= before != (plugin.error.clone(), plugin.supports_dub, plugin.servers.clone());
        }
        if changed {
            self.store(state);
        }
        drop(_changing);
        self.notify();
    }

    /// The providers lookups run against: every enabled plugin without an error, in repository
    /// and index order. A plugin not loaded yet is prepared from its stored settings and starts
    /// on first use.
    pub fn providers(&self) -> Vec<Arc<dyn AnimeProvider>> {
        let state = self.state();
        let mut loaded = self.inner.loaded.lock().unwrap_or_else(|p| p.into_inner());
        let mut providers: Vec<Arc<dyn AnimeProvider>> = Vec::new();
        for plugin in state.plugins.iter().filter(|p| p.enabled && p.error.is_none()) {
            if let Some(known) = loaded.get(&plugin.id).filter(|l| l.revision == plugin.revision) {
                providers.push(known.provider.clone());
                continue;
            }
            let Ok(spec) = self.spec_for(plugin) else {
                continue;
            };
            let runtime = Arc::new(PluginRuntime::new(spec));
            let settings = ProviderSettings {
                episode_servers: plugin.servers.clone(),
                supports_dub: plugin.supports_dub,
            };
            let provider = Arc::new(PluginProvider::new(
                &plugin.id,
                runtime.clone(),
                settings,
                &plugin.revision.to_string(),
            ));
            loaded.insert(
                plugin.id.clone(),
                Loaded {
                    revision: plugin.revision,
                    runtime,
                    provider: provider.clone(),
                },
            );
            providers.push(provider);
        }
        providers
    }

    /// The engine of a loaded plugin, for tools and tests.
    pub fn runtime(&self, id: &str) -> Option<Arc<PluginRuntime>> {
        let loaded = self.inner.loaded.lock().unwrap_or_else(|p| p.into_inner());
        loaded.get(id).map(|l| l.runtime.clone())
    }

    // ------------------------------------------------------------------ downloading

    fn check_link(&self, url: &Url) -> Result<(), String> {
        match url.scheme() {
            "https" => Ok(()),
            "http" if self.inner.options.allow_insecure => Ok(()),
            _ => Err(format!("only https links are accepted ({})", url.host_str().unwrap_or("no host"))),
        }
    }

    async fn download(&self, url: &Url, limit: usize, what: &str) -> Result<Vec<u8>, String> {
        self.check_link(url)?;
        let host = url.host_str().unwrap_or("the server").to_string();
        let client = http_client().map_err(|e| e.reason())?;
        let mut response = client
            .get(url.clone())
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .map_err(|e| crate::error::AppError::from(e).reason())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {} from {}", response.status().as_u16(), host));
        }
        let mut data: Vec<u8> = Vec::new();
        loop {
            let chunk = response
                .chunk()
                .await
                .map_err(|e| crate::error::AppError::from(e).reason())?;
            let Some(chunk) = chunk else { break };
            if data.len() + chunk.len() > limit {
                return Err(format!("the {} is larger than {} KB", what, limit / 1024));
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }

    /// Downloads the index behind a repository link: the first candidate address that answers
    /// with JSON. A single manifest counts as an index of one.
    async fn fetch_index(&self, link: &str) -> Result<(Url, Vec<Value>), String> {
        if link.trim().starts_with("file://") {
            return self.read_local_index(link.trim());
        }
        let candidates = index_candidates(link)?;
        let mut last_error = "the link could not be read".to_string();
        for candidate in candidates {
            let url = Url::parse(&candidate).map_err(|_| "That is not a valid link.".to_string())?;
            let data = match self.download(&url, MAX_INDEX_BYTES, "plugin list").await {
                Ok(data) => data,
                Err(reason) => {
                    last_error = reason;
                    continue;
                }
            };
            return match serde_json::from_slice::<Value>(&data) {
                Ok(Value::Array(entries)) => Ok((url, entries)),
                Ok(entry @ Value::Object(_)) => Ok((url, vec![entry])),
                Ok(_) => Err("the link does not hold a plugin list (a JSON array of manifests)".to_string()),
                Err(_) => Err("the link does not hold a plugin list (it is not JSON)".to_string()),
            };
        }
        Err(format!("the plugin list could not be downloaded: {}", last_error))
    }

    /// A repository in a local directory; see `DEV_REPOS_ENV`.
    fn read_local_index(&self, link: &str) -> Result<(Url, Vec<Value>), String> {
        if std::env::var(DEV_REPOS_ENV).map_or(true, |value| value != "1") {
            return Err("only https links are accepted".to_string());
        }
        let dir = Url::parse(link)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .ok_or_else(|| "that is not a local directory".to_string())?;
        let index_path = if dir.is_dir() { dir.join("index.json") } else { dir.clone() };
        let root = index_path.parent().map(Path::to_path_buf).unwrap_or(dir);
        let text = std::fs::read_to_string(&index_path).map_err(|e| format!("{}: {}", index_path.display(), e))?;
        let mut entries = match serde_json::from_str::<Value>(&text) {
            Ok(Value::Array(entries)) => entries,
            Ok(entry @ Value::Object(_)) => vec![entry],
            _ => return Err("the link does not hold a plugin list (it is not JSON)".to_string()),
        };
        for entry in entries.iter_mut() {
            let id = entry.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            let file = entry
                .get("payloadURI")
                .and_then(Value::as_str)
                .and_then(|uri| uri.split(['?', '#']).next())
                .and_then(|uri| uri.rsplit('/').next())
                .unwrap_or_default()
                .to_string();
            if !valid_plugin_id(&id) || file.is_empty() || file.contains("..") {
                continue;
            }
            if let Ok(source) = std::fs::read_to_string(root.join("src").join(&id).join(&file)) {
                if let Some(object) = entry.as_object_mut() {
                    object.insert("payload".to_string(), Value::String(source));
                }
            }
        }
        let index_url = Url::from_file_path(&index_path).map_err(|_| "that is not a local directory".to_string())?;
        Ok((index_url, entries))
    }

    /// Turns index entries into what to install. Entries of other extension types are left
    /// out silently; malformed ones are reported in the second value.
    async fn wanted_from(&self, index_url: &Url, entries: Vec<Value>) -> (Vec<Wanted>, Vec<String>) {
        let mut wanted: Vec<Wanted> = Vec::new();
        let mut problems: Vec<String> = Vec::new();
        for entry in entries {
            if wanted.len() >= MAX_PLUGINS_PER_REPO {
                problems.push(format!("only the first {} plugins are used", MAX_PLUGINS_PER_REPO));
                break;
            }
            if !entry.is_object() {
                problems.push("an entry is not a manifest object".to_string());
                continue;
            }
            if entry.get("type").and_then(Value::as_str) != Some(PLUGIN_TYPE) {
                continue;
            }
            let id = entry.get("id").and_then(Value::as_str).unwrap_or_default().trim().to_string();
            if !valid_plugin_id(&id) {
                problems.push(format!(
                    "'{}' is not a usable plugin id (letters, digits, '-' and '_' only)",
                    id.chars().take(40).collect::<String>()
                ));
                continue;
            }
            if wanted.iter().any(|known| known.id == id) {
                problems.push(format!("{} is listed twice", id));
                continue;
            }

            let has_payload = entry.get("payload").and_then(Value::as_str).map_or(false, |p| !p.trim().is_empty())
                || entry.get("payloadURI").and_then(Value::as_str).map_or(false, |p| !p.trim().is_empty());
            let has_version = entry.get("version").and_then(Value::as_str).map_or(false, |v| !v.trim().is_empty());
            let manifest_uri = entry.get("manifestURI").and_then(Value::as_str).unwrap_or_default().trim().to_string();

            // An entry that carries the code location and a version is the manifest; otherwise
            // its own manifest is fetched.
            if has_payload && (has_version || manifest_uri.is_empty()) {
                wanted.push(Wanted {
                    id,
                    manifest: entry,
                    base: index_url.clone(),
                });
                continue;
            }
            if manifest_uri.is_empty() {
                problems.push(format!("{} names neither a payload nor a manifest", id));
                continue;
            }
            let fetched = async {
                let url = index_url
                    .join(&manifest_uri)
                    .map_err(|_| "its manifest link is invalid".to_string())?;
                let data = self.download(&url, MAX_MANIFEST_BYTES, "manifest").await?;
                let manifest: Value =
                    serde_json::from_slice(&data).map_err(|_| "its manifest is not JSON".to_string())?;
                if manifest.get("id").and_then(Value::as_str).map(str::trim) != Some(id.as_str()) {
                    return Err("its manifest is for a different plugin id".to_string());
                }
                if manifest.get("type").and_then(Value::as_str) != Some(PLUGIN_TYPE) {
                    return Err("its manifest is not a streaming provider".to_string());
                }
                Ok::<_, String>((url, manifest))
            }
            .await;
            match fetched {
                Ok((url, manifest)) => wanted.push(Wanted {
                    id,
                    manifest,
                    base: url,
                }),
                Err(reason) => problems.push(format!("{}: {}", id, reason)),
            }
        }
        (wanted, problems)
    }

    /// Downloads (or takes inline) the payload a manifest names.
    async fn fetch_payload(&self, wanted: &Wanted) -> Result<(String, bool), String> {
        let manifest = &wanted.manifest;
        let language = text_field(manifest, "language").to_ascii_lowercase();
        let payload_uri = manifest.get("payloadURI").and_then(Value::as_str).unwrap_or_default().trim();
        let typescript = match language.as_str() {
            "typescript" => true,
            "javascript" | "" => false,
            other => return Err(format!("plugins written in '{}' are not supported", other)),
        };
        if let Some(inline) = manifest.get("payload").and_then(Value::as_str).filter(|p| !p.trim().is_empty()) {
            if inline.len() > MAX_PAYLOAD_BYTES {
                return Err(format!("the plugin code is larger than {} KB", MAX_PAYLOAD_BYTES / 1024));
            }
            return Ok((inline.to_string(), typescript));
        }
        if payload_uri.is_empty() {
            return Err("the manifest names no plugin code".to_string());
        }
        let url = wanted
            .base
            .join(payload_uri)
            .map_err(|_| "the link to the plugin code is invalid".to_string())?;
        let data = self.download(&url, MAX_PAYLOAD_BYTES, "plugin code").await?;
        let source = String::from_utf8(data).map_err(|_| "the plugin code is not UTF-8 text".to_string())?;
        if source.trim().is_empty() {
            return Err("the plugin code is empty".to_string());
        }
        Ok((source, typescript))
    }

    fn describe(&self, stored: &mut StoredPlugin, manifest: &Value) {
        stored.name = text_field(manifest, "name");
        stored.version = text_field(manifest, "version");
        stored.author = text_field(manifest, "author");
        stored.description = text_field(manifest, "description");
        stored.lang = text_field(manifest, "lang");
        stored.icon = manifest
            .get("icon")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|icon| icon.starts_with("https://") || icon.starts_with("data:image/"))
            .map(String::from);
        stored.user_config = UserConfig::from_manifest(manifest);
    }

    /// Brings the plugins of one repository in line with its index. Returns the notes for the
    /// repository's `error` (problems that left a plugin out or on its old version).
    async fn sync_repo(&self, state: &mut State, repo_url: &str, index_url: &Url, entries: Vec<Value>) -> Vec<String> {
        let (wanted, mut problems) = self.wanted_from(index_url, entries).await;

        // Plugins the index no longer lists go away.
        let listed: Vec<&str> = wanted.iter().map(|w| w.id.as_str()).collect();
        let gone: Vec<StoredPlugin> = state
            .plugins
            .iter()
            .filter(|p| p.repo_url == repo_url && !listed.contains(&p.id.as_str()))
            .cloned()
            .collect();
        for plugin in &gone {
            self.unload(&plugin.id);
            let _ = std::fs::remove_file(self.payload_path(plugin));
        }
        state.plugins.retain(|p| !(p.repo_url == repo_url && !listed.contains(&p.id.as_str())));

        for want in &wanted {
            let existing = state.plugins.iter().position(|p| p.id == want.id);
            if let Some(at) = existing {
                if state.plugins[at].repo_url != repo_url {
                    problems.push(format!(
                        "{} is already installed from {}",
                        want.id, state.plugins[at].repo_url
                    ));
                    continue;
                }
            }
            let version = text_field(&want.manifest, "version");
            let inline = want.manifest.get("payload").and_then(Value::as_str).filter(|p| !p.trim().is_empty());
            let up_to_date = existing.map_or(false, |at| {
                let plugin = &state.plugins[at];
                let stored = std::fs::read_to_string(self.payload_path(plugin)).ok();
                // Code that comes with the manifest is compared itself; otherwise the version decides.
                plugin.version == version
                    && stored.is_some()
                    && inline.map_or(true, |source| stored.as_deref() == Some(source))
            });
            if up_to_date {
                // Same version: only the description may have changed.
                let at = existing.unwrap_or_default();
                let mut plugin = state.plugins[at].clone();
                let config_before = plugin.user_config.clone();
                self.describe(&mut plugin, &want.manifest);
                if plugin.user_config != config_before {
                    // Its settings decide what code runs, so the plugin is started again.
                    plugin.revision = self.inner.revisions.fetch_add(1, Ordering::SeqCst) + 1;
                    self.unload(&plugin.id);
                    if plugin.enabled {
                        self.refresh_status(&mut plugin).await;
                    }
                }
                state.plugins[at] = plugin;
                continue;
            }

            let mut plugin = existing.map(|at| state.plugins[at].clone()).unwrap_or_else(|| StoredPlugin {
                id: want.id.clone(),
                repo_url: repo_url.to_string(),
                enabled: true,
                ..StoredPlugin::default()
            });
            match self.fetch_payload(want).await {
                Ok((source, typescript)) => {
                    if existing.is_some() {
                        let _ = std::fs::remove_file(self.payload_path(&plugin));
                    }
                    self.describe(&mut plugin, &want.manifest);
                    plugin.typescript = typescript;
                    plugin.revision = self.inner.revisions.fetch_add(1, Ordering::SeqCst) + 1;
                    self.unload(&plugin.id);
                    match Self::write_atomic(&self.payload_path(&plugin), source.as_bytes()) {
                        Ok(()) => {
                            if plugin.enabled {
                                self.refresh_status(&mut plugin).await;
                            } else {
                                plugin.error = None;
                            }
                        }
                        Err(reason) => plugin.error = Some(format!("the plugin could not be saved: {}", reason)),
                    }
                }
                Err(reason) if existing.is_some() => {
                    // The installed version keeps working.
                    problems.push(format!("{} could not be updated: {}", want.id, reason));
                    continue;
                }
                Err(reason) => {
                    self.describe(&mut plugin, &want.manifest);
                    plugin.error = Some(reason);
                }
            }
            match existing {
                Some(at) => state.plugins[at] = plugin,
                None => state.plugins.push(plugin),
            }
        }

        // Plugins keep the order of the index within their repository.
        let order: HashMap<&str, usize> = wanted.iter().enumerate().map(|(i, w)| (w.id.as_str(), i)).collect();
        let repo_order: HashMap<String, usize> =
            state.repos.iter().enumerate().map(|(i, r)| (r.url.clone(), i)).collect();
        state.plugins.sort_by_key(|p| {
            (
                repo_order.get(&p.repo_url).copied().unwrap_or(usize::MAX),
                if p.repo_url == repo_url { order.get(p.id.as_str()).copied().unwrap_or(usize::MAX) } else { 0 },
            )
        });
        problems
    }

    // ------------------------------------------------------------------ changes

    /// Adds a repository by link and installs every streaming plugin its index lists.
    pub async fn add_repo(&self, link: &str) -> Result<PluginRepo, String> {
        let link = link.trim().to_string();
        if link.is_empty() {
            return Err("Enter the link of a plugin repository.".to_string());
        }
        let _changing = self.inner.changes.lock().await;
        let mut state = self.state();
        if state.repos.iter().any(|repo| same_repo(&repo.url, &link)) {
            return Err("This repository has already been added.".to_string());
        }
        let (index_url, entries) = self
            .fetch_index(&link)
            .await
            .map_err(|reason| format!("Could not add the repository: {}.", reason))?;

        state.repos.push(StoredRepo {
            url: link.clone(),
            name: repo_name(&link),
            updated_at_ms: now_ms(),
            error: None,
        });
        let problems = self.sync_repo(&mut state, &link, &index_url, entries).await;
        if !state.plugins.iter().any(|p| p.repo_url == link) && problems.is_empty() {
            return Err("Could not add the repository: it lists no streaming plugins.".to_string());
        }
        if let Some(repo) = state.repos.iter_mut().find(|repo| repo.url == link) {
            repo.error = (!problems.is_empty()).then(|| problems.join("; "));
        }
        self.store(state);
        drop(_changing);
        self.notify();
        self.list_repos()
            .into_iter()
            .find(|repo| repo.url == link)
            .ok_or_else(|| "The repository could not be saved.".to_string())
    }

    pub async fn remove_repo(&self, link: &str) -> Result<(), String> {
        let link = link.trim();
        let _changing = self.inner.changes.lock().await;
        let mut state = self.state();
        let Some(url) = state
            .repos
            .iter()
            .find(|repo| repo.url == link)
            .or_else(|| state.repos.iter().find(|repo| same_repo(&repo.url, link)))
            .map(|repo| repo.url.clone())
        else {
            return Ok(());
        };
        for plugin in state.plugins.iter().filter(|p| p.repo_url == url) {
            self.unload(&plugin.id);
            let _ = std::fs::remove_file(self.payload_path(plugin));
        }
        state.plugins.retain(|p| p.repo_url != url);
        state.repos.retain(|repo| repo.url != url);
        self.store(state);
        drop(_changing);
        self.notify();
        Ok(())
    }

    /// Downloads every repository's index again and installs new versions. A repository that
    /// cannot be reached keeps its plugins and gets the reason as its `error`.
    pub async fn refresh_repos(&self) -> Vec<PluginRepo> {
        let _changing = self.inner.changes.lock().await;
        let mut state = self.state();
        let links: Vec<String> = state.repos.iter().map(|repo| repo.url.clone()).collect();
        for link in links {
            let outcome = match self.fetch_index(&link).await {
                Ok((index_url, entries)) => {
                    let problems = self.sync_repo(&mut state, &link, &index_url, entries).await;
                    Ok(problems)
                }
                Err(reason) => Err(reason),
            };
            if let Some(repo) = state.repos.iter_mut().find(|repo| repo.url == link) {
                match outcome {
                    Ok(problems) => {
                        repo.updated_at_ms = now_ms();
                        repo.error = (!problems.is_empty()).then(|| problems.join("; "));
                    }
                    Err(reason) => repo.error = Some(reason),
                }
            }
        }
        self.store(state);
        drop(_changing);
        self.notify();
        self.list_repos()
    }

    /// The plugin's own settings with the values in effect; no fields for a plugin without any.
    pub fn plugin_config(&self, id: &str) -> Result<PluginConfig, String> {
        let state = self.state();
        let plugin = state
            .plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| format!("No plugin named '{}' is installed.", id))?;
        let Some(config) = &plugin.user_config else {
            return Ok(PluginConfig {
                fields: Vec::new(),
                requires_config: false,
                configured: false,
            });
        };
        // Values saved for another configuration version are not in effect.
        let saved = plugin.saved_config.as_ref().filter(|saved| saved.version == config.version);
        let values: HashMap<String, String> = config.values(saved).into_iter().collect();
        Ok(PluginConfig {
            fields: config
                .fields
                .iter()
                .map(|field| PluginConfigField {
                    name: field.name.clone(),
                    label: field.label.clone(),
                    kind: field.kind.clone(),
                    default: field.default.clone(),
                    value: values.get(&field.name).cloned().unwrap_or_default(),
                    options: (!field.options.is_empty()).then(|| field.options.clone()),
                })
                .collect(),
            requires_config: config.requires_config,
            configured: saved.is_some(),
        })
    }

    /// Saves the user's values for the plugin's settings and starts the plugin again with them.
    /// Fields left out of `values` take their defaults; values of unknown fields are ignored.
    pub async fn set_plugin_config(&self, id: &str, values: &HashMap<String, Value>) -> Result<(), String> {
        let _changing = self.inner.changes.lock().await;
        let mut state = self.state();
        let Some(at) = state.plugins.iter().position(|p| p.id == id) else {
            return Err(format!("No plugin named '{}' is installed.", id));
        };
        let mut plugin = state.plugins[at].clone();
        let Some(config) = plugin.user_config.clone() else {
            return Err("This plugin has no settings.".to_string());
        };
        plugin.saved_config = Some(config.saved_from(values));
        plugin.revision = self.inner.revisions.fetch_add(1, Ordering::SeqCst) + 1;
        self.unload(id);
        if plugin.enabled {
            self.refresh_status(&mut plugin).await;
        }
        state.plugins[at] = plugin;
        self.store(state);
        drop(_changing);
        self.notify();
        Ok(())
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let _changing = self.inner.changes.lock().await;
        let mut state = self.state();
        let Some(at) = state.plugins.iter().position(|p| p.id == id) else {
            return Err(format!("No plugin named '{}' is installed.", id));
        };
        if state.plugins[at].enabled != enabled {
            let mut plugin = state.plugins[at].clone();
            plugin.enabled = enabled;
            if enabled {
                self.refresh_status(&mut plugin).await;
            } else {
                self.unload(id);
            }
            state.plugins[at] = plugin;
            self.store(state);
        }
        drop(_changing);
        self.notify();
        Ok(())
    }
}
