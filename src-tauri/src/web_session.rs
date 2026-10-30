//! Requests made by a real webview that belongs to the app.
//!
//! Some sites refuse the app's HTTP client with a Cloudflare block or an interactive check but
//! serve a browser. For those, a request is made by a hidden webview window that has the site
//! loaded: a same-origin `fetch()` inside it carries that webview's own session. When the site
//! wants the visitor to pass a check, the window is shown and the user passes it themselves;
//! afterwards the same webview keeps being used. Nothing here solves, simulates or skips a check,
//! and nothing the webview earned is used outside it: the page scripts never read cookies or
//! storage, response headers that set cookies are withheld, and the only thing remembered on
//! disk by this module is the list of origins that answered (the webview keeps its own cookies).
//!
//! The windows load remote pages. No capability lists them, so they cannot call the app's
//! commands, and answers are read back by evaluating a script in the page rather than by giving
//! the page anything to call. Whatever a page answers is untrusted data: it is only ever handed
//! to the caller as a response body.

use crate::error::AppError;
use async_trait::async_trait;
use base64::Engine as _;
use once_cell::sync::{Lazy, OnceCell};
use serde::Serialize;
use serde_json::Value;
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::webview::{DownloadEvent, PageLoadEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use url::Url;

/// Emitted to the main window when a site answers again after the user verified.
pub const EVENT_VERIFIED: &str = "site-verified";
/// Emitted to the main window when the user closed the window while the site still refuses.
pub const EVENT_CANCELLED: &str = "site-verification-cancelled";

const MAIN_WINDOW: &str = "main";
/// Folder under the app's local data directory that holds the site windows' cookies and storage.
const DATA_FOLDER: &str = "site-sessions";

/// How long a mirror gets to start answering before the next one is tried.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(7);
/// A page that has started arriving is used after this long even if it never finishes loading.
pub const COMMIT_GRACE: Duration = Duration::from_secs(3);
/// How often one mirror is reloaded after its page left the site or stopped answering.
pub const MAX_RELOADS: u32 = 2;
/// After every mirror failed, the site is left alone for this long.
pub const WALK_COOLDOWN: Duration = Duration::from_secs(15);
/// A freshly loaded page may still be passing the site's own automatic check; a refusal within
/// this time after loading is asked again before it is reported.
const SETTLE: Duration = Duration::from_secs(6);
const SETTLE_RETRY: Duration = Duration::from_millis(1200);
/// A refusal on a page loaded longer ago than this is asked again on a freshly loaded page
/// first: the page's standing with the site may simply have run out.
const STALE_PAGE: Duration = Duration::from_secs(60);
/// A refusal is remembered this long, so requests queued behind it fail at once.
const BLOCK_MEMORY: Duration = Duration::from_secs(5);
const STEP_INTERVAL: Duration = Duration::from_millis(100);
const POLL_INTERVAL: Duration = Duration::from_millis(80);
const POLL_REPLY_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_PAGE_MOVES: u32 = 3;
const REACH_TIMEOUT: Duration = Duration::from_millis(1500);
/// While the verification window is open, the refused request is repeated whenever its page
/// has loaded something new (a passed check reloads it), and this often otherwise.
const PROBE_INTERVAL: Duration = Duration::from_secs(10);
const PROBE_TICK: Duration = Duration::from_secs(1);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const PROBE_FOR: Duration = Duration::from_secs(5 * 60);
/// A hidden window nobody used for this long is closed; its cookies stay on disk.
const IDLE_CLOSE_AFTER: Duration = Duration::from_secs(10 * 60);
const REAP_INTERVAL: Duration = Duration::from_secs(60);

/// File under the app's local data directory that lists the origins whose session answered.
const KNOWN_ORIGINS_FILE: &str = "site-sessions.json";
/// A single-origin site's page may fail this many requests in a row before the page itself is
/// taken to be the problem.
const MAX_REQUEST_FAILURES: u32 = 3;

/// Largest response body, in bytes, handed back to the app.
pub const MAX_BODY_BYTES: usize = 6 * 1024 * 1024;
/// Largest request body sent through a page.
pub const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024;
pub const MAX_HEADER_CHARS: usize = 4096;
const MAX_HEADER_NAME_CHARS: usize = 128;
/// How many response headers are read back from the page.
pub const MAX_HEADERS: usize = 96;
/// Response headers never read back, should a page ever be able to see them.
pub const WITHHELD_HEADERS: [&str; 2] = ["set-cookie", "set-cookie2"];
/// Request methods a page may be asked to use.
pub const SESSION_METHODS: [&str; 6] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

/// A site that can be asked through the in-app browser.
#[derive(Debug)]
pub struct SiteSpec {
    /// The key providers of this site return from `AnimeProvider::site()`.
    pub key: &'static str,
    /// Name shown to the user.
    pub label: &'static str,
    /// `https://host` of every mirror, in order of preference.
    pub origins: &'static [&'static str],
}

/// How many sites made up at run time are kept at once. Past it, the least recently used one
/// that is not in use gives way.
const MAX_DYNAMIC_SITES: usize = 256;

struct DynamicSite {
    site: &'static SiteSpec,
    /// When it was last asked for, on the `SITE_USES` count.
    last_used: u64,
}

static DYNAMIC_SITES: Lazy<Mutex<HashMap<String, DynamicSite>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static SITE_USES: AtomicU64 = AtomicU64::new(0);

/// Which of `(key, last used, in use)` gives way to a new entry: the least recently used one
/// not in use. `None` when every entry is in use.
pub fn least_recently_used<K>(entries: impl IntoIterator<Item = (K, u64, bool)>) -> Option<K> {
    entries
        .into_iter()
        .filter(|(_, _, in_use)| !in_use)
        .min_by_key(|(_, last_used, _)| *last_used)
        .map(|(key, _, _)| key)
}

/// A site the user may be verifying, or that waits in the Verify list, is never evicted.
fn site_in_use(key: &str) -> bool {
    with_site(key, |state| state.visible || state.busy || state.blocked.is_some()).unwrap_or(false)
}

/// Drops what is kept for an evicted site: its hidden window and its remembered origin. Its
/// cookies stay in the webview's storage.
fn forget_site(site: &'static SiteSpec, origin: &str) {
    let window = {
        let mut sites = sites();
        // http and https of one host share a key; only this spec's own state goes.
        let owned = sites.get(site.key).map_or(false, |state| std::ptr::eq(state.spec, site));
        owned.then(|| sites.remove(site.key)).flatten().and_then(|mut state| state.window.take())
    };
    if let Some(window) = window {
        let _ = window.destroy();
    }
    if let Some(known) = KNOWN_ORIGINS.get() {
        known.remove(origin);
    }
}

/// The site made of the single origin `origin` (`scheme://host[:port]`), for callers that learn
/// at run time which site they talk to. Its key and label are the host. The same origin gives
/// the same site for as long as it is kept.
pub fn site_for_origin(origin: &str) -> Option<&'static SiteSpec> {
    let origin = normalized_origin(origin)?;
    let url = Url::parse(&origin).ok()?;
    let host = url.host_str()?;
    let use_count = SITE_USES.fetch_add(1, Ordering::SeqCst) + 1;
    let mut known = DYNAMIC_SITES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = known.get_mut(&origin) {
        entry.last_used = use_count;
        return Some(entry.site);
    }
    let mut evicted = None;
    if known.len() >= MAX_DYNAMIC_SITES {
        let candidates = known
            .iter()
            .map(|(origin, entry)| (origin.clone(), entry.last_used, site_in_use(entry.site.key)));
        let victim = least_recently_used(candidates)?;
        evicted = known.remove(&victim).map(|entry| (entry.site, victim));
    }
    let key = match url.port() {
        Some(port) => format!("{}:{}", host, port),
        None => host.to_string(),
    };
    // shortcut: an evicted spec stays allocated because callers may still hold it (a few bytes
    // per eviction); move to Arc<SiteSpec> if evictions ever become frequent.
    let key: &'static str = Box::leak(key.into_boxed_str());
    let origin_text: &'static str = Box::leak(origin.clone().into_boxed_str());
    let origins: &'static [&'static str] = Box::leak(vec![origin_text].into_boxed_slice());
    let site: &'static SiteSpec = Box::leak(Box::new(SiteSpec {
        key,
        label: key,
        origins,
    }));
    known.insert(origin, DynamicSite { site, last_used: use_count });
    drop(known);
    if let Some((old, old_origin)) = evicted {
        forget_site(old, &old_origin);
    }
    Some(site)
}

/// A same-origin request to make inside the site's page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WebRequest {
    /// One of `SESSION_METHODS`.
    pub method: String,
    /// Path with optional query on the site; see `valid_request_path`.
    pub path: String,
    /// Headers a page may set on its own request; the browser supplies the rest.
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// A path on the site to name as the referrer instead of the loaded page.
    pub referrer: Option<String>,
}

impl WebRequest {
    pub fn get(path: &str) -> Self {
        Self {
            method: "GET".to_string(),
            path: path.to_string(),
            ..Self::default()
        }
    }

    /// Why this request cannot be made inside a page, if it cannot.
    pub fn problem(&self) -> Option<&'static str> {
        if !SESSION_METHODS.contains(&self.method.as_str()) {
            return Some("request method");
        }
        if !valid_request_path(&self.path) {
            return Some("path");
        }
        if self.referrer.as_deref().map_or(false, |path| !valid_request_path(path)) {
            return Some("referrer");
        }
        match &self.body {
            Some(_) if matches!(self.method.as_str(), "GET" | "HEAD") => Some("body on a request without one"),
            Some(body) if body.len() > MAX_REQUEST_BODY_BYTES => Some("request body size"),
            _ => None,
        }
    }
}

/// What a same-origin request inside the site's page answered.
#[derive(Debug, Clone, PartialEq)]
pub struct WebResponse {
    pub status: u16,
    /// The mirror that answered.
    pub origin: String,
    /// The address that answered, after any redirects the page followed.
    pub url: String,
    pub redirected: bool,
    /// The response headers the page could read, by lower-case name; repeated ones joined.
    pub headers: HashMap<String, String>,
    /// The body exactly as the site sent it.
    pub body: Vec<u8>,
}

impl WebResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    /// The start of the body as text, for telling a check page from an answer.
    pub fn head_text(&self) -> String {
        String::from_utf8_lossy(&self.body[..self.body.len().min(BARRIER_SCAN_BYTES)]).into_owned()
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// How far a lookup may go to reach a site through the in-app browser. Loading a third-party
/// site in a hidden browser is only worth it when the user asked for that site or nothing else
/// delivered, so a lookup that merely includes the site among many must not start one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionUse {
    /// Only a session that answers, or that answered before (a site the user already got
    /// through), may be asked; a site never reached through the browser is not loaded.
    Existing,
    /// The site's window may be created and its page loaded.
    Create,
}

/// Where a site's session stands in this process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// No window, or one that has not answered a request yet.
    Cold,
    /// A window exists and the site answered through it.
    Warm,
    /// No window in this process, but the site answered through its session in an earlier run;
    /// the cookies that made it answer are still in the webview's storage.
    Resumable,
    /// The site last refused the session from this origin and waits for the user to verify.
    Refused(String),
}

/// What a provider does once the plain client was refused.
#[derive(Debug, Clone)]
pub enum SessionChoice {
    /// Ask through the session.
    Ask,
    /// Leave it at the plain client's refusal.
    Keep,
    /// Report that the site waits for the user, without asking anything.
    Refused(AppError),
}

/// Applies `SessionUse` to the state of the site's session.
pub fn choose(session: &dyn SiteSession, site: &'static SiteSpec, access: SessionUse) -> SessionChoice {
    if !session.available() {
        return SessionChoice::Keep;
    }
    match (access, session.state(site)) {
        (SessionUse::Create, _)
        | (SessionUse::Existing, SessionState::Warm)
        | (SessionUse::Existing, SessionState::Resumable) => SessionChoice::Ask,
        (SessionUse::Existing, SessionState::Cold) => SessionChoice::Keep,
        (SessionUse::Existing, SessionState::Refused(origin)) => {
            SessionChoice::Refused(verification_required(site, &origin))
        }
    }
}

/// The way providers reach the in-app browser; tests substitute their own.
#[async_trait]
pub trait SiteSession: Send + Sync {
    /// False when no request can be made at all (no app handle: unit tests, or before startup).
    fn available(&self) -> bool;

    fn state(&self, site: &'static SiteSpec) -> SessionState;

    /// The request, made on the site inside its page with the page's session.
    async fn fetch(
        &self,
        site: &'static SiteSpec,
        request: &WebRequest,
        timeout: Duration,
    ) -> Result<WebResponse, AppError>;
}

/// The session backed by the app's own webview windows.
pub struct AppSession;

#[async_trait]
impl SiteSession for AppSession {
    fn available(&self) -> bool {
        is_available()
    }

    fn state(&self, site: &'static SiteSpec) -> SessionState {
        session_state(site)
    }

    async fn fetch(
        &self,
        site: &'static SiteSpec,
        request: &WebRequest,
        timeout: Duration,
    ) -> Result<WebResponse, AppError> {
        fetch(site, request, timeout).await
    }
}

pub fn app_session() -> Arc<dyn SiteSession> {
    Arc::new(AppSession)
}

// ---------------------------------------------------------------- recognising a refusal

/// A page a site answers with instead of the content that was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Barrier {
    /// A check the visitor can pass.
    Challenge,
    /// A refusal that offers nothing to pass.
    Block,
}

const BLOCK_MARKERS: [&str; 4] = [
    "Attention Required! | Cloudflare",
    "Sorry, you have been blocked",
    "cf-error-details",
    "error code: 1020",
];

const CHALLENGE_MARKERS: [&str; 7] = [
    "Just a moment",
    "_cf_chl_opt",
    "cf-chl",
    "challenge-platform",
    "cf-turnstile",
    "Checking your browser",
    "DDoS-Guard",
];

/// Only the start of a body is searched; the markers sit in the head of these pages.
const BARRIER_SCAN_BYTES: usize = 32 * 1024;

fn head_of(body: &str, limit: usize) -> &str {
    if body.len() <= limit {
        return body;
    }
    let mut end = limit;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    &body[..end]
}

/// Tells a Cloudflare or DDoS-Guard check or block page from a real answer. `headers` are keyed
/// by lower-case name. A page of the site itself that merely loads the protection's scripts is
/// not a barrier, so the body only counts together with the status such pages are sent with.
pub fn detect_barrier(status: u16, headers: &HashMap<String, String>, body: &str) -> Option<Barrier> {
    let mitigated = headers
        .get("cf-mitigated")
        .map_or(false, |value| value.to_ascii_lowercase().contains("challenge"));
    if mitigated {
        return Some(Barrier::Challenge);
    }
    if !matches!(status, 403 | 503) {
        return None;
    }
    let head = head_of(body, BARRIER_SCAN_BYTES);
    // The block page loads the challenge scripts too, so it is looked for first.
    if BLOCK_MARKERS.iter().any(|marker| head.contains(marker)) {
        Some(Barrier::Block)
    } else if CHALLENGE_MARKERS.iter().any(|marker| head.contains(marker)) {
        Some(Barrier::Challenge)
    } else {
        None
    }
}

/// What marks the document a window shows as a check or block page. A document has no status
/// to go by, so only what the site's own pages do not carry counts: the titles these pages
/// have, and markup only they contain.
pub const BARRIER_TITLES: [(&str, Barrier); 4] = [
    ("Just a moment", Barrier::Challenge),
    ("Checking your browser", Barrier::Challenge),
    ("DDoS-Guard", Barrier::Challenge),
    ("Attention Required! | Cloudflare", Barrier::Block),
];
pub const BARRIER_MARKUP: [(&str, Barrier); 3] = [
    ("_cf_chl_opt", Barrier::Challenge),
    ("cf-error-details", Barrier::Block),
    ("Sorry, you have been blocked", Barrier::Block),
];

/// Tells whether a document with this title and markup is a check or block page.
pub fn document_barrier(title: &str, markup: &str) -> Option<Barrier> {
    let markup = head_of(markup, BARRIER_SCAN_BYTES);
    let found: Vec<Barrier> = BARRIER_TITLES
        .iter()
        .filter(|(marker, _)| title.contains(marker))
        .chain(BARRIER_MARKUP.iter().filter(|(marker, _)| markup.contains(marker)))
        .map(|(_, kind)| *kind)
        .collect();
    if found.contains(&Barrier::Block) {
        Some(Barrier::Block)
    } else {
        found.first().copied()
    }
}

/// What the document in a site's window is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentState {
    /// A check or block page of the site's protection.
    Barrier(Barrier),
    /// A document of the site that is no such page.
    Site,
    /// A document of another origin.
    Elsewhere,
    /// It could not be told (the window is between documents, for example).
    Unreadable,
}

// ---------------------------------------------------------------- scripts run in the page

/// A JavaScript string literal for `value`. Quotes, backslashes, line breaks, markup characters
/// and control characters are escaped, so the value cannot end the literal or a script element.
pub fn js_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' | '>' | '&' | '\'' | '`' | '\u{2028}' | '\u{2029}' | '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// True for a path (with optional query) that can only address the origin it is appended to.
pub fn valid_request_path(path: &str) -> bool {
    let mut chars = path.chars();
    chars.next() == Some('/')
        && !matches!(chars.next(), Some('/') | Some('\\'))
        && !path.chars().any(|c| c.is_control() || c == '#')
}

fn new_request_key() -> String {
    let part = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(std::process::id() as u128);
        hasher.finish()
    };
    format!("__anix_{:016x}{:016x}", part(), part())
}

/// The script that makes the request. It keeps its state in one non-enumerable property of the
/// page's `window`, named by the unguessable `key`, where `collect_script` finds the answer. If
/// the page is not on `origin` any more it says so instead of asking. The answer's body is
/// handed over base64-encoded, so it arrives byte for byte whatever it holds. The script reads
/// the response and nothing else of the page: no cookies, no storage.
pub fn request_script(key: &str, origin: &str, request: &WebRequest) -> String {
    let header_pairs = request
        .headers
        .iter()
        .map(|(name, value)| format!("[{},{}]", js_string(name), js_string(value)))
        .collect::<Vec<_>>()
        .join(",");
    let withheld = WITHHELD_HEADERS
        .iter()
        .map(|name| js_string(name))
        .collect::<Vec<_>>()
        .join(",");
    let referrer = match &request.referrer {
        Some(path) => format!("q.referrer=o+{};", js_string(path)),
        None => String::new(),
    };
    let body = match &request.body {
        Some(bytes) => format!(
            "var d=atob({}),u=new Uint8Array(d.length);for(var i=0;i<d.length;i++)u[i]=d.charCodeAt(i);q.body=u;",
            js_string(&base64::engine::general_purpose::STANDARD.encode(bytes))
        ),
        None => String::new(),
    };
    [
        "(function(){var k=",
        &js_string(key),
        ",o=",
        &js_string(origin),
        ",s={done:false};",
        "try{Object.defineProperty(window,k,{value:s,configurable:true})}catch(e){return}",
        "function finish(r){s.result=r;s.done=true}",
        "if(location.origin!==o){finish({elsewhere:String(location.origin)});return}",
        "try{var c=new AbortController();s.abort=function(){c.abort()};",
        "var h={};[",
        &header_pairs,
        "].forEach(function(p){h[p[0]]=p[1]});",
        "var q={method:",
        &js_string(&request.method),
        ",headers:h,credentials:\"include\",cache:\"no-store\",redirect:\"follow\",signal:c.signal};",
        &referrer,
        &body,
        "fetch(o+",
        &js_string(&request.path),
        ",q).then(function(r){return r.arrayBuffer().then(function(a){",
        "var w=[",
        &withheld,
        "],g=[];try{r.headers.forEach(function(v,n){n=String(n).toLowerCase();",
        "if(g.length<",
        &MAX_HEADERS.to_string(),
        "&&w.indexOf(n)<0)g.push([n.slice(0,",
        &MAX_HEADER_NAME_CHARS.to_string(),
        "),String(v).slice(0,",
        &MAX_HEADER_CHARS.to_string(),
        ")])})}catch(e){}",
        "var b=new Uint8Array(a);if(b.length>",
        &MAX_BODY_BYTES.to_string(),
        "){finish({status:r.status,truncated:true});return}",
        "var t=\"\";for(var i=0;i<b.length;i+=32768)t+=String.fromCharCode.apply(null,b.subarray(i,i+32768));",
        "finish({status:r.status,url:String(r.url||\"\").slice(0,4096),redirected:!!r.redirected,headers:g,body:btoa(t)})})})",
        ".catch(function(e){finish({error:String(e).slice(0,300)})})",
        "}catch(e){finish({error:String(e).slice(0,300)})}})();",
    ]
    .concat()
}

/// Evaluates to `null` while the request is running, `"gone"` when the page that was asked no
/// longer exists, and the answer as JSON text once there is one (which also removes it).
pub fn collect_script(key: &str) -> String {
    [
        "(function(){var k=",
        &js_string(key),
        ",s=window[k];if(!s)return \"gone\";if(!s.done)return null;",
        "try{delete window[k]}catch(e){}",
        "try{var t=JSON.stringify(s.result);return t.length>",
        &(MAX_BODY_BYTES * 2).to_string(),
        "?JSON.stringify({truncated:true}):t}catch(e){return JSON.stringify({error:\"unreadable answer\"})}})()",
    ]
    .concat()
}

/// The script that says what document the window is on: its origin, and whether its title or
/// the start of its markup carries the marks of a check or block page. The comparison is made
/// in the page; none of the document's content is handed over.
pub fn document_script() -> String {
    let list = |markers: &[(&str, Barrier)], kind: Barrier| {
        markers
            .iter()
            .filter(|(_, marked)| *marked == kind)
            .map(|(marker, _)| js_string(marker))
            .collect::<Vec<_>>()
            .join(",")
    };
    [
        "(function(){try{var t=String(document.title||\"\"),h=\"\";",
        "try{h=String(document.documentElement.outerHTML).slice(0,",
        &BARRIER_SCAN_BYTES.to_string(),
        ")}catch(e){}",
        "function has(x,l){for(var i=0;i<l.length;i++){if(x.indexOf(l[i])>=0)return true}return false}",
        "return JSON.stringify({origin:String(location.origin),",
        "challenge:has(t,[",
        &list(&BARRIER_TITLES, Barrier::Challenge),
        "])||has(h,[",
        &list(&BARRIER_MARKUP, Barrier::Challenge),
        "]),block:has(t,[",
        &list(&BARRIER_TITLES, Barrier::Block),
        "])||has(h,[",
        &list(&BARRIER_MARKUP, Barrier::Block),
        "])})}catch(e){return JSON.stringify({unreadable:true})}})()",
    ]
    .concat()
}

/// Reads what `document_script` evaluated to, for a window that should be on `origin`.
pub fn parse_document_state(raw: &str, origin: &str) -> DocumentState {
    let Ok(value) = serde_json::from_str::<Value>(raw.trim()) else {
        return DocumentState::Unreadable;
    };
    let value = match value {
        Value::String(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => value,
            Err(_) => return DocumentState::Unreadable,
        },
        other => other,
    };
    let Some(page_origin) = value.get("origin").and_then(Value::as_str) else {
        return DocumentState::Unreadable;
    };
    if page_origin != origin {
        return DocumentState::Elsewhere;
    }
    let flag = |name: &str| value.get(name).and_then(Value::as_bool).unwrap_or(false);
    if flag("block") {
        DocumentState::Barrier(Barrier::Block)
    } else if flag("challenge") {
        DocumentState::Barrier(Barrier::Challenge)
    } else {
        DocumentState::Site
    }
}

/// Aborts a request nobody waits for any more and removes its state from the page.
pub fn cancel_script(key: &str) -> String {
    [
        "(function(){var k=",
        &js_string(key),
        ",s=window[k];if(!s)return;try{if(s.abort)s.abort()}catch(e){}",
        "try{delete window[k]}catch(e){}})();",
    ]
    .concat()
}

/// What evaluating `collect_script` gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Collected {
    /// The evaluation produced nothing usable (the page is busy navigating, for example).
    NoAnswer,
    Pending,
    Gone,
    Response {
        status: u16,
        /// As the page reported it; not checked against the site here.
        url: String,
        redirected: bool,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    },
    TooLarge,
    /// The request failed inside the page, usually because the host could not be reached.
    Failed(String),
    /// The page is on another origin than the one that was to be asked.
    Elsewhere(String),
}

fn clipped(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Reads the JSON the webview serialised the script's value to. Everything in it comes from the
/// page, so every field is checked and anything unexpected is a failure, never a response.
pub fn parse_collected(raw: &str) -> Collected {
    let Ok(value) = serde_json::from_str::<Value>(raw.trim()) else {
        return Collected::NoAnswer;
    };
    let result = match value {
        Value::Null => return Collected::Pending,
        Value::String(text) if text == "gone" => return Collected::Gone,
        Value::String(text) => match serde_json::from_str::<Value>(&text) {
            Ok(result) => result,
            Err(_) => return Collected::Failed("unreadable answer".to_string()),
        },
        other => other,
    };
    if !result.is_object() {
        return Collected::Failed("unreadable answer".to_string());
    }
    if let Some(origin) = result.get("elsewhere").and_then(Value::as_str) {
        return Collected::Elsewhere(clipped(origin, 200));
    }
    if let Some(error) = result.get("error").and_then(Value::as_str) {
        return Collected::Failed(clipped(error, 200));
    }
    if result.get("truncated").and_then(Value::as_bool) == Some(true) {
        return Collected::TooLarge;
    }

    let status = result
        .get("status")
        .and_then(Value::as_u64)
        .filter(|status| (100..=599).contains(status));
    let body = result.get("body").and_then(Value::as_str);
    let (Some(status), Some(body)) = (status, body) else {
        return Collected::Failed("unreadable answer".to_string());
    };
    if body.len() > MAX_BODY_BYTES / 3 * 4 + 4 {
        return Collected::TooLarge;
    }
    let Ok(body) = base64::engine::general_purpose::STANDARD.decode(body) else {
        return Collected::Failed("unreadable answer".to_string());
    };
    let mut headers: HashMap<String, String> = HashMap::new();
    let pairs = result.get("headers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    for pair in pairs.iter().take(MAX_HEADERS) {
        let (Some(name), Some(value)) = (
            pair.get(0).and_then(Value::as_str),
            pair.get(1).and_then(Value::as_str),
        ) else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let is_name = !name.is_empty()
            && name.len() <= MAX_HEADER_NAME_CHARS
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
        if !is_name || WITHHELD_HEADERS.contains(&name.as_str()) {
            continue;
        }
        let value = clipped(value, MAX_HEADER_CHARS);
        match headers.get_mut(&name) {
            Some(known) => {
                known.push_str(", ");
                known.push_str(&value);
            }
            None => {
                headers.insert(name, value);
            }
        }
    }
    Collected::Response {
        status: status as u16,
        url: clipped(result.get("url").and_then(Value::as_str).unwrap_or_default(), 4096),
        redirected: result.get("redirected").and_then(Value::as_bool).unwrap_or(false),
        headers,
        body,
    }
}

/// The address a page says answered, when it is one on `origin`; the address asked otherwise.
/// A page cannot make an answer look as if it came from elsewhere.
pub fn answered_url(origin: &str, asked_path: &str, reported: &str) -> String {
    Url::parse(reported)
        .ok()
        .filter(|url| origin_of(url).as_deref() == Some(origin))
        .map(|url| url.to_string())
        .unwrap_or_else(|| format!("{}{}", origin, asked_path))
}

// ---------------------------------------------------------------- origins that answered before

/// The origins whose session answered after the plain client was refused, kept across restarts
/// so that such a site is asked through its session again without the user doing anything.
/// Origins only: what makes the site answer stays in the webview's own storage.
pub struct KnownOrigins {
    path: Option<PathBuf>,
    /// Oldest first, so a full list gives up the site verified longest ago.
    origins: Mutex<Vec<String>>,
}

impl KnownOrigins {
    /// Reads the list kept in `path`; a missing or unreadable file is an empty list.
    pub fn open(path: Option<PathBuf>) -> Self {
        let stored = path
            .as_deref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
            .unwrap_or_default();
        let mut origins: Vec<String> = Vec::new();
        for origin in stored.iter().filter_map(|origin| normalized_origin(origin)) {
            if !origins.contains(&origin) {
                origins.push(origin);
            }
        }
        // The newest ones are kept.
        origins.drain(..origins.len().saturating_sub(MAX_DYNAMIC_SITES));
        Self {
            path,
            origins: Mutex::new(origins),
        }
    }

    pub fn contains(&self, origin: &str) -> bool {
        self.origins.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|known| known == origin)
    }

    fn save(&self, origins: &[String]) {
        let Some(path) = &self.path else {
            return;
        };
        let written = serde_json::to_vec_pretty(origins).map_err(|e| e.to_string()).and_then(|data| {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(path, data).map_err(|e| e.to_string())
        });
        if let Err(e) = written {
            tracing::warn!("could not save {}: {}", path.display(), e);
        }
    }

    /// Notes that the session of `origin` answered.
    pub fn add(&self, origin: &str) {
        let Some(origin) = normalized_origin(origin) else {
            return;
        };
        let mut origins = self.origins.lock().unwrap_or_else(|p| p.into_inner());
        if origins.contains(&origin) {
            return;
        }
        if origins.len() >= MAX_DYNAMIC_SITES {
            origins.remove(0);
        }
        origins.push(origin);
        self.save(&origins);
    }

    /// Notes that the session of `origin` is refused again.
    pub fn remove(&self, origin: &str) {
        let mut origins = self.origins.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(at) = origins.iter().position(|known| known == origin) {
            origins.remove(at);
            self.save(&origins);
        }
    }
}

static KNOWN_ORIGINS: OnceCell<KnownOrigins> = OnceCell::new();

fn answered_before(site: &SiteSpec) -> bool {
    KNOWN_ORIGINS.get().map_or(false, |known| {
        site.origins
            .iter()
            .any(|origin| normalized_origin(origin).map_or(false, |origin| known.contains(&origin)))
    })
}

// ---------------------------------------------------------------- naming a site

type Labeller = Arc<dyn Fn(&SiteSpec) -> Option<String> + Send + Sync>;

static LABELLER: OnceCell<Labeller> = OnceCell::new();

/// Lets the app name a site better than by its host (by the plugins that use it). Set once.
pub fn set_labeller(labeller: Labeller) {
    let _ = LABELLER.set(labeller);
}

/// The name of a site as the user is shown it.
pub fn display_label(site: &SiteSpec) -> String {
    LABELLER
        .get()
        .and_then(|labeller| labeller(site))
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| site.label.to_string())
}

/// A site named by who uses it: `"AnimeKai (anikai.to)"`, or the host alone when nobody does.
pub fn label_with_owners(host: &str, owners: &[String]) -> String {
    if owners.is_empty() {
        host.to_string()
    } else {
        format!("{} ({})", owners.join(", "), host)
    }
}

// ---------------------------------------------------------------- choosing a mirror

fn normalized_origin(origin: &str) -> Option<String> {
    Url::parse(origin).ok().and_then(|url| origin_of(&url))
}

/// `scheme://host[:port]` of an http(s) URL.
pub fn origin_of(url: &Url) -> Option<String> {
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => Some(url.origin().ascii_serialization()),
        _ => None,
    }
}

fn host_of(origin: &str) -> String {
    Url::parse(origin)
        .ok()
        .and_then(|url| url.host_str().map(String::from))
        .unwrap_or_else(|| origin.to_string())
}

/// The order mirrors are tried in: the one that answered last first, and mirrors that did not
/// accept a connection last (they are still tried, since the webview may reach them differently).
pub fn order_origins(count: usize, remembered: Option<usize>, reachable: &[bool]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).collect();
    if let Some(idx) = remembered.filter(|idx| *idx < count) {
        order.retain(|i| *i != idx);
        order.insert(0, idx);
    }
    order.sort_by_key(|idx| !reachable.get(*idx).copied().unwrap_or(true));
    order
}

/// What the caller has to do next to get a page of the site into the window.
#[derive(Debug, Clone, PartialEq)]
pub enum WalkStep {
    /// No mirror is being tried: decide the order and call `Walk::begin`.
    Order,
    /// Load this origin in the window.
    Load(String),
    Wait,
    /// A page of this origin is loaded and has been since `since`.
    Ready { origin: String, since: Instant },
    /// Every mirror failed a moment ago.
    Failed(String),
}

/// Which mirror of a site the window is on and how far its page has loaded. It holds no window,
/// only what page-load events and request outcomes say, so it can be exercised without one.
#[derive(Debug)]
pub struct Walk {
    origins: Vec<String>,
    /// Mirrors still to try, the one being loaded or used first.
    queue: VecDeque<usize>,
    navigated_at: Option<Instant>,
    committed_at: Option<Instant>,
    ready_at: Option<Instant>,
    finished: bool,
    drifted: bool,
    reloads: u32,
    remembered: Option<usize>,
    failed_until: Option<Instant>,
    failure: Option<String>,
    page_origin: Option<String>,
}

impl Walk {
    pub fn new(origins: &[&str]) -> Self {
        Self {
            origins: origins
                .iter()
                .map(|origin| normalized_origin(origin).unwrap_or_else(|| origin.to_string()))
                .collect(),
            queue: VecDeque::new(),
            navigated_at: None,
            committed_at: None,
            ready_at: None,
            finished: false,
            drifted: false,
            reloads: 0,
            remembered: None,
            failed_until: None,
            failure: None,
            page_origin: None,
        }
    }

    pub fn origins(&self) -> &[String] {
        &self.origins
    }

    pub fn remembered(&self) -> Option<usize> {
        self.remembered
    }

    /// The mirror being loaded or used.
    pub fn current(&self) -> Option<&str> {
        self.queue.front().map(|idx| self.origins[*idx].as_str())
    }

    /// Origin of the document the window shows, whatever site it belongs to.
    pub fn page_origin(&self) -> Option<&str> {
        self.page_origin.as_deref()
    }

    fn is_current(&self, origin: &str) -> bool {
        self.current() == Some(origin)
    }

    fn clear_page(&mut self) {
        self.navigated_at = None;
        self.committed_at = None;
        self.ready_at = None;
        self.finished = false;
        self.drifted = false;
    }

    fn mark_loading(&mut self, now: Instant) {
        self.clear_page();
        self.navigated_at = Some(now);
    }

    /// Starts trying the mirrors in the given order.
    pub fn begin(&mut self, order: Vec<usize>) {
        self.queue = order.into_iter().filter(|idx| *idx < self.origins.len()).collect();
        self.failed_until = None;
        self.reloads = 0;
        self.clear_page();
    }

    pub fn step(&mut self, now: Instant) -> WalkStep {
        loop {
            if let Some(until) = self.failed_until {
                if now < until {
                    return WalkStep::Failed(self.failure.clone().unwrap_or_default());
                }
                self.failed_until = None;
            }
            let Some(&idx) = self.queue.front() else {
                return WalkStep::Order;
            };
            let origin = self.origins[idx].clone();
            let Some(navigated_at) = self.navigated_at else {
                self.mark_loading(now);
                return WalkStep::Load(origin);
            };
            if self.drifted {
                if self.reloads >= MAX_RELOADS {
                    self.fail_current("kept leaving the site", now);
                    continue;
                }
                self.reloads += 1;
                self.mark_loading(now);
                return WalkStep::Load(origin);
            }
            match self.committed_at {
                Some(committed_at) => {
                    let settled = self.finished
                        || now.saturating_duration_since(committed_at) >= COMMIT_GRACE;
                    if self.ready_at.is_none() && settled {
                        self.ready_at = Some(now);
                    }
                    return match self.ready_at {
                        Some(since) => WalkStep::Ready { origin, since },
                        None => WalkStep::Wait,
                    };
                }
                None if now.saturating_duration_since(navigated_at) >= LOAD_TIMEOUT => {
                    self.fail_current("did not load", now);
                }
                None => return WalkStep::Wait,
            }
        }
    }

    /// A document was committed (`finished == false`) or finished loading in the window.
    pub fn on_page(&mut self, origin: &str, finished: bool, now: Instant) {
        self.page_origin = Some(origin.to_string());
        let Some(&current) = self.queue.front() else {
            return;
        };
        let Some(landed) = self.origins.iter().position(|known| known == origin) else {
            // Another site altogether: an advertisement, or a stop on the way through a check.
            self.drifted = true;
            self.committed_at = None;
            self.ready_at = None;
            self.finished = false;
            return;
        };
        if landed != current {
            // The mirror redirected to another one, which is then the one in use.
            self.queue.retain(|idx| *idx != landed && *idx != current);
            self.queue.push_front(landed);
            self.committed_at = None;
            self.ready_at = None;
        }
        self.drifted = false;
        if self.navigated_at.is_none() {
            self.navigated_at = Some(now);
        }
        if !finished {
            self.committed_at = Some(now);
            self.ready_at = None;
            self.finished = false;
        } else if self.committed_at.is_some() {
            // A finish without a commit belongs to a load that was abandoned.
            self.finished = true;
            self.ready_at.get_or_insert(now);
        }
    }

    fn fail_current(&mut self, reason: &str, now: Instant) {
        let Some(idx) = self.queue.pop_front() else {
            return;
        };
        if self.remembered == Some(idx) {
            self.remembered = None;
        }
        self.failure = Some(format!(
            "{} {} in the in-app browser",
            host_of(&self.origins[idx]),
            reason
        ));
        self.reloads = 0;
        self.clear_page();
        if self.queue.is_empty() {
            self.failed_until = Some(now + WALK_COOLDOWN);
        }
    }

    /// A request on `origin` failed in a way that says the mirror is not usable.
    pub fn fail_origin(&mut self, origin: &str, reason: &str, now: Instant) {
        if self.is_current(origin) {
            self.fail_current(reason, now);
        }
    }

    /// Gives up on every mirror for a while, for a failure that is not about one of them.
    pub fn fail_all(&mut self, reason: &str, now: Instant) {
        self.queue.clear();
        self.failure = Some(reason.to_string());
        self.failed_until = Some(now + WALK_COOLDOWN);
        self.clear_page();
    }

    /// The window turned out to show something else than `origin`.
    pub fn mark_drifted(&mut self, origin: &str) {
        if self.is_current(origin) && self.navigated_at.is_some() {
            self.drifted = true;
            self.committed_at = None;
            self.ready_at = None;
            self.finished = false;
        }
    }

    /// The loaded page of `origin` stopped answering: load it again, a limited number of times.
    pub fn request_reload(&mut self, origin: &str, now: Instant) {
        if !self.is_current(origin) || self.ready_at.is_none() {
            return;
        }
        if self.reloads >= MAX_RELOADS {
            self.fail_current("stopped answering", now);
        } else {
            self.reloads += 1;
            self.clear_page();
        }
    }

    /// Loads the page of `origin` again without holding it against the mirror.
    pub fn refresh(&mut self, origin: &str) {
        if self.is_current(origin) && self.ready_at.is_some() {
            self.clear_page();
        }
    }

    /// A request on `origin` was answered: it is the mirror to start with from now on.
    pub fn confirm(&mut self, origin: &str) {
        if self.is_current(origin) {
            self.remembered = self.queue.front().copied();
            self.reloads = 0;
        }
    }

    /// The window is being sent to `origin` by the caller rather than by `step`.
    pub fn focus(&mut self, origin: &str, now: Instant) {
        let Some(idx) = self.origins.iter().position(|known| known == origin) else {
            return;
        };
        self.queue.retain(|i| *i != idx);
        self.queue.push_front(idx);
        self.failed_until = None;
        self.reloads = 0;
        self.mark_loading(now);
    }

    /// The window is gone; the mirror in use is kept and loaded again on the next step.
    pub fn reset_page(&mut self) {
        self.clear_page();
        self.page_origin = None;
    }
}

// ---------------------------------------------------------------- the windows

/// The request that was last refused, repeated to see whether the refusal has gone.
#[derive(Clone)]
struct Blocked {
    origin: String,
    path: String,
    headers: Vec<(String, String)>,
    at: Instant,
    /// Only the app's plain client met the check; the in-app browser was not asked yet.
    plain: bool,
}

struct SiteState {
    spec: &'static SiteSpec,
    walk: Walk,
    window: Option<WebviewWindow>,
    /// A window is being created or the mirror order is being worked out.
    busy: bool,
    /// The window is shown for the user to verify.
    visible: bool,
    blocked: Option<Blocked>,
    /// Requests in a row that failed inside the page without an answer.
    request_failures: u32,
    /// Counts the page-load events of the window.
    page_events: u64,
    last_used: Instant,
}

impl SiteState {
    fn new(spec: &'static SiteSpec) -> Self {
        Self {
            spec,
            walk: Walk::new(spec.origins),
            window: None,
            busy: false,
            visible: false,
            blocked: None,
            request_failures: 0,
            page_events: 0,
            last_used: Instant::now(),
        }
    }
}

static APP: OnceCell<AppHandle> = OnceCell::new();
static SHUT_DOWN: AtomicBool = AtomicBool::new(false);
static SITES: Lazy<Mutex<HashMap<&'static str, SiteState>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// The lock is never held across a call into the window system: page-load events arrive on the
/// main thread and take it too.
fn sites() -> MutexGuard<'static, HashMap<&'static str, SiteState>> {
    SITES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn with_site<T>(key: &str, change: impl FnOnce(&mut SiteState) -> T) -> Option<T> {
    sites().get_mut(key).map(change)
}

fn unavailable() -> AppError {
    AppError::Other("the in-app browser is not available".to_string())
}

fn app() -> Result<AppHandle, AppError> {
    if SHUT_DOWN.load(Ordering::SeqCst) {
        return Err(unavailable());
    }
    APP.get().cloned().ok_or_else(unavailable)
}

pub fn is_available() -> bool {
    APP.get().is_some() && !SHUT_DOWN.load(Ordering::SeqCst)
}

/// Where the site's session stands; see `SessionState`.
pub fn session_state(site: &SiteSpec) -> SessionState {
    if !is_available() {
        return SessionState::Cold;
    }
    with_site(site.key, |state| match &state.blocked {
        Some(blocked) => SessionState::Refused(blocked.origin.clone()),
        None if state.window.is_some() && state.walk.remembered().is_some() => SessionState::Warm,
        None => SessionState::Cold,
    })
    .filter(|state| *state != SessionState::Cold)
    .unwrap_or_else(|| {
        if answered_before(site) {
            SessionState::Resumable
        } else {
            SessionState::Cold
        }
    })
}

/// Gives the module the app handle it creates windows with. Called once at startup.
pub fn init(app: AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    let known = app.path().app_local_data_dir().ok().map(|dir| dir.join(KNOWN_ORIGINS_FILE));
    let _ = KNOWN_ORIGINS.set(KnownOrigins::open(known));
    // The hidden windows must not keep the app running once its own window is gone.
    if let Some(main) = app.get_webview_window(MAIN_WINDOW) {
        main.on_window_event(|event| {
            if matches!(event, WindowEvent::Destroyed) {
                shut_down();
            }
        });
    }
    tauri::async_runtime::spawn(async {
        while !SHUT_DOWN.load(Ordering::SeqCst) {
            tokio::time::sleep(REAP_INTERVAL).await;
            close_idle_windows();
        }
    });
}

fn shut_down() {
    SHUT_DOWN.store(true, Ordering::SeqCst);
    let windows: Vec<WebviewWindow> = sites()
        .values_mut()
        .filter_map(|state| {
            state.walk.reset_page();
            state.visible = false;
            state.window.take()
        })
        .collect();
    for window in windows {
        let _ = window.destroy();
    }
}

fn close_idle_windows() {
    let windows: Vec<WebviewWindow> = sites()
        .values_mut()
        .filter(|state| !state.visible && !state.busy && state.last_used.elapsed() >= IDLE_CLOSE_AFTER)
        .filter_map(|state| {
            state.walk.reset_page();
            state.window.take()
        })
        .collect();
    for window in windows {
        tracing::info!("closing the idle in-app browser window {}", window.label());
        let _ = window.destroy();
    }
}

pub fn window_label(site_key: &str) -> String {
    let safe: String = site_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    format!("site-{}", safe)
}

pub fn window_title(label: &str) -> String {
    format!("Verify to continue — {}", label)
}

fn site_url(origin: &str, path: &str) -> Result<Url, AppError> {
    Url::parse(&format!("{}{}", origin, path))
        .map_err(|e| AppError::Other(format!("invalid site address {}: {}", origin, e)))
}

fn page_event(key: &'static str, url: &Url, finished: bool) {
    let Some(origin) = origin_of(url) else {
        return;
    };
    with_site(key, |state| {
        state.page_events += 1;
        state.walk.on_page(&origin, finished, Instant::now());
    });
}

fn build_window(app: &AppHandle, site: &'static SiteSpec, url: Url) -> tauri::Result<WebviewWindow> {
    let key = site.key;
    // The sites get a data directory of their own: their cookies persist there across restarts,
    // apart from the app's own page, and what is set up for these windows applies only to them
    // (on Linux a download handler belongs to everything sharing a data directory).
    let data_directory = app.path().app_local_data_dir()?.join(DATA_FOLDER);
    let window = WebviewWindowBuilder::new(app, window_label(key), WebviewUrl::External(url))
        .title(window_title(&display_label(site)))
        .inner_size(1000.0, 720.0)
        .center()
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .data_directory(data_directory)
        .on_page_load(move |_window, payload| {
            page_event(key, payload.url(), matches!(payload.event(), PageLoadEvent::Finished));
        })
        // Nothing a site offers is ever saved to disk from these windows.
        .on_download(|_webview, event| !matches!(event, DownloadEvent::Requested { .. }))
        .build()?;
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            // The session lives in this window, so closing it only hides it.
            api.prevent_close();
            closed_by_user(key);
        }
    });
    Ok(window)
}

/// Creates the site's window on `origin`. The caller has set `busy`; the task clears it and
/// stores the window itself, so a caller that stops waiting loses nothing.
fn spawn_create(
    app: AppHandle,
    site: &'static SiteSpec,
    origin: String,
    path: String,
) -> tauri::async_runtime::JoinHandle<Result<WebviewWindow, AppError>> {
    tauri::async_runtime::spawn_blocking(move || {
        let built = site_url(&origin, &path).and_then(|url| {
            build_window(&app, site, url).map_err(|e| AppError::Other(e.to_string()))
        });
        let stored = {
            let mut sites = sites();
            let state = sites.entry(site.key).or_insert_with(|| SiteState::new(site));
            state.busy = false;
            match &built {
                Ok(window) if !SHUT_DOWN.load(Ordering::SeqCst) => {
                    state.window = Some(window.clone());
                    true
                }
                Ok(_) => false,
                Err(e) => {
                    tracing::warn!("in-app browser window for {} could not be opened: {}", site.label, e.reason());
                    state.walk.fail_all(
                        &format!("the in-app browser could not be opened ({})", e.reason()),
                        Instant::now(),
                    );
                    false
                }
            }
        };
        match built {
            Ok(window) if stored => Ok(window),
            Ok(window) => {
                let _ = window.destroy();
                Err(unavailable())
            }
            Err(e) => Err(e),
        }
    })
}

async fn accepts_connections(origin: String) -> bool {
    let Ok(url) = Url::parse(&origin) else {
        return true;
    };
    let (Some(host), Some(port)) = (url.host_str().map(String::from), url.port_or_known_default()) else {
        return true;
    };
    // The system resolver, like the webview: a name it resolves to a dead address is found out
    // here in a moment instead of costing a whole page-load timeout.
    matches!(
        tokio::time::timeout(REACH_TIMEOUT, tokio::net::TcpStream::connect((host.as_str(), port))).await,
        Ok(Ok(_))
    )
}

/// Works out the mirror order and starts the walk. The caller has set `busy`.
fn spawn_order(site: &'static SiteSpec, origins: Vec<String>, remembered: Option<usize>) {
    tauri::async_runtime::spawn(async move {
        let checks: Vec<_> = origins
            .into_iter()
            .map(|origin| tauri::async_runtime::spawn(accepts_connections(origin)))
            .collect();
        let mut reachable = Vec::with_capacity(checks.len());
        for check in checks {
            reachable.push(check.await.unwrap_or(true));
        }
        with_site(site.key, |state| {
            state.busy = false;
            state.walk.begin(order_origins(reachable.len(), remembered, &reachable));
        });
    });
}

fn navigate(site_key: &str, window: &WebviewWindow, origin: &str, path: &str) {
    let sent = site_url(origin, path).and_then(|url| {
        window.navigate(url).map_err(|e| AppError::Other(e.to_string()))
    });
    if let Err(e) = sent {
        tracing::warn!("in-app browser window {} is unusable: {}", window.label(), e.reason());
        with_site(site_key, |state| {
            state.window = None;
            state.walk.reset_page();
        });
        let _ = window.destroy();
    }
}

struct Page {
    window: WebviewWindow,
    origin: String,
    since: Instant,
}

enum Action {
    Ready(Page),
    Verifying(String),
    Fail(String),
    Order(Vec<String>, Option<usize>),
    Create(String),
    Navigate(WebviewWindow, String),
    Wait,
}

/// Waits until the site's window shows a loaded page of one of its mirrors, creating the window
/// and walking the mirrors as needed. Progress is kept when the caller stops waiting.
async fn ready_page(app: &AppHandle, site: &'static SiteSpec, deadline: Instant) -> Result<Page, AppError> {
    loop {
        let action = {
            let mut sites = sites();
            let state = sites.entry(site.key).or_insert_with(|| SiteState::new(site));
            state.last_used = Instant::now();
            if state.visible {
                // The user is at the window; it is not loaded or sent anywhere from here.
                match &state.blocked {
                    Some(blocked) => Action::Verifying(blocked.origin.clone()),
                    None => Action::Wait,
                }
            } else if state.busy {
                Action::Wait
            } else {
                let now = Instant::now();
                let mut step = state.walk.step(now);
                if step == WalkStep::Order && state.walk.origins().len() == 1 {
                    state.walk.begin(vec![0]);
                    step = state.walk.step(now);
                }
                match step {
                    WalkStep::Order => {
                        state.busy = true;
                        Action::Order(state.walk.origins().to_vec(), state.walk.remembered())
                    }
                    WalkStep::Load(origin) => match state.window.clone() {
                        Some(window) => Action::Navigate(window, origin),
                        None => {
                            state.busy = true;
                            Action::Create(origin)
                        }
                    },
                    WalkStep::Wait => Action::Wait,
                    WalkStep::Ready { origin, since } => match state.window.clone() {
                        Some(window) => Action::Ready(Page { window, origin, since }),
                        None => {
                            state.walk.reset_page();
                            Action::Wait
                        }
                    },
                    WalkStep::Failed(reason) => Action::Fail(reason),
                }
            }
        };
        match action {
            Action::Ready(page) => return Ok(page),
            Action::Verifying(origin) => return Err(verification_required(site, &origin)),
            Action::Fail(reason) => return Err(AppError::Unreachable(reason)),
            Action::Order(origins, remembered) => spawn_order(site, origins, remembered),
            Action::Create(origin) => {
                spawn_create(app.clone(), site, origin, "/".to_string());
            }
            Action::Navigate(window, origin) => navigate(site.key, &window, &origin, "/"),
            Action::Wait => {}
        }
        if Instant::now() >= deadline {
            return Err(AppError::Network(format!(
                "{} is still loading in the in-app browser",
                site.label
            )));
        }
        tokio::time::sleep(STEP_INTERVAL).await;
    }
}

/// How a request made inside a page ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Response(WebResponse),
    /// The request failed inside the page.
    Failed(String),
    /// The window shows another origin.
    Elsewhere,
    /// The page that was asked was replaced before it answered.
    Gone,
    TooLarge,
    TimedOut,
}

/// A page of a site that can be asked: the window, or a stand-in for it in tests.
#[async_trait]
pub trait PageAccess: Send + Sync {
    /// Makes the request inside the page.
    async fn request(&self, origin: &str, asked: &WebRequest, deadline: Instant) -> Outcome;
    /// Says what document the page is.
    async fn document(&self, origin: &str) -> DocumentState;
}

/// What asking a page came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    Answer(WebResponse),
    /// The site wants the visitor to pass its check first: the answer was its check or block
    /// page, or no answer came and the page itself is one.
    Refused,
    /// The request failed on a page that is the site itself.
    RequestFailed(String),
    /// The page moved, vanished, timed out or answered with too much.
    Other(Outcome),
}

/// Makes the request in the page and says what the outcome means. A request that fails, hangs
/// or loses its page says nothing by itself: a check page reloads itself and aborts whatever was
/// in flight, so the failure looks like a network error. The document the page is on decides.
/// Only on a page that is the site itself is a failed request the request's own failure.
pub async fn ask(page: &dyn PageAccess, origin: &str, asked: &WebRequest, deadline: Instant) -> Asked {
    let outcome = page.request(origin, asked, deadline).await;
    match outcome {
        Outcome::Response(response) => {
            if detect_barrier(response.status, &response.headers, &response.head_text()).is_some() {
                return Asked::Refused;
            }
            // A refusal without the marks of a check (a rate limit, say) from a site whose page
            // is its check page is the protection answering, not the site.
            if matches!(response.status, 403 | 429 | 503)
                && matches!(page.document(origin).await, DocumentState::Barrier(_))
            {
                return Asked::Refused;
            }
            Asked::Answer(response)
        }
        Outcome::TooLarge => Asked::Other(Outcome::TooLarge),
        other => match (page.document(origin).await, other) {
            (DocumentState::Barrier(_), _) => Asked::Refused,
            (DocumentState::Site, Outcome::Failed(reason)) => Asked::RequestFailed(reason),
            (DocumentState::Elsewhere, Outcome::Failed(_)) => Asked::Other(Outcome::Elsewhere),
            (DocumentState::Unreadable, Outcome::Failed(_)) => Asked::Other(Outcome::Gone),
            (_, other) => Asked::Other(other),
        },
    }
}

struct WindowPage(WebviewWindow);

#[async_trait]
impl PageAccess for WindowPage {
    async fn request(&self, origin: &str, asked: &WebRequest, deadline: Instant) -> Outcome {
        request(&self.0, origin, asked, deadline).await
    }

    async fn document(&self, origin: &str) -> DocumentState {
        // The window may be between two documents; it is asked a few times.
        for _ in 0..4 {
            let state = match evaluate(&self.0, &document_script()).await {
                Some(raw) => parse_document_state(&raw, origin),
                None => DocumentState::Unreadable,
            };
            if state != DocumentState::Unreadable {
                return state;
            }
            tokio::time::sleep(STEP_INTERVAL * 3).await;
        }
        DocumentState::Unreadable
    }
}

/// Removes a request's state from the page unless its answer was collected.
struct RequestGuard {
    window: WebviewWindow,
    key: String,
    collected: bool,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if !self.collected {
            let _ = self.window.eval(cancel_script(&self.key));
        }
    }
}

/// What a script evaluates to in the window, as the JSON text the webview hands back.
async fn evaluate(window: &WebviewWindow, script: &str) -> Option<String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Mutex::new(Some(tx));
    let sent = window.eval_with_callback(script.to_string(), move |raw| {
        if let Some(tx) = tx.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = tx.send(raw);
        }
    });
    if sent.is_err() {
        return None;
    }
    match tokio::time::timeout(POLL_REPLY_TIMEOUT, rx).await {
        Ok(Ok(raw)) => Some(raw),
        _ => None,
    }
}

async fn collect_once(window: &WebviewWindow, script: &str) -> Collected {
    match evaluate(window, script).await {
        Some(raw) => parse_collected(&raw),
        None => Collected::NoAnswer,
    }
}

async fn request(window: &WebviewWindow, origin: &str, asked: &WebRequest, deadline: Instant) -> Outcome {
    let key = new_request_key();
    if window.eval(request_script(&key, origin, asked)).is_err() {
        return Outcome::Gone;
    }
    let mut guard = RequestGuard {
        window: window.clone(),
        key: key.clone(),
        collected: false,
    };
    let collect = collect_script(&key);
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let collected = collect_once(window, &collect).await;
        match collected {
            Collected::NoAnswer | Collected::Pending => {
                if Instant::now() >= deadline {
                    return Outcome::TimedOut;
                }
            }
            other => {
                guard.collected = true;
                return match other {
                    Collected::Response { status, url, redirected, headers, body } => {
                        Outcome::Response(WebResponse {
                            status,
                            origin: origin.to_string(),
                            url: answered_url(origin, &asked.path, &url),
                            redirected,
                            headers,
                            body,
                        })
                    }
                    Collected::TooLarge => Outcome::TooLarge,
                    Collected::Failed(reason) => Outcome::Failed(reason),
                    Collected::Elsewhere(_) => Outcome::Elsewhere,
                    _ => Outcome::Gone,
                };
            }
        }
    }
}

fn verification_required(site: &SiteSpec, origin: &str) -> AppError {
    AppError::VerificationRequired {
        site: site.key.to_string(),
        origin: origin.to_string(),
    }
}

/// Records that `origin` wants the visitor to pass its check, so the site is listed for
/// verification, and gives the error that says so. The refusal is looked for again with a plain
/// GET of the address that was asked.
fn note_refused(site: &'static SiteSpec, origin: &str, asked: &WebRequest) -> AppError {
    with_site(site.key, |state| {
        state.blocked = Some(Blocked {
            origin: origin.to_string(),
            path: asked.path.clone(),
            headers: asked.headers.clone(),
            at: Instant::now(),
            plain: false,
        });
    });
    if let Some(known) = KNOWN_ORIGINS.get() {
        known.remove(origin);
    }
    tracing::info!("{} refuses the in-app browser until the user verifies", host_of(origin));
    verification_required(site, origin)
}

/// The app's plain client met `origin`'s check page and no in-app browser session was asked for
/// it: the site is listed for verification, so the user can pass the check in the site's
/// window (Verify), and `asked` is what is repeated there to see it pass. A session started
/// later still tries by itself first.
pub fn note_plain_refusal(site: &'static SiteSpec, origin: &str, asked: &WebRequest) {
    if !is_available() {
        return;
    }
    let mut sites = sites();
    let state = sites.entry(site.key).or_insert_with(|| SiteState::new(site));
    if state.blocked.is_some() || state.window.is_some() {
        return;
    }
    state.blocked = Some(Blocked {
        origin: origin.to_string(),
        path: asked.path.clone(),
        headers: asked.headers.clone(),
        // Old enough that `fetch` does not take it for a refusal of the session just now.
        at: Instant::now().checked_sub(BLOCK_MEMORY * 2).unwrap_or_else(Instant::now),
        plain: true,
    });
}

/// The plain client is answered by the site again; a refusal noted from it alone is dropped.
pub fn clear_plain_refusal(site: &SiteSpec) {
    with_site(site.key, |state| {
        if !state.visible && state.blocked.as_ref().map_or(false, |blocked| blocked.plain) {
            state.blocked = None;
        }
    });
}

/// The request, made by a same-origin `fetch()` inside the site's own page in a hidden window of
/// the app. The answer of a check or block page is reported as
/// `AppError::VerificationRequired`; no window is shown from here. Any other answer, whatever
/// its status, is returned as the answer.
pub async fn fetch(
    site: &'static SiteSpec,
    asked: &WebRequest,
    timeout: Duration,
) -> Result<WebResponse, AppError> {
    let app = app()?;
    if let Some(problem) = asked.problem() {
        return Err(AppError::Other(format!(
            "not a request for the in-app browser on {} ({}): {} {}",
            site.label,
            problem,
            clipped(&asked.method, 12),
            clipped(&asked.path, 80)
        )));
    }
    let deadline = Instant::now() + timeout;

    // While the user is at the window it is theirs: nothing is asked or loaded in it from here.
    let refused_just_now = with_site(site.key, |state| {
        state
            .blocked
            .as_ref()
            .filter(|blocked| state.visible || blocked.at.elapsed() < BLOCK_MEMORY)
            .map(|blocked| blocked.origin.clone())
    })
    .flatten();
    if let Some(origin) = refused_just_now {
        return Err(verification_required(site, &origin));
    }

    let mut moves = 0;
    // The origin whose check page was met while waiting for it to pass by itself.
    let mut met_check: Option<String> = None;
    loop {
        let page = match ready_page(&app, site, deadline).await {
            Ok(page) => page,
            // A check page that reloads itself can use up the time; it is still a check.
            Err(AppError::Network(_)) if met_check.is_some() => {
                let origin = met_check.unwrap_or_default();
                return Err(note_refused(site, &origin, asked));
            }
            Err(e) => return Err(e),
        };
        match ask(&WindowPage(page.window.clone()), &page.origin, asked, deadline).await {
            Asked::Other(Outcome::TooLarge) => {
                with_site(site.key, |state| state.request_failures = 0);
                return Err(AppError::Network(format!(
                    "{} answered with more than the in-app browser hands back",
                    host_of(&page.origin)
                )));
            }
            Asked::Refused => {
                with_site(site.key, |state| state.request_failures = 0);
                {
                    let loaded_for = page.since.elapsed();
                    if loaded_for < SETTLE && Instant::now() + SETTLE_RETRY < deadline {
                        met_check = Some(page.origin.clone());
                        tokio::time::sleep(SETTLE_RETRY).await;
                        continue;
                    }
                    let already_known = with_site(site.key, |state| state.blocked.is_some()) == Some(true);
                    if !already_known && loaded_for >= STALE_PAGE && Instant::now() + SETTLE_RETRY < deadline {
                        with_site(site.key, |state| state.walk.refresh(&page.origin));
                        continue;
                    }
                    return Err(note_refused(site, &page.origin, asked));
                }
            }
            Asked::Answer(response) => {
                with_site(site.key, |state| state.request_failures = 0);
                let newly_settled = with_site(site.key, |state| {
                    let before = state.walk.remembered();
                    state.blocked = None;
                    state.walk.confirm(&page.origin);
                    state.last_used = Instant::now();
                    state.walk.remembered() != before
                });
                if newly_settled == Some(true) {
                    tracing::info!(
                        "{} is answered through the in-app browser on {}",
                        display_label(site),
                        host_of(&page.origin)
                    );
                }
                if let Some(known) = KNOWN_ORIGINS.get() {
                    known.add(&page.origin);
                }
                return Ok(response);
            }
            Asked::RequestFailed(reason) => {
                tracing::warn!(
                    "in-app browser request to {} failed on a page of the site: {}",
                    host_of(&page.origin),
                    reason
                );
                // With mirrors the next one is tried. A site of one origin has nowhere to go:
                // the failure is the request's (a redirect off the site, an aborted answer)
                // unless it keeps happening.
                let give_up_on_page = with_site(site.key, |state| {
                    state.request_failures += 1;
                    let condemn = state.walk.origins().len() > 1 || state.request_failures >= MAX_REQUEST_FAILURES;
                    if condemn {
                        state.request_failures = 0;
                        state.walk.fail_origin(&page.origin, "could not be reached", Instant::now());
                    }
                    condemn && state.walk.origins().len() > 1
                }) == Some(true);
                if !give_up_on_page {
                    return Err(AppError::Network(format!(
                        "the request to {} failed in the in-app browser ({})",
                        host_of(&page.origin),
                        reason
                    )));
                }
            }
            Asked::Other(Outcome::Elsewhere | Outcome::Gone) if moves >= MAX_PAGE_MOVES => {
                // The window never settled on a page of the site that could be asked. Whatever
                // it is doing, the user can look at it; it is not the request that failed.
                tracing::warn!("the {} page in the in-app browser kept changing", site.label);
                return Err(note_refused(site, &page.origin, asked));
            }
            Asked::Other(Outcome::Elsewhere) => {
                moves += 1;
                with_site(site.key, |state| state.walk.mark_drifted(&page.origin));
            }
            Asked::Other(Outcome::Gone) => {
                moves += 1;
                tokio::time::sleep(STEP_INTERVAL).await;
            }
            Asked::Other(_) => {
                with_site(site.key, |state| state.walk.request_reload(&page.origin, Instant::now()));
                return Err(AppError::Network(format!(
                    "{} did not answer in the in-app browser",
                    host_of(&page.origin)
                )));
            }
        }
    }
}

// ---------------------------------------------------------------- verification by the user

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SiteVerification {
    pub site: String,
    pub label: String,
    pub origin: String,
}

/// Sites whose last request through the in-app browser met a check or block page.
pub fn verification_status() -> Vec<SiteVerification> {
    let mut pending: Vec<SiteVerification> = sites()
        .values()
        .filter_map(|state| {
            state.blocked.as_ref().map(|blocked| SiteVerification {
                site: state.spec.key.to_string(),
                label: display_label(state.spec),
                origin: blocked.origin.clone(),
            })
        })
        .collect();
    pending.sort_by(|a, b| a.label.cmp(&b.label));
    pending
}

#[derive(Clone, Serialize)]
struct SiteEvent {
    site: String,
    label: String,
}

fn emit(event: &str, site: &SiteSpec) {
    let Some(app) = APP.get() else {
        return;
    };
    let payload = SiteEvent {
        site: site.key.to_string(),
        label: display_label(site),
    };
    if let Err(e) = app.emit_to(MAIN_WINDOW, event, payload) {
        tracing::warn!("could not announce {} for {}: {}", event, site.label, e);
    }
}

fn hide(window: &WebviewWindow) {
    let _ = window.hide();
    let _ = window.set_skip_taskbar(true);
}

/// Repeats the refused request. `Some(true)` means the site still refuses, `None` that it could
/// not be told (the page is elsewhere, or nothing was refused).
async fn still_refused(site_key: &'static str) -> Option<bool> {
    let (window, blocked) = with_site(site_key, |state| {
        let blocked = state.blocked.clone()?;
        let on_site = state.walk.page_origin() == Some(blocked.origin.as_str());
        let window = state.window.clone()?;
        on_site.then_some((window, blocked))
    })
    .flatten()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let probe = WebRequest {
        headers: blocked.headers.clone(),
        ..WebRequest::get(&blocked.path)
    };
    let page = WindowPage(window);
    verification_probe(&page, &blocked.origin, &probe, deadline).await
}

/// Whether a site that refused still does. The check counts as passed (`Some(false)`) only when
/// the page is no longer a check page and the refused request is then answered; `None` means it
/// could not be told yet.
pub async fn verification_probe(
    page: &dyn PageAccess,
    origin: &str,
    refused: &WebRequest,
    deadline: Instant,
) -> Option<bool> {
    match page.document(origin).await {
        DocumentState::Barrier(_) => return Some(true),
        DocumentState::Site => {}
        DocumentState::Elsewhere | DocumentState::Unreadable => return None,
    }
    match ask(page, origin, refused, deadline).await {
        Asked::Answer(_) => Some(false),
        Asked::Refused => Some(true),
        _ => None,
    }
}

/// The site answers again: hide the window, put it back on the site's front page and tell the
/// frontend. Only the first caller after a refusal does anything.
fn finish_verified(site_key: &'static str) {
    let Some((spec, window, origin)) = with_site(site_key, |state| {
        let blocked = state.blocked.take()?;
        state.visible = false;
        state.last_used = Instant::now();
        state.walk.confirm(&blocked.origin);
        if state.window.is_some() {
            state.walk.focus(&blocked.origin, Instant::now());
        }
        Some((state.spec, state.window.clone(), blocked.origin))
    })
    .flatten() else {
        return;
    };
    if let Some(window) = window {
        hide(&window);
        navigate(site_key, &window, &origin, "/");
    }
    if let Some(known) = KNOWN_ORIGINS.get() {
        known.add(&origin);
    }
    tracing::info!("{} answers the in-app browser again", display_label(spec));
    emit(EVENT_VERIFIED, spec);
}

fn closed_by_user(site_key: &'static str) {
    let Some((spec, window)) = with_site(site_key, |state| {
        let was_visible = std::mem::replace(&mut state.visible, false);
        was_visible.then(|| (state.spec, state.window.clone()))
    })
    .flatten() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Some(window) = window {
            hide(&window);
        }
        // The user may have passed the check a moment before closing the window.
        if still_refused(site_key).await == Some(false) {
            finish_verified(site_key);
            return;
        }
        let back = with_site(site_key, |state| {
            let origin = state.walk.current()?.to_string();
            let window = state.window.clone()?;
            state.walk.focus(&origin, Instant::now());
            Some((window, origin))
        })
        .flatten();
        if let Some((window, origin)) = back {
            navigate(site_key, &window, &origin, "/");
        }
        emit(EVENT_CANCELLED, spec);
    });
}

fn spawn_probe(site_key: &'static str) {
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let mut probed_at = Instant::now();
        let mut probed_events = None;
        while started.elapsed() < PROBE_FOR {
            tokio::time::sleep(PROBE_TICK).await;
            let Some((watching, events)) = with_site(site_key, |state| {
                (state.visible && state.blocked.is_some(), state.page_events)
            }) else {
                return;
            };
            if !watching {
                return;
            }
            if probed_events == Some(events) && probed_at.elapsed() < PROBE_INTERVAL {
                continue;
            }
            probed_events = Some(events);
            probed_at = Instant::now();
            if still_refused(site_key).await == Some(false) {
                // A window the user closed meanwhile is dealt with by the close handler.
                if with_site(site_key, |state| state.visible) == Some(true) {
                    finish_verified(site_key);
                }
                return;
            }
        }
    });
}

/// Shows the site's window on the page that was refused, for the user to pass the site's check.
/// While it is open the refused request is repeated as its page changes; once it is answered
/// the window is hidden and `site-verified` is emitted. Closing the window hides it and emits
/// `site-verification-cancelled` if the site still refuses.
pub async fn open_verification(site_key: &str) -> Result<(), AppError> {
    let app = app()?;
    enum Open {
        Show(WebviewWindow),
        Navigate(WebviewWindow, String, String),
        Create(&'static SiteSpec, String, String),
        Busy,
    }
    let unknown = || AppError::Other(format!("no site named '{}' uses the in-app browser", site_key));

    let deadline = Instant::now() + LOAD_TIMEOUT;
    let (spec, window, newly_shown) = loop {
        let (spec, open) = with_site(site_key, |state| {
            let spec = state.spec;
            if state.busy {
                return (spec, Open::Busy);
            }
            if state.visible {
                if let Some(window) = state.window.clone() {
                    return (spec, Open::Show(window));
                }
            }
            let (origin, path) = match &state.blocked {
                Some(blocked) => (blocked.origin.clone(), blocked.path.clone()),
                None => {
                    let origin = state
                        .walk
                        .current()
                        .or_else(|| state.walk.origins().first().map(String::as_str))
                        .unwrap_or_default()
                        .to_string();
                    (origin, "/".to_string())
                }
            };
            state.walk.focus(&origin, Instant::now());
            state.visible = true;
            state.last_used = Instant::now();
            let open = match state.window.clone() {
                Some(window) => Open::Navigate(window, origin, path),
                None => {
                    state.busy = true;
                    Open::Create(spec, origin, path)
                }
            };
            (spec, open)
        })
        .ok_or_else(unknown)?;

        match open {
            Open::Show(window) => break (spec, window, false),
            Open::Navigate(window, origin, path) => {
                navigate(spec.key, &window, &origin, &path);
                break (spec, window, true);
            }
            Open::Create(spec, origin, path) => {
                let created = spawn_create(app.clone(), spec, origin, path)
                    .await
                    .map_err(|e| AppError::Other(e.to_string()))
                    .and_then(|window| window);
                match created {
                    Ok(window) => break (spec, window, true),
                    Err(e) => {
                        with_site(spec.key, |state| state.visible = false);
                        return Err(e);
                    }
                }
            }
            Open::Busy if Instant::now() >= deadline => {
                return Err(AppError::Other(format!(
                    "the in-app browser for {} is still starting; try again",
                    spec.label
                )));
            }
            Open::Busy => tokio::time::sleep(STEP_INTERVAL).await,
        }
    };

    let _ = window.set_title(&window_title(&display_label(spec)));
    let _ = window.set_skip_taskbar(false);
    if let Err(e) = window.show() {
        with_site(spec.key, |state| state.visible = false);
        return Err(AppError::Other(format!(
            "the verification window could not be shown: {}",
            e
        )));
    }
    let _ = window.unminimize();
    let _ = window.set_focus();
    if newly_shown {
        spawn_probe(spec.key);
    }
    Ok(())
}
