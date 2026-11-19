//! The download engine: parallel HLS segments with retries and link renewal, byte ranges and a
//! safe resume for direct files, integrity checks, speed and a speed limit. `downloads/mod.rs`
//! drives it; ffmpeg still does the mux.

use super::{PART_SUFFIX, STALL_TIMEOUT};
use crate::clients::app_settings::{self, DohMode};
use crate::clients::http::note_unreachable_host;
use once_cell::sync::Lazy;
use reqwest::header::{CONTENT_RANGE, CONTENT_TYPE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE, RETRY_AFTER};
use reqwest::{Client, RequestBuilder, Response};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::task::JoinSet;

/// Most requests in flight for one HLS download, whatever a host allows.
pub const MAX_SEGMENT_CONNECTIONS: usize = 8;
/// Successes in a row that earn a host one more connection.
pub const SUCCESSES_PER_STEP: u32 = 8;
/// Steps of `SUCCESSES_PER_STEP` successes at a host's ceiling that raise the ceiling by one.
const CEILING_RELAX_STEPS: u32 = 32;
/// Longest a single segment request may take, headers and body together.
const SEGMENT_TIMEOUT: Duration = Duration::from_secs(120);
/// A `Retry-After` longer than this is not waited for in full.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);
/// Times a download asks its extension for a fresh link.
const MAX_RENEWALS: u32 = 3;
const PARALLEL_DOWNLOADS_MAX: usize = 3;
/// Speed is averaged over this long.
pub const SPEED_WINDOW: Duration = Duration::from_secs(5);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---- Errors and retries ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// The host could not be reached, or the connection dropped.
    Connect(String),
    Timeout,
    /// The host answered with an error status.
    Status { code: u16, retry_after: Option<Duration> },
    /// The bytes are not what was asked for (a web page, the wrong size, not video).
    Integrity(String),
    /// A local file could not be read or written.
    Io(String),
    /// The file changed on the server while it was being fetched in parts: start again from byte 0.
    Changed,
    /// The body is a playlist, so ffmpeg should read the URL itself (`"hls"` or `"dash"`).
    Manifest(&'static str),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Connect(e) | FetchError::Integrity(e) | FetchError::Io(e) => f.write_str(e),
            FetchError::Timeout => f.write_str("The stream host did not answer in time."),
            FetchError::Changed => f.write_str("The file changed on the server while it was downloading."),
            FetchError::Status { code, .. } => write!(f, "HTTP {}", code),
            FetchError::Manifest(kind) => write!(f, "The body is a {} playlist.", kind),
        }
    }
}

fn io_error(path: &Path, e: std::io::Error) -> FetchError {
    FetchError::Io(format!("Could not write {}: {}", path.display(), e))
}

fn map_reqwest(e: reqwest::Error) -> FetchError {
    if e.is_redirect() {
        FetchError::Integrity(super::redirect_refusal(&e))
    } else if e.is_timeout() {
        FetchError::Timeout
    } else {
        FetchError::Connect(std::error::Error::source(&e).map_or_else(|| e.to_string(), |s| format!("{}: {}", e, s)))
    }
}

/// Whether trying the same request again can help: timeouts, dropped connections, 408, 429, 5xx.
pub fn is_retryable(error: &FetchError) -> bool {
    match error {
        FetchError::Timeout | FetchError::Connect(_) => true,
        FetchError::Status { code, .. } => matches!(code, 408 | 429 | 500..=599),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    /// Tries, the first one included.
    pub max_tries: u32,
    pub base: Duration,
    pub jitter: bool,
}

impl RetryPolicy {
    /// Four tries, about 1, 2 and 4 seconds apart, with a little jitter.
    pub const APP: RetryPolicy = RetryPolicy { max_tries: 4, base: Duration::from_secs(1), jitter: true };
}

/// How long to wait after failed try `attempt` (0 for the first): the base doubled each time,
/// never less than `retry_after`, plus `jitter_ms`.
pub fn backoff_delay(policy: &RetryPolicy, attempt: u32, retry_after: Option<Duration>, jitter_ms: u64) -> Duration {
    let doubled = policy.base.saturating_mul(1u32 << attempt.min(10));
    doubled.max(retry_after.unwrap_or_default().min(MAX_RETRY_AFTER)) + Duration::from_millis(jitter_ms)
}

pub(super) async fn wait_before_retry(policy: &RetryPolicy, attempt: u32, error: &FetchError) {
    let retry_after = match error {
        FetchError::Status { retry_after, .. } => *retry_after,
        _ => None,
    };
    let jitter = if policy.jitter { rand::random::<u64>() % 250 } else { 0 };
    tokio::time::sleep(backoff_delay(policy, attempt, retry_after, jitter)).await;
}

/// True only in Automatic mode, for a host that could not be connected to: its DNS may be blocked.
pub fn doh_switch_decision(mode: DohMode, error: &FetchError) -> bool {
    mode == DohMode::Auto && matches!(error, FetchError::Connect(_) | FetchError::Timeout)
}

/// After a failed request: a host that cannot be connected to is looked up through DoH from now
/// on (Automatic mode), so the retry uses it.
pub(super) fn note_failure(url: &str, error: &FetchError) {
    if doh_switch_decision(app_settings::current().doh_mode, error) {
        if let Some(host) = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)) {
            note_unreachable_host(&host, &error.to_string());
        }
    }
}

// ---- Adaptive connections per host -------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct HostState {
    limit: usize,
    successes: u32,
    ceiling: Option<usize>,
    /// Steps of successes spent at the ceiling.
    capped_steps: u32,
}

/// How many requests to run at once per host: starts low, grows after a run of successes, is
/// halved (once per level) when the host pushes back, and held below the level it refused until
/// a long run of successes at the ceiling shows the host has relaxed.
#[derive(Debug)]
pub struct HostLimits {
    start: usize,
    max: usize,
    hosts: Mutex<HashMap<String, HostState>>,
}

impl HostLimits {
    pub fn new(start: usize, max: usize) -> Self {
        let max = max.max(1);
        Self { start: start.clamp(1, max), max, hosts: Mutex::new(HashMap::new()) }
    }

    fn state(&self, host: &str) -> HostState {
        lock(&self.hosts).get(host).copied().unwrap_or(HostState { limit: self.start, successes: 0, ceiling: None, capped_steps: 0 })
    }

    pub fn limit(&self, host: &str) -> usize {
        self.state(host).limit
    }

    pub fn ceiling(&self, host: &str) -> Option<usize> {
        self.state(host).ceiling
    }

    pub fn record_success(&self, host: &str) {
        let mut state = self.state(host);
        state.successes += 1;
        if state.successes >= SUCCESSES_PER_STEP {
            state.successes = 0;
            if state.limit < state.ceiling.unwrap_or(self.max).min(self.max) {
                state.limit += 1;
            } else if let Some(ceiling) = state.ceiling {
                state.capped_steps += 1;
                if state.capped_steps >= CEILING_RELAX_STEPS {
                    state.capped_steps = 0;
                    state.ceiling = (ceiling + 1 < self.max).then_some(ceiling + 1);
                }
            }
        }
        lock(&self.hosts).insert(host.to_string(), state);
    }

    /// `started` is the limit the failed request began under: pushback from a request that began
    /// above the current (already lowered) limit says nothing new.
    pub fn record_pressure(&self, host: &str, started: usize) {
        let mut state = self.state(host);
        if started > state.limit {
            return;
        }
        let refused = state.limit.saturating_sub(1).max(1);
        state.ceiling = Some(state.ceiling.map_or(refused, |c| c.min(refused)));
        state.limit = (state.limit / 2).max(1);
        state.successes = 0;
        state.capped_steps = 0;
        lock(&self.hosts).insert(host.to_string(), state);
    }
}

/// Limits shared by every download of the app.
pub static HOST_LIMITS: Lazy<Arc<HostLimits>> = Lazy::new(|| Arc::new(HostLimits::new(4, MAX_SEGMENT_CONNECTIONS)));

/// `host[:port]` of a URL.
pub fn host_key(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{}:{}", host, port),
        None => host.to_string(),
    })
}

fn pressure_error(error: &FetchError) -> bool {
    matches!(error, FetchError::Timeout | FetchError::Status { code: 429 | 503, .. })
}

// ---- Integrity ---------------------------------------------------------------------------

/// Three TS packets in a row start with the sync byte, from some offset below one packet.
fn has_ts_sync(body: &[u8]) -> bool {
    body.len() >= 188 * 2 + 1 && (0..188).any(|k| body[k] == 0x47 && body.get(k + 188) == Some(&0x47) && body.get(k + 376) == Some(&0x47))
}

/// Ok for the body of one media segment: not empty, as long as announced, not a web page or
/// JSON error, and starting like MPEG-TS or an fMP4 box.
pub fn check_segment(body: &[u8], content_length: Option<u64>, content_type: Option<&str>) -> Result<(), String> {
    check_encrypted_segment(body, content_length, content_type)?;
    let head = String::from_utf8_lossy(&body[..body.len().min(64)]).trim_start().to_ascii_lowercase();
    if head.starts_with("<!doctype") || head.starts_with("<html") {
        return Err("The stream host returned a web page instead of a video.".to_string());
    }
    if head.starts_with('{') || head.starts_with('[') {
        return Err("The stream host returned an error message instead of a video.".to_string());
    }
    let fmp4 = body.len() >= 8 && matches!(&body[4..8], b"ftyp" | b"styp" | b"moof");
    if body[0] == 0x47 || fmp4 || has_ts_sync(body) {
        Ok(())
    } else {
        Err("A segment of the stream is not video data.".to_string())
    }
}

/// Ok for the body of an AES-128 encrypted segment: ciphertext has no recognisable bytes, so
/// only its size and content type are checked.
pub fn check_encrypted_segment(body: &[u8], content_length: Option<u64>, content_type: Option<&str>) -> Result<(), String> {
    if body.is_empty() {
        return Err("A segment of the stream is empty.".to_string());
    }
    if let Some(expected) = content_length.filter(|len| *len != body.len() as u64) {
        return Err(format!("A segment of the stream is incomplete: received {} of {} bytes.", body.len(), expected));
    }
    let kind = content_type.map(str::to_ascii_lowercase).unwrap_or_default();
    if kind.contains("text/html") {
        return Err("The stream host returned a web page instead of a video.".to_string());
    }
    if kind.contains("json") {
        return Err("The stream host returned an error message instead of a video.".to_string());
    }
    Ok(())
}

/// Ok for an AES-128 key: exactly 16 bytes.
pub fn check_aes_key(body: &[u8]) -> Result<(), String> {
    if body.len() == 16 {
        Ok(())
    } else {
        Err(format!("The stream's decryption key has {} bytes instead of 16.", body.len()))
    }
}

/// Keys are checked as keys, everything else as video.
fn is_key_file(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("key"))
}

fn check_file(path: &Path, encrypted: bool, body: &[u8], len: Option<u64>, kind: Option<&str>) -> Result<(), String> {
    if is_key_file(path) {
        check_aes_key(body)
    } else if encrypted {
        check_encrypted_segment(body, len, kind)
    } else {
        check_segment(body, len, kind)
    }
}

/// Whether the file left by an earlier run is a complete, plausible segment.
fn saved_file_ok(path: &Path, encrypted: bool) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut head = Vec::new();
    if file.by_ref().take(4096).read_to_end(&mut head).is_err() {
        return false;
    }
    check_file(path, encrypted, &head, None, None).is_ok()
}

/// Ok when every file exists and passes the segment check; the message names the first bad one.
pub fn verify_segments_before_mux(paths: &[PathBuf]) -> Result<(), String> {
    verify_files_before_mux(paths, &[])
}

/// `verify_segments_before_mux` where `encrypted[i]` says path `i` is AES-128 ciphertext (a
/// missing entry means it is not).
pub fn verify_files_before_mux(paths: &[PathBuf], encrypted: &[bool]) -> Result<(), String> {
    for (index, path) in paths.iter().enumerate() {
        let head = std::fs::File::open(path).and_then(|file| {
            use std::io::Read;
            let mut head = Vec::new();
            file.take(4096).read_to_end(&mut head).map(|_| head)
        });
        match head {
            Err(e) => return Err(format!("A segment of the stream is missing: segment {} ({}).", index, e)),
            Ok(head) => {
                if let Err(reason) = check_file(path, encrypted.get(index).copied().unwrap_or(false), &head, None, None) {
                    return Err(format!("A segment of the stream is incomplete: segment {}. {}", index, reason));
                }
            }
        }
    }
    Ok(())
}

// ---- Speed -------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct MeterState {
    samples: VecDeque<(Instant, u64)>,
    bytes: u64,
    expected: Option<u64>,
    items_done: usize,
    items_total: usize,
}

/// Bytes per second over a sliding window, summed over every connection that records into it
/// (clones share one meter). It also counts what a download has done, for its progress.
#[derive(Debug, Clone)]
pub struct SpeedMeter {
    window: Duration,
    state: Arc<Mutex<MeterState>>,
}

impl SpeedMeter {
    pub fn new(window: Duration) -> Self {
        Self { window, state: Arc::new(Mutex::new(MeterState::default())) }
    }

    pub fn record(&self, bytes: u64, at: Instant) {
        let mut state = lock(&self.state);
        state.bytes += bytes;
        state.samples.push_back((at, bytes));
        while state.samples.front().is_some_and(|(t, _)| at.saturating_duration_since(*t) > self.window) {
            state.samples.pop_front();
        }
    }

    pub fn bytes_per_sec(&self, at: Instant) -> f64 {
        let state = lock(&self.state);
        let recent: Vec<&(Instant, u64)> =
            state.samples.iter().filter(|(t, _)| at.saturating_duration_since(*t) <= self.window).collect();
        let Some((first, _)) = recent.first() else { return 0.0 };
        let span = at.saturating_duration_since(*first).min(self.window).max(Duration::from_secs(1));
        recent.iter().map(|(_, n)| *n).sum::<u64>() as f64 / span.as_secs_f64()
    }

    /// Bytes on disk for this download, those of an earlier run included.
    pub fn set_bytes(&self, bytes: u64) {
        lock(&self.state).bytes = bytes;
    }

    pub fn set_expected(&self, total: Option<u64>) {
        lock(&self.state).expected = total;
    }

    pub fn set_items(&self, done: usize, total: usize) {
        let mut state = lock(&self.state);
        state.items_done = done;
        state.items_total = total;
    }

    fn item_done(&self) {
        lock(&self.state).items_done += 1;
    }

    pub fn bytes(&self) -> u64 {
        lock(&self.state).bytes
    }

    pub fn expected(&self) -> Option<u64> {
        lock(&self.state).expected
    }

    /// Finished files and the number of files.
    pub fn items(&self) -> (usize, usize) {
        let state = lock(&self.state);
        (state.items_done, state.items_total)
    }
}

/// Seconds until `remaining` bytes are done at `bps`; `None` while nothing moves.
pub fn eta_seconds(remaining: u64, bps: f64) -> Option<u64> {
    (bps > 0.0).then(|| (remaining as f64 / bps).ceil() as u64)
}

#[derive(Debug)]
struct LimiterState {
    bytes_per_sec: Option<u64>,
    next_free: Instant,
}

/// Keeps the combined speed of everything that waits on it under a limit.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<LimiterState>>,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self::with(Some(bytes_per_sec.max(1)))
    }

    pub fn unlimited() -> Self {
        Self::with(None)
    }

    fn with(bytes_per_sec: Option<u64>) -> Self {
        Self { state: Arc::new(Mutex::new(LimiterState { bytes_per_sec, next_free: Instant::now() })) }
    }

    pub fn set_limit(&self, bytes_per_sec: Option<u64>) {
        lock(&self.state).bytes_per_sec = bytes_per_sec.map(|v| v.max(1));
    }

    /// Waits until `bytes` more may pass.
    pub async fn throttle(&self, bytes: u64) {
        let until = {
            let mut state = lock(&self.state);
            let Some(rate) = state.bytes_per_sec else { return };
            let start = state.next_free.max(Instant::now());
            state.next_free = start + Duration::from_secs_f64(bytes as f64 / rate as f64);
            state.next_free
        };
        tokio::time::sleep_until(until.into()).await;
    }
}

/// The limit shared by every download (Settings › Downloads).
pub static LIMITER: Lazy<RateLimiter> = Lazy::new(RateLimiter::unlimited);

/// Downloads that run at once: `max_parallel_downloads`, 1 when unset, at most 3.
pub fn parallel_downloads(settings: &serde_json::Value) -> usize {
    settings
        .get("max_parallel_downloads")
        .and_then(|v| v.as_u64())
        .map_or(1, |n| (n as usize).clamp(1, PARALLEL_DOWNLOADS_MAX))
}

/// `download_speed_limit_kbps` in bytes per second. The key's value is in KiB/s (1024 bytes),
/// whatever its name says; `None` (off) when unset, 0 or negative.
pub fn speed_limit_bps(settings: &serde_json::Value) -> Option<u64> {
    settings.get("download_speed_limit_kbps").and_then(|v| v.as_u64()).filter(|k| *k > 0).map(|k| k.saturating_mul(1024))
}

// ---- HLS segments ------------------------------------------------------------------------

pub type RenewFuture = Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + Send>>;
/// Asks the extension for the stream again; returns the new URL of every file, in order.
pub type RenewFn = Arc<dyn Fn() -> RenewFuture + Send + Sync>;

#[derive(Clone)]
pub struct SegmentOptions {
    pub headers: Vec<(String, String)>,
    pub retry: RetryPolicy,
    pub limits: Arc<HostLimits>,
    pub renew: Option<RenewFn>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SegmentReport {
    pub fetched: usize,
    pub renewals: u32,
}

/// Where segment `index` is kept inside `dir`.
pub fn segment_path(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("seg{:05}.ts", index))
}

fn with_headers(mut request: RequestBuilder, headers: &[(String, String)]) -> RequestBuilder {
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
}

pub(super) fn retry_after_of(res: &Response) -> Option<Duration> {
    parse_retry_after(res.headers().get(RETRY_AFTER)?.to_str().ok()?, SystemTime::now())
}

/// Seconds since 1970 of an HTTP date (`Sun, 06 Nov 1994 08:49:37 GMT`).
fn http_date_secs(value: &str) -> Option<u64> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut parts = value.split_once(',')?.1.split_whitespace();
    let day: u64 = parts.next()?.parse().ok()?;
    let name = parts.next()?;
    let month = MONTHS.iter().position(|m| m.eq_ignore_ascii_case(name))? as u64 + 1;
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':').map(|p| p.parse::<u64>().ok());
    let (h, m, s) = (clock.next()??, clock.next()??, clock.next()??);
    // Days since 1970 from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let (era, yoe) = (y.div_euclid(400), y.rem_euclid(400) as u64);
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146097 + (yoe * 365 + yoe / 4 - yoe / 100 + doy) as i64 - 719468;
    Some(u64::try_from(days).ok()? * 86400 + h * 3600 + m * 60 + s)
}

/// A `Retry-After` value: whole seconds, or an HTTP date (the wait until then, none if past).
/// `backoff_delay` caps how long is waited.
pub fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = UNIX_EPOCH + Duration::from_secs(http_date_secs(value)?);
    Some(at.duration_since(now).unwrap_or_default())
}

fn header_text(res: &Response, name: reqwest::header::HeaderName) -> Option<String> {
    res.headers().get(name)?.to_str().ok().map(str::to_string)
}

/// One request for a whole small file, checked by `path`'s kind.
async fn attempt_file(client: &Client, url: &str, headers: &[(String, String)], path: &Path, encrypted: bool) -> Result<Vec<u8>, FetchError> {
    let request = async {
        let res = with_headers(client.get(url), headers).send().await.map_err(map_reqwest)?;
        if !res.status().is_success() {
            return Err(FetchError::Status { code: res.status().as_u16(), retry_after: retry_after_of(&res) });
        }
        let (len, kind) = (res.content_length(), header_text(&res, CONTENT_TYPE));
        let body = res.bytes().await.map_err(map_reqwest)?;
        check_file(path, encrypted, &body, len, kind.as_deref()).map_err(FetchError::Integrity)?;
        Ok(body.to_vec())
    };
    tokio::time::timeout(SEGMENT_TIMEOUT, request).await.unwrap_or(Err(FetchError::Timeout))
}

/// `attempt_file` with retries; failures that mean pressure lower the host's connection level.
async fn fetch_with_retry(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
    policy: &RetryPolicy,
    path: &Path,
    encrypted: bool,
    limits: Option<&HostLimits>,
) -> Result<Vec<u8>, FetchError> {
    let host = host_key(url).unwrap_or_default();
    let mut attempt = 0;
    loop {
        let started = limits.map_or(1, |l| l.limit(&host));
        let error = match attempt_file(client, url, headers, path, encrypted).await {
            Ok(body) => return Ok(body),
            Err(e) => e,
        };
        note_failure(url, &error);
        if let Some(limits) = limits.filter(|_| pressure_error(&error)) {
            limits.record_pressure(&host, started);
        }
        attempt += 1;
        if !is_retryable(&error) || attempt >= policy.max_tries {
            return Err(error);
        }
        wait_before_retry(policy, attempt - 1, &error).await;
    }
}

/// One segment, with retry and backoff; 403 and 404 come back at once (the link expired).
pub async fn fetch_segment(client: &Client, url: &str, headers: &[(String, String)], policy: &RetryPolicy) -> Result<Vec<u8>, FetchError> {
    fetch_with_retry(client, url, headers, policy, Path::new("segment"), false, None).await
}

async fn save_file(path: &Path, body: &[u8]) -> Result<(), FetchError> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    tokio::fs::write(&tmp, body).await.map_err(|e| io_error(&tmp, e))?;
    tokio::fs::rename(&tmp, path).await.map_err(|e| io_error(path, e))
}

/// One pass over the files that are not saved yet, a few at a time.
async fn fetch_pass(
    client: &Client,
    urls: &[String],
    paths: &[PathBuf],
    encrypted: &[bool],
    options: &SegmentOptions,
    meter: Option<&SpeedMeter>,
) -> Result<(), FetchError> {
    let is_encrypted = |i: usize| encrypted.get(i).copied().unwrap_or(false);
    let mut pending: VecDeque<usize> = (0..urls.len()).filter(|i| !saved_file_ok(&paths[*i], is_encrypted(*i))).collect();
    if let Some(meter) = meter {
        meter.set_items(urls.len() - pending.len(), urls.len());
    }
    // shortcut: the connection level follows the first file's host; split hosts share it.
    let host = pending.front().and_then(|i| host_key(&urls[*i])).unwrap_or_default();
    let mut running: JoinSet<Result<(), FetchError>> = JoinSet::new();
    loop {
        let limit = options.limits.limit(&host).clamp(1, MAX_SEGMENT_CONNECTIONS);
        while running.len() < limit {
            let Some(i) = pending.pop_front() else { break };
            let (client, url, path, options, meter, host) =
                (client.clone(), urls[i].clone(), paths[i].clone(), options.clone(), meter.cloned(), host.clone());
            let encrypted = is_encrypted(i);
            running.spawn(async move {
                let body = fetch_with_retry(&client, &url, &options.headers, &options.retry, &path, encrypted, Some(&options.limits)).await?;
                LIMITER.throttle(body.len() as u64).await;
                save_file(&path, &body).await?;
                options.limits.record_success(&host);
                if let Some(meter) = meter {
                    meter.record(body.len() as u64, Instant::now());
                    meter.item_done();
                }
                Ok(())
            });
        }
        match running.join_next().await {
            None => return Ok(()),
            Some(Ok(Ok(()))) => {}
            Some(Ok(Err(e))) => return Err(e),
            Some(Err(e)) => return Err(FetchError::Io(format!("A download task stopped: {}", e))),
        }
    }
}

/// The core of `fetch_segments`: any list of (URL, file) with optional progress.
pub(super) async fn fetch_files(
    client: &Client,
    urls: &[String],
    paths: &[PathBuf],
    encrypted: &[bool],
    options: &SegmentOptions,
    meter: Option<&SpeedMeter>,
) -> Result<SegmentReport, FetchError> {
    let mut urls = urls.to_vec();
    let mut renewals = 0;
    loop {
        match fetch_pass(client, &urls, paths, encrypted, options, meter).await {
            Ok(()) => return Ok(SegmentReport { fetched: urls.len(), renewals }),
            Err(FetchError::Status { code: 403, .. }) if renewals < MAX_RENEWALS && options.renew.is_some() => {
                renewals += 1;
                let renew = options.renew.as_ref().expect("checked above");
                urls = match renew().await {
                    Ok(fresh) if fresh.len() == urls.len() => {
                        // Keys are fetched again: the renewed stream may carry a new one.
                        for key in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "key")) {
                            let _ = tokio::fs::remove_file(key).await;
                        }
                        fresh
                    }
                    Ok(_) => return Err(FetchError::Integrity("The renewed stream is not the same as the one being downloaded.".to_string())),
                    Err(e) => return Err(FetchError::Integrity(format!("The stream link expired and could not be renewed: {}", e))),
                };
            }
            Err(e) => return Err(e),
        }
    }
}

/// Fetches `urls` in parallel into `dir` (`segment_path`), skipping the ones already saved, and
/// renews the links once a 403 shows they expired.
pub async fn fetch_segments(client: &Client, urls: &[String], dir: &Path, options: &SegmentOptions) -> Result<SegmentReport, FetchError> {
    let paths: Vec<PathBuf> = (0..urls.len()).map(|i| segment_path(dir, i)).collect();
    fetch_files(client, urls, &paths, &[], options, None).await
}

// ---- Direct files ------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DirectOptions {
    pub max_connections: usize,
    pub min_part_bytes: u64,
    pub retry: RetryPolicy,
    pub headers: Vec<(String, String)>,
}

impl Default for DirectOptions {
    fn default() -> Self {
        Self { max_connections: 4, min_part_bytes: 16 * 1024 * 1024, retry: RetryPolicy::APP, headers: Vec::new() }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DirectReport {
    pub connections: usize,
    pub resumed_from: u64,
}

/// What the server called the file when the partial copy was started.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Validator {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Validator {
    fn of(res: &Response) -> Self {
        Self { etag: header_text(res, ETAG), last_modified: header_text(res, LAST_MODIFIED) }
    }

    /// The `If-Range` value: a strong ETag, else the date.
    fn if_range(&self) -> Option<&str> {
        self.etag.as_deref().filter(|e| !e.starts_with("W/")).or(self.last_modified.as_deref())
    }
}

/// The note kept beside a partial file: the total size when known (the first line, as 1.4.x
/// wrote it) and the validator.
#[derive(Debug, Clone, Default, PartialEq)]
struct Note {
    total: Option<u64>,
    validator: Validator,
}

fn read_note(path: &Path) -> Option<Note> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut note = Note::default();
    for line in text.lines().map(str::trim) {
        if let Ok(total) = line.parse::<u64>() {
            note.total = Some(total);
        } else if let Some(v) = line.strip_prefix("etag: ") {
            note.validator.etag = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("last-modified: ") {
            note.validator.last_modified = Some(v.to_string());
        }
    }
    Some(note)
}

fn write_note(path: &Path, note: &Note) -> std::io::Result<()> {
    let mut text = note.total.map(|t| format!("{}\n", t)).unwrap_or_default();
    if let Some(v) = &note.validator.etag {
        text += &format!("etag: {}\n", v);
    }
    if let Some(v) = &note.validator.last_modified {
        text += &format!("last-modified: {}\n", v);
    }
    std::fs::write(path, text)
}

/// The partial copy of `target` while it downloads.
pub fn partial_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(PART_SUFFIX);
    PathBuf::from(name)
}

fn note_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(".validator");
    PathBuf::from(name)
}

pub fn write_validator(target: &Path, validator: &Validator) -> std::io::Result<()> {
    write_note(&note_path(target), &Note { total: None, validator: validator.clone() })
}

pub fn read_validator(target: &Path) -> Option<Validator> {
    read_note(&note_path(target)).map(|note| note.validator)
}

/// The file of part `index` of a ranged download of `partial`; parts are joined into `partial`
/// only at the end, so after a crash they hold all the bytes saved so far.
pub fn part_path(partial: &Path, index: usize) -> PathBuf {
    let name = partial.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let stem = name.strip_suffix(PART_SUFFIX).unwrap_or(&name);
    partial.with_file_name(format!("{}.p{}{}", stem, index, PART_SUFFIX))
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// `(start, total)` of a `Content-Range: bytes a-b/total` header.
fn content_range(res: &Response) -> Option<(u64, u64)> {
    super::parse_content_range(&header_text(res, CONTENT_RANGE)?)
}

async fn send(request: RequestBuilder) -> Result<Response, FetchError> {
    match tokio::time::timeout(STALL_TIMEOUT, request.send()).await {
        Ok(res) => res.map_err(map_reqwest),
        Err(_) => Err(FetchError::Timeout),
    }
}

fn status_error(res: &Response) -> FetchError {
    FetchError::Status { code: res.status().as_u16(), retry_after: retry_after_of(res) }
}

/// Writes the body of `res` to `path` (appending, or starting it over), with the speed limit and
/// the meter. The file is not opened until the first bytes have been looked at, so a body that
/// turns out to be a playlist or web page leaves an earlier partial copy alone. `sniff` looks at
/// the first bytes for that; `note` is written just before the file is opened.
async fn stream_body(
    res: &mut Response,
    path: &Path,
    append: bool,
    sniff: bool,
    meter: Option<&SpeedMeter>,
    note: Option<(&Path, Note)>,
) -> Result<u64, FetchError> {
    let mut file: Option<tokio::io::BufWriter<tokio::fs::File>> = None;
    let mut note = note;
    let mut written = 0u64;
    loop {
        let chunk = match tokio::time::timeout(STALL_TIMEOUT, res.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                if let Some(file) = file.as_mut() {
                    let _ = file.flush().await;
                }
                return Err(map_reqwest(e));
            }
            Err(_) => {
                if let Some(file) = file.as_mut() {
                    let _ = file.flush().await;
                }
                return Err(FetchError::Timeout);
            }
        };
        if sniff && written == 0 {
            match super::sniff_text_body(&chunk) {
                Some("hls") => return Err(FetchError::Manifest("hls")),
                Some("dash") => return Err(FetchError::Manifest("dash")),
                Some(_) => return Err(FetchError::Integrity("The stream host returned a web page instead of a video.".to_string())),
                None => {}
            }
        }
        if file.is_none() {
            if let Some((note_path, note)) = note.take() {
                write_note(note_path, &note).map_err(|e| io_error(note_path, e))?;
            }
            file = Some(open_partial(path, append).await?);
        }
        let file = file.as_mut().expect("opened above");
        file.write_all(&chunk).await.map_err(|e| io_error(path, e))?;
        written += chunk.len() as u64;
        if let Some(meter) = meter {
            meter.record(chunk.len() as u64, Instant::now());
        }
        LIMITER.throttle(chunk.len() as u64).await;
    }
    let mut file = match file {
        Some(file) => file,
        None => {
            if let Some((note_path, note)) = note.take() {
                write_note(note_path, &note).map_err(|e| io_error(note_path, e))?;
            }
            open_partial(path, append).await?
        }
    };
    file.flush().await.map_err(|e| io_error(path, e))?;
    Ok(written)
}

async fn open_partial(path: &Path, append: bool) -> Result<tokio::io::BufWriter<tokio::fs::File>, FetchError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let file = options.open(path).await.map_err(|e| io_error(path, e))?;
    Ok(tokio::io::BufWriter::with_capacity(1 << 20, file))
}

/// Streams `res` to a new partial file and notes its size and validator beside it.
async fn stream_from_start(mut res: Response, partial: &Path, note: &Path, total: Option<u64>, meter: Option<&SpeedMeter>) -> Result<(), FetchError> {
    let total = total.or_else(|| res.content_length().filter(|len| *len > 0));
    if let Some(meter) = meter {
        meter.set_bytes(0);
        meter.set_expected(total);
    }
    let note = Some((note, Note { total, validator: Validator::of(&res) }));
    let written = stream_body(&mut res, partial, false, true, meter, note).await?;
    finish_check(written, total)
}

fn finish_check(written: u64, total: Option<u64>) -> Result<(), FetchError> {
    if written == 0 {
        return Err(FetchError::Integrity("The stream host sent an empty response.".to_string()));
    }
    match total {
        Some(total) if total != written => Err(FetchError::Connect(format!("The download is incomplete: received {} of {} bytes.", written, total))),
        _ => Ok(()),
    }
}

/// Appends the part files in order to `partial`; a part that is short ends the copy, so what
/// is left is always a prefix of the file. The part files are removed.
async fn merge_parts(partial: &Path, parts: &[(PathBuf, u64)]) -> Result<(), FetchError> {
    let mut out = open_partial(partial, true).await?;
    let mut contiguous = true;
    for (path, expected) in parts {
        if contiguous {
            if let Ok(mut part) = tokio::fs::File::open(path).await {
                let copied = tokio::io::copy(&mut part, &mut out).await.map_err(|e| io_error(partial, e))?;
                contiguous = copied == *expected;
            } else {
                contiguous = false;
            }
        }
        let _ = tokio::fs::remove_file(path).await;
    }
    out.flush().await.map_err(|e| io_error(partial, e))
}

/// Fetches bytes `start..=end` of the file into `part`, resuming it after a dropped connection.
async fn fetch_part(client: Client, url: String, options: DirectOptions, if_range: Option<String>, part: PathBuf, range: (u64, u64), meter: Option<SpeedMeter>) -> Result<(), FetchError> {
    let (start, end) = range;
    let mut attempt = 0;
    loop {
        let have = file_len(&part);
        if start + have > end {
            return Ok(());
        }
        let mut request = with_headers(client.get(&url), &options.headers).header(RANGE, format!("bytes={}-{}", start + have, end));
        if let Some(v) = &if_range {
            request = request.header(IF_RANGE, v);
        }
        let result = async {
            let mut res = send(request).await?;
            match res.status().as_u16() {
                206 if content_range(&res).is_some_and(|(first, _)| first == start + have) => {}
                206 => return Err(FetchError::Integrity("The stream host sent a different part of the file than the one asked for.".to_string())),
                // The server sent the whole file: the file changed since the parts were planned.
                200 => return Err(FetchError::Changed),
                _ => return Err(status_error(&res)),
            }
            stream_body(&mut res, &part, true, false, meter.as_ref(), None).await.map(drop)
        }
        .await;
        let Err(error) = result else { continue };
        note_failure(&url, &error);
        if file_len(&part) > have {
            attempt = 0;
        }
        attempt += 1;
        if !is_retryable(&error) || attempt >= options.retry.max_tries {
            return Err(error);
        }
        wait_before_retry(&options.retry, attempt - 1, &error).await;
    }
}

/// Downloads `total` bytes over `count` connections, then joins the parts into `partial`. With
/// `keep`, part files an earlier run left are continued; returns how many bytes they held.
async fn download_in_parts(client: &Client, url: &str, partial: &Path, total: u64, count: u64, validator: &Validator, keep: bool, options: &DirectOptions, meter: Option<&SpeedMeter>) -> Result<u64, FetchError> {
    let size = total.div_ceil(count);
    let ranges: Vec<(u64, u64)> = (0..count).map(|i| (i * size, ((i + 1) * size).min(total) - 1)).collect();
    let parts: Vec<(PathBuf, u64)> = ranges.iter().enumerate().map(|(i, (s, e))| (part_path(partial, i), e - s + 1)).collect();
    let mut kept = 0;
    for (path, expected) in &parts {
        let len = file_len(path);
        // A part longer than its range is not this plan's: the join would copy the extra bytes.
        if keep && len <= *expected {
            kept += len;
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
    if let Some(meter) = meter {
        meter.set_bytes(kept);
    }
    let mut running = JoinSet::new();
    for (range, (path, _)) in ranges.iter().zip(&parts) {
        running.spawn(fetch_part(client.clone(), url.to_string(), options.clone(), validator.if_range().map(str::to_string), path.clone(), *range, meter.cloned()));
    }
    let mut failure = None;
    while let Some(done) = running.join_next().await {
        let result = done.unwrap_or_else(|e| Err(FetchError::Io(format!("A download task stopped: {}", e))));
        if let Err(e) = result {
            failure = Some(e);
            break;
        }
    }
    running.shutdown().await;
    merge_parts(partial, &parts).await?;
    failure.map_or(Ok(kept), Err)
}

/// One go at finishing the file; a `Restart` asks for a plain request from byte 0.
enum Pass {
    Done,
    Restart,
}

#[allow(clippy::too_many_arguments, reason = "the paths, options and progress of one download; a struct would only rename them")]
async fn direct_pass(client: &Client, url: &str, partial: &Path, note: &Path, options: &DirectOptions, meter: Option<&SpeedMeter>, plain: bool, report: &mut DirectReport) -> Result<Pass, FetchError> {
    let have = file_len(partial);
    let saved = read_note(note).unwrap_or_default();
    let request = with_headers(client.get(url), &options.headers);
    if have > 0 {
        let mut request = request.header(RANGE, format!("bytes={}-", have));
        if let Some(v) = saved.validator.if_range() {
            request = request.header(IF_RANGE, v);
        }
        let mut res = send(request).await?;
        return match res.status().as_u16() {
            206 => {
                let Some((start, total)) = content_range(&res).filter(|(s, t)| *s == have && saved.total.is_none_or(|n| n == *t)) else {
                    return Ok(Pass::Restart);
                };
                let validator = if saved.validator == Validator::default() { Validator::of(&res) } else { saved.validator };
                write_note(note, &Note { total: Some(total), validator }).map_err(|e| io_error(note, e))?;
                report.resumed_from = start;
                if let Some(meter) = meter {
                    meter.set_bytes(have);
                    meter.set_expected(Some(total));
                }
                let written = stream_body(&mut res, partial, true, false, meter, None).await?;
                finish_check(written + have, Some(total)).map(|_| Pass::Done)
            }
            // The server sent the whole file: it ignored the range, or the file changed.
            200 => {
                report.resumed_from = 0;
                stream_from_start(res, partial, note, None, meter).await.map(|_| Pass::Done)
            }
            416 => Ok(Pass::Restart),
            _ => Err(status_error(&res)),
        };
    }

    report.resumed_from = 0;
    // A first request asks for one byte to learn whether ranges work and how big the file is.
    let probe = if plain { request } else { request.header(RANGE, "bytes=0-0") };
    let res = send(probe).await?;
    match res.status().as_u16() {
        206 => {
            let total = content_range(&res).map(|(_, total)| total);
            let count = total.map_or(0, |t| (t / options.min_part_bytes.max(1)).min(options.max_connections as u64));
            match total {
                Some(total) if count >= 2 => {
                    let validator = Validator::of(&res);
                    // A run stopped while its parts downloaded (the app killed) left them, and an
                    // empty partial file. They are still this file's bytes when the size and an
                    // If-Range validator match the note; each part then asks only for the rest.
                    let keep = saved.total == Some(total) && validator.if_range().is_some() && saved.validator == validator;
                    write_note(note, &Note { total: Some(total), validator: validator.clone() }).map_err(|e| io_error(note, e))?;
                    if let Some(meter) = meter {
                        meter.set_expected(Some(total));
                    }
                    report.connections = count as usize;
                    match download_in_parts(client, url, partial, total, count, &validator, keep, options, meter).await {
                        Err(FetchError::Changed) => return Ok(Pass::Restart),
                        other => report.resumed_from = other?,
                    }
                    finish_check(file_len(partial), Some(total)).map(|_| Pass::Done)
                }
                // Ranges work but the file is small: one plain request.
                _ => {
                    let res = send(with_headers(client.get(url), &options.headers)).await?;
                    if !res.status().is_success() {
                        return Err(status_error(&res));
                    }
                    stream_from_start(res, partial, note, None, meter).await.map(|_| Pass::Done)
                }
            }
        }
        200 => stream_from_start(res, partial, note, None, meter).await.map(|_| Pass::Done),
        _ => Err(status_error(&res)),
    }
}

/// Downloads `url` into `partial`, keeping the size and validator in `note` beside it. Ranges
/// are used for big files and to resume what an earlier run left; a partial file with no note
/// (from 1.4.x) is resumed without `If-Range`.
pub(super) async fn download_partial(client: &Client, url: &str, partial: &Path, note: &Path, options: &DirectOptions, meter: Option<&SpeedMeter>) -> Result<DirectReport, FetchError> {
    let mut report = DirectReport { connections: 1, resumed_from: 0 };
    let (mut attempt, mut plain) = (0, false);
    loop {
        let have = file_len(partial);
        let error = match direct_pass(client, url, partial, note, options, meter, plain, &mut report).await {
            Ok(Pass::Done) => return Ok(report),
            Ok(Pass::Restart) => {
                let _ = std::fs::remove_file(partial);
                let _ = std::fs::remove_file(note);
                plain = true;
                continue;
            }
            Err(e) => e,
        };
        note_failure(url, &error);
        if file_len(partial) > have {
            attempt = 0;
        }
        attempt += 1;
        if !is_retryable(&error) || attempt >= options.retry.max_tries {
            return Err(error);
        }
        wait_before_retry(&options.retry, attempt - 1, &error).await;
    }
}

/// Downloads `url` to `target`: ranges for big files, a resume that checks the file did not change.
pub async fn download_direct(client: &Client, url: &str, target: &Path, options: &DirectOptions) -> Result<DirectReport, FetchError> {
    let (partial, note) = (partial_path(target), note_path(target));
    let report = download_partial(client, url, &partial, &note, options, None).await?;
    tokio::fs::rename(&partial, target).await.map_err(|e| io_error(target, e))?;
    let _ = tokio::fs::remove_file(&note).await;
    Ok(report)
}
