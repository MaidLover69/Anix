use crate::commands::player::sanitized_command;
use crate::db;
use crate::error::AppError;
use crate::models::downloads::{
    HEADROOM_BYTES, P1080_BYTES, P360_BYTES, P480_BYTES, P720_BYTES,
};
use crate::models::{DownloadRecord, SourcesResult, StreamItem, SubtitleItem};
use crate::providers::manager::ProviderManager;
use crate::state::AppState;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::ffi::OsString;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::{watch, Notify, OwnedSemaphorePermit, Semaphore};
use tracing::{error, info, warn};

/// Sent to stream hosts by both the direct downloader and ffmpeg.
pub const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:146.0) Gecko/20100101 Firefox/146.0";

/// Longest file name most Linux filesystems accept, in bytes.
pub const MAX_FILE_NAME_BYTES: usize = 255;
/// Budget for the part of a name shared by the video and its side files; the rest is left
/// for suffixes such as ` (2)`, a subtitle language and the extension.
const MAX_BASE_NAME_BYTES: usize = 180;
/// Suffix of every file a download is still working on.
pub const PART_SUFFIX: &str = ".anix-part";
/// The suffix working files had before the app was renamed; only cleaned up, never written.
pub const LEGACY_PART_SUFFIX: &str = ".anilili-part";

/// Queued downloads that run at the same time.
const MAX_QUEUE_DOWNLOADS: usize = 2;
const QUEUE_POLL_INTERVAL: Duration = Duration::from_secs(5);
const PROGRESS_WRITE_INTERVAL: Duration = Duration::from_secs(1);
/// A direct download with no data for this long is treated as dead.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const SUBTITLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_SUBTITLE_BYTES: usize = 10 * 1024 * 1024;
const STDERR_TAIL_BYTES: usize = 4096;
const ERROR_TAIL_CHARS: usize = 600;

/// The HTTP client used for stream bodies and subtitles: no limit on the whole request, so
/// a long body is not cut off.
fn stream_client() -> reqwest::Client {
    crate::clients::http::download_client()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageCheck {
    pub ok: bool,
    pub free_bytes: i64,
    pub needed_bytes: i64,
}

impl StorageCheck {
    pub fn new(ok: bool, free_bytes: i64, needed_bytes: i64) -> Self {
        Self {
            ok,
            free_bytes,
            needed_bytes,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Download directory
// ---------------------------------------------------------------------------------------

/// The `download_dir` setting as an absolute path, or `None` when it is unset or unusable.
/// A leading `~` stands for `home`.
pub fn configured_download_dir(settings: &serde_json::Value, home: Option<&Path>) -> Option<PathBuf> {
    let raw = settings.get("download_dir")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }

    let path = if raw == "~" {
        home?.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home?.join(rest)
    } else {
        PathBuf::from(raw)
    };

    path.is_absolute().then_some(path)
}

fn read_download_dir_setting() -> Option<PathBuf> {
    let settings_path = db::get_config_dir().ok()?.join("settings.json");
    let text = std::fs::read_to_string(settings_path).ok()?;
    let settings: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            warn!("settings.json could not be parsed, using the default download folder: {}", e);
            return None;
        }
    };
    configured_download_dir(&settings, dirs::home_dir().as_deref())
}

/// The folder downloads are saved in: the `download_dir` setting, or `downloads` inside the
/// app's data directory when the setting is empty. Reads and creates on disk, so async code
/// calls it through `spawn_blocking`.
pub fn get_download_dir() -> Result<PathBuf, AppError> {
    let dir = match read_download_dir_setting() {
        Some(dir) => dir,
        None => db::get_app_dir()?.join("downloads"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| {
        AppError::Other(format!(
            "Could not create the download folder {}: {}",
            dir.display(),
            e
        ))
    })?;
    Ok(dir)
}

pub fn check_storage(quality: &str) -> Result<StorageCheck, AppError> {
    let dir = get_download_dir()?;
    let needed_bytes = match quality {
        "1080p" => P1080_BYTES,
        "720p" => P720_BYTES,
        "480p" => P480_BYTES,
        "360p" => P360_BYTES,
        _ => P720_BYTES,
    };

    #[cfg(target_family = "unix")]
    let free_bytes = {
        use std::os::unix::ffi::OsStrExt;
        match std::ffi::CString::new(dir.as_os_str().as_bytes()) {
            Ok(c_path) => unsafe {
                let mut stat: libc::statvfs = std::mem::zeroed();
                if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                    (stat.f_bavail as i64).saturating_mul(stat.f_frsize as i64)
                } else {
                    10_000_000_000i64
                }
            },
            Err(_) => 10_000_000_000i64,
        }
    };

    #[cfg(not(target_family = "unix"))]
    let free_bytes = 10_000_000_000i64;

    let ok = free_bytes >= (needed_bytes + HEADROOM_BYTES);
    Ok(StorageCheck {
        ok,
        free_bytes,
        needed_bytes,
    })
}

// ---------------------------------------------------------------------------------------
// File names
// ---------------------------------------------------------------------------------------

/// The longest prefix of `s` that fits in `max_bytes` without splitting a character.
pub fn truncate_to_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Makes `name` safe as a single path component.
pub fn sanitize_filename(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            _ => c,
        })
        .collect();
    // No leading dot (hidden file, `.` or `..`) and no stray whitespace at either end.
    replaced
        .trim()
        .trim_start_matches('.')
        .trim_end_matches('.')
        .trim()
        .to_string()
}

/// `<title> - EP <n> [<CATEGORY> <quality>]`, at most 180 bytes. Category and quality are
/// part of the name so the sub and dub, or two qualities, of one episode get different
/// files. Only the title is shortened to make it fit.
pub fn output_base_name(series_title: &str, episode_num: f64, category: &str, quality: &str) -> String {
    let category = sanitize_filename(category).to_uppercase();
    let quality = sanitize_filename(quality);
    let mut tags: Vec<&str> = Vec::new();
    for tag in [truncate_to_bytes(&category, 16), truncate_to_bytes(&quality, 24)] {
        let tag = tag.trim();
        if !tag.is_empty() {
            tags.push(tag);
        }
    }

    let mut suffix = format!(" - EP {:02}", episode_num);
    if !tags.is_empty() {
        suffix.push_str(&format!(" [{}]", tags.join(" ")));
    }

    let title = sanitize_filename(series_title);
    let title = if title.is_empty() { "Untitled".to_string() } else { title };
    let title = truncate_to_bytes(&title, MAX_BASE_NAME_BYTES.saturating_sub(suffix.len())).trim_end();

    format!("{}{}", title, suffix)
}

/// A short stable tag derived from the download id; it keeps the working files of two
/// downloads of the same episode apart.
pub fn work_tag(download_id: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    download_id.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn work_file_name(base: &str, tag: &str, role: &str) -> String {
    format!("{}.{}.{}{}", base, tag, role, PART_SUFFIX)
}

/// Deletes working files (`*.anix-part`, and `*.anilili-part` from before the rename) left in `dir` by a run that was interrupted.
/// Returns how many were removed. Only call it while no download is running.
pub fn remove_stale_part_files(dir: &Path) -> usize {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_part = name.ends_with(PART_SUFFIX) || name.ends_with(LEGACY_PART_SUFFIX);
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if is_part && is_file && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

// ---------------------------------------------------------------------------------------
// What to download
// ---------------------------------------------------------------------------------------

/// A resolved stream plus everything needed to request it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DownloadSource {
    pub url: String,
    /// `"hls"`, `"dash"`, `"mp4"`, ... when the provider said so.
    pub stream_type: Option<String>,
    pub referer: Option<String>,
    pub origin: Option<String>,
    pub headers: HashMap<String, String>,
    pub subtitles: Vec<SubtitleItem>,
}

impl DownloadSource {
    pub fn from_stream(stream: &StreamItem, subtitles: Vec<SubtitleItem>) -> Self {
        Self {
            url: stream.url.clone(),
            stream_type: Some(stream.stream_type.clone()),
            referer: stream.referer.clone(),
            origin: stream.origin.clone(),
            headers: stream.headers.clone().unwrap_or_default(),
            subtitles,
        }
    }
}

/// Accepts only absolute `http`/`https` URLs. Anything else (`file:`, `concat:`, a value
/// starting with `-`, ...) never reaches ffmpeg or the HTTP client.
pub fn validate_stream_url(raw: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(raw.trim()).map_err(|e| format!("Invalid stream URL: {}", e))?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(parsed.to_string()),
        scheme => Err(format!(
            "Unsupported stream URL scheme \"{}\": only http and https can be downloaded",
            scheme
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHeaders {
    pub user_agent: String,
    /// Every other header, in a stable order.
    pub headers: Vec<(String, String)>,
}

fn clean_header_value(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

fn is_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// The headers a stream host expects: Referer, Origin (the explicit one, else the referer's
/// origin), the provider's custom headers, and a browser User-Agent unless the provider
/// supplies its own. Values are stripped of control characters so none can add a header.
pub fn request_headers(source: &DownloadSource) -> RequestHeaders {
    let mut user_agent = BROWSER_USER_AGENT.to_string();
    let mut headers: Vec<(String, String)> = Vec::new();

    let referer = source.referer.as_deref().map(clean_header_value).filter(|v| !v.is_empty());
    let origin = source
        .origin
        .as_deref()
        .map(clean_header_value)
        .filter(|v| !v.is_empty())
        .or_else(|| {
            let parsed = reqwest::Url::parse(referer.as_deref()?).ok()?;
            let origin = parsed.origin();
            origin.is_tuple().then(|| origin.ascii_serialization())
        });

    if let Some(referer) = referer {
        headers.push(("Referer".to_string(), referer));
    }
    if let Some(origin) = origin {
        headers.push(("Origin".to_string(), origin));
    }

    let mut custom: Vec<(&String, &String)> = source.headers.iter().collect();
    custom.sort();
    for (name, value) in custom {
        let name = name.trim();
        let value = clean_header_value(value);
        if !is_header_name(name) || value.is_empty() {
            continue;
        }
        if name.eq_ignore_ascii_case("user-agent") {
            user_agent = value;
        } else if let Some(existing) = headers.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            existing.1 = value;
        } else {
            headers.push((name.to_string(), value));
        }
    }

    RequestHeaders { user_agent, headers }
}

fn apply_headers(mut req: reqwest::RequestBuilder, headers: &RequestHeaders) -> reqwest::RequestBuilder {
    req = req.header("User-Agent", &headers.user_agent);
    for (name, value) in &headers.headers {
        req = req.header(name, value);
    }
    req
}

fn quality_height(quality: &str) -> Option<u32> {
    let digits: String = quality
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().ok().filter(|h| *h >= 100)
}

/// Chooses the stream for a wanted quality such as `"1080p"`, `"720p"`, `"best"` or
/// `"auto"`: an exact match, else the best one below it, else the smallest one above it.
/// Without a numeric wish the highest quality wins. Streams that do not state a quality
/// are used when nothing better is known.
pub fn pick_stream<'a>(streams: &'a [StreamItem], wanted_quality: &str) -> Option<&'a StreamItem> {
    let rated: Vec<(u32, &StreamItem)> = streams
        .iter()
        .filter_map(|s| quality_height(s.quality.as_deref().unwrap_or("")).map(|h| (h, s)))
        .collect();

    if rated.is_empty() {
        return streams.first();
    }

    let best = rated.iter().max_by_key(|(h, _)| *h).map(|(_, s)| *s);
    match quality_height(wanted_quality) {
        None => best,
        Some(wanted) => rated
            .iter()
            .filter(|(h, _)| *h <= wanted)
            .max_by_key(|(h, _)| *h)
            .or_else(|| rated.iter().min_by_key(|(h, _)| *h))
            .map(|(_, s)| *s),
    }
}

// ---------------------------------------------------------------------------------------
// Subtitles
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    WebVtt,
    Ass,
    Srt,
}

impl SubtitleFormat {
    pub fn extension(self) -> &'static str {
        match self {
            SubtitleFormat::WebVtt => "vtt",
            SubtitleFormat::Ass => "ass",
            SubtitleFormat::Srt => "srt",
        }
    }

    fn ffmpeg_demuxer(self) -> &'static str {
        match self {
            SubtitleFormat::WebVtt => "webvtt",
            SubtitleFormat::Ass => "ass",
            SubtitleFormat::Srt => "srt",
        }
    }

    /// Codec of the subtitle track inside the MKV.
    fn mkv_codec(self) -> &'static str {
        match self {
            SubtitleFormat::Ass => "ass",
            SubtitleFormat::WebVtt | SubtitleFormat::Srt => "srt",
        }
    }
}

/// Recognises a subtitle file by its content, whatever the server or the provider called
/// it. Returns `None` for anything else, in particular an HTML error page.
pub fn detect_subtitle_format(body: &[u8]) -> Option<SubtitleFormat> {
    if body.is_empty() || body.len() > MAX_SUBTITLE_BYTES || body.iter().take(4096).any(|b| *b == 0) {
        return None;
    }

    let text = String::from_utf8_lossy(body);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with("WEBVTT") {
        return Some(SubtitleFormat::WebVtt);
    }
    if text.starts_with('<') {
        return None;
    }
    if text.contains("[Script Info]") || text.contains("[V4+ Styles]") || text.contains("[Events]") {
        return Some(SubtitleFormat::Ass);
    }

    // An SRT cue: "00:00:01,000 --> 00:00:04,000".
    let has_cue = text.lines().take(400).any(|line| {
        line.split_once("-->").is_some_and(|(start, end)| {
            let is_time = |s: &str| {
                let s = s.trim();
                s.contains(':') && s.chars().next().is_some_and(|c| c.is_ascii_digit())
            };
            is_time(start) && is_time(end)
        })
    });
    has_cue.then_some(SubtitleFormat::Srt)
}

/// A subtitle file that was fetched and recognised.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleInput {
    pub path: PathBuf,
    pub format: SubtitleFormat,
    pub language: String,
}

// ---------------------------------------------------------------------------------------
// ffmpeg
// ---------------------------------------------------------------------------------------

/// Options of ffmpeg's HLS demuxer that the installed ffmpeg understands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HlsDemuxerOptions {
    pub allowed_extensions: bool,
    pub extension_picky: bool,
}

/// Reads the output of `ffmpeg -h demuxer=hls`.
pub fn parse_hls_demuxer_help(help: &str) -> HlsDemuxerOptions {
    let has = |option: &str| {
        help.lines()
            .any(|line| line.trim_start().split_whitespace().next() == Some(option))
    };
    HlsDemuxerOptions {
        allowed_extensions: has("-allowed_extensions"),
        extension_picky: has("-extension_picky"),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FfmpegInput {
    /// A stream on the network. `hls` carries the demuxer options to relax, when the stream
    /// is an HLS playlist.
    Remote {
        url: String,
        headers: RequestHeaders,
        hls: Option<HlsDemuxerOptions>,
    },
    /// A file already on disk. Must be an absolute path.
    Local(PathBuf),
}

/// The complete ffmpeg argument list for copying `input` (and optionally one subtitle
/// file) into a Matroska file at `output`.
///
/// The URL and every path are values of `-i` or the final output operand of an absolute
/// path, so none of them can be read as an option.
pub fn build_ffmpeg_args(
    input: &FfmpegInput,
    subtitle: Option<&SubtitleInput>,
    output: &Path,
) -> Vec<OsString> {
    fn text<const N: usize>(args: &mut Vec<OsString>, values: [&str; N]) {
        args.extend(values.into_iter().map(OsString::from));
    }

    let mut args: Vec<OsString> = Vec::new();
    // Warnings are kept because ffmpeg reports the HTTP status of a refused segment as one.
    text(&mut args, ["-hide_banner", "-nostdin", "-loglevel", "warning", "-y"]);
    text(&mut args, ["-nostats", "-progress", "pipe:1"]);

    match input {
        FfmpegInput::Remote { url, headers, hls } => {
            text(&mut args, ["-user_agent", &headers.user_agent]);
            if !headers.headers.is_empty() {
                let block: String = headers
                    .headers
                    .iter()
                    .map(|(name, value)| format!("{}: {}\r\n", name, value))
                    .collect();
                text(&mut args, ["-headers", &block]);
            }
            // Microseconds without data before ffmpeg gives up on a connection.
            text(&mut args, ["-rw_timeout", "60000000"]);
            if let Some(options) = hls {
                // Stream hosts often give segments unrelated extensions such as .jpg.
                if options.allowed_extensions {
                    text(&mut args, ["-allowed_extensions", "ALL"]);
                }
                if options.extension_picky {
                    text(&mut args, ["-extension_picky", "0"]);
                }
            }
            text(&mut args, ["-i", url]);
        }
        FfmpegInput::Local(path) => {
            text(&mut args, ["-i"]);
            args.push(path.as_os_str().to_os_string());
        }
    }

    if let Some(sub) = subtitle {
        text(&mut args, ["-f", sub.format.ffmpeg_demuxer(), "-i"]);
        args.push(sub.path.as_os_str().to_os_string());
    }

    text(&mut args, ["-c", "copy"]);
    if let Some(sub) = subtitle {
        text(&mut args, ["-c:s", sub.format.mkv_codec()]);
        let language = clean_header_value(&sub.language);
        if !language.is_empty() {
            text(&mut args, ["-metadata:s:s:0", &format!("title={}", language)]);
        }
    }
    text(&mut args, ["-f", "matroska"]);
    args.push(output.as_os_str().to_os_string());

    args
}

#[derive(Debug, Clone, PartialEq)]
enum FfmpegError {
    NotInstalled,
    Launch(String),
    Failed { code: Option<i32>, stderr_tail: String },
}

impl std::fmt::Display for FfmpegError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfmpegError::NotInstalled => write!(
                f,
                "ffmpeg is not installed. Install the ffmpeg package to download this stream."
            ),
            FfmpegError::Launch(e) => write!(f, "ffmpeg could not be started: {}", e),
            FfmpegError::Failed { code, stderr_tail } => {
                match code {
                    Some(code) => write!(f, "ffmpeg failed (exit code {})", code)?,
                    None => write!(f, "ffmpeg was terminated")?,
                }
                if stderr_tail.is_empty() {
                    Ok(())
                } else {
                    write!(f, ": {}", stderr_tail)
                }
            }
        }
    }
}

fn ffmpeg_command() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::from(sanitized_command("ffmpeg"));
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    cmd
}

fn launch_error(e: std::io::Error) -> FfmpegError {
    if e.kind() == std::io::ErrorKind::NotFound {
        FfmpegError::NotInstalled
    } else {
        FfmpegError::Launch(e.to_string())
    }
}

static HLS_OPTIONS: Lazy<Mutex<Option<HlsDemuxerOptions>>> = Lazy::new(|| Mutex::new(None));

/// Checks that ffmpeg can be run and which HLS options it has. Only a success is cached,
/// so installing ffmpeg while the app is open is noticed.
async fn probe_ffmpeg() -> Result<HlsDemuxerOptions, FfmpegError> {
    if let Some(options) = *lock(&HLS_OPTIONS) {
        return Ok(options);
    }

    let output = ffmpeg_command()
        .args(["-hide_banner", "-h", "demuxer=hls"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(launch_error)?;

    let options = parse_hls_demuxer_help(&String::from_utf8_lossy(&output.stdout));
    *lock(&HLS_OPTIONS) = Some(options);
    Ok(options)
}

/// The end of ffmpeg's stderr as one line, short enough for the downloads list.
fn summarize_stderr(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");
    let chars: Vec<char> = joined.chars().collect();
    if chars.len() > ERROR_TAIL_CHARS {
        let tail: String = chars[chars.len() - ERROR_TAIL_CHARS..].iter().collect();
        format!("...{}", tail)
    } else {
        joined
    }
}

/// Throttled writer of `progress` and `file_size` for one download.
struct ProgressReporter<'a> {
    pool: &'a SqlitePool,
    id: &'a str,
    last_write: Option<Instant>,
}

impl<'a> ProgressReporter<'a> {
    fn new(pool: &'a SqlitePool, id: &'a str) -> Self {
        Self { pool, id, last_write: None }
    }

    /// Writes at most about once a second; calls in between are dropped.
    async fn report(&mut self, progress: Option<f64>, file_size: Option<i64>) {
        if self.last_write.is_some_and(|t| t.elapsed() < PROGRESS_WRITE_INTERVAL) {
            return;
        }
        self.last_write = Some(Instant::now());
        if let Err(e) = db::update_download_progress(self.pool, self.id, progress, file_size).await {
            warn!("Could not save download progress for {}: {}", self.id, e);
        }
    }
}

/// Runs ffmpeg to completion. Progress lines on its stdout are turned into throttled
/// database updates; `duration_s`, when known, makes them a fraction between `from` and
/// `to`. Dropping the returned future kills ffmpeg.
async fn run_ffmpeg(
    args: Vec<OsString>,
    reporter: &mut ProgressReporter<'_>,
    duration_s: Option<f64>,
    (from, to): (f64, f64),
) -> Result<(), FfmpegError> {
    let mut child = ffmpeg_command()
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(launch_error)?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let read_stderr = async {
        let mut tail: Vec<u8> = Vec::new();
        if let Some(mut stderr) = stderr {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > STDERR_TAIL_BYTES * 2 {
                    tail.drain(..tail.len() - STDERR_TAIL_BYTES);
                }
            }
        }
        tail
    };

    let read_progress = async {
        let Some(stdout) = stdout else { return };
        let mut lines = BufReader::new(stdout).lines();
        let mut out_time_us: Option<f64> = None;
        let mut total_size: Option<i64> = None;
        while let Ok(Some(line)) = lines.next_line().await {
            let Some((key, value)) = line.split_once('=') else { continue };
            match key.trim() {
                "out_time_us" => out_time_us = value.trim().parse::<f64>().ok().or(out_time_us),
                "total_size" => total_size = value.trim().parse::<i64>().ok().or(total_size),
                "progress" => {
                    let fraction = match (out_time_us, duration_s) {
                        (Some(us), Some(d)) if d > 0.0 => {
                            Some(from + (to - from) * (us / 1_000_000.0 / d).clamp(0.0, 1.0))
                        }
                        _ => None,
                    };
                    reporter.report(fraction, total_size).await;
                }
                _ => {}
            }
        }
    };

    let (status, stderr_tail, ()) = tokio::join!(child.wait(), read_stderr, read_progress);

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(FfmpegError::Failed {
            code: status.code(),
            stderr_tail: summarize_stderr(&stderr_tail),
        }),
        Err(e) => Err(FfmpegError::Launch(e.to_string())),
    }
}

// ---------------------------------------------------------------------------------------
// One download
// ---------------------------------------------------------------------------------------

/// How a download ended.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadOutcome {
    /// The file is at `path`; the row is `COMPLETED`.
    Completed { path: PathBuf, size: i64 },
    /// The row is `FAILED` and its `error_msg` is this text.
    Failed(String),
    /// Stopped on request; working files are gone and the row was left alone.
    Cancelled,
}

struct Finished {
    path: PathBuf,
    size: i64,
}

/// Files a download has created and not yet handed over as results.
#[derive(Clone, Default)]
struct WorkFiles(Arc<Mutex<Vec<PathBuf>>>);

impl WorkFiles {
    fn track(&self, path: &Path) {
        lock(&self.0).push(path.to_path_buf());
    }

    fn release(&self, path: &Path) {
        lock(&self.0).retain(|p| p != path);
    }

    async fn remove_all(&self) {
        let paths: Vec<PathBuf> = std::mem::take(&mut *lock(&self.0));
        for path in paths {
            let _ = tokio::fs::remove_file(&path).await;
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Job<'a> {
    pool: &'a SqlitePool,
    record: &'a DownloadRecord,
    dir: PathBuf,
    base: String,
    tag: String,
    work: &'a WorkFiles,
}

impl Job<'_> {
    /// Path of a working file; it is removed again unless the download hands it over.
    fn work_path(&self, role: &str) -> PathBuf {
        let path = self.dir.join(work_file_name(&self.base, &self.tag, role));
        self.work.track(&path);
        path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Hls,
    Dash,
    File,
}

fn classify_stream(url: &str, stream_type: Option<&str>) -> StreamKind {
    let url = url.to_ascii_lowercase();
    let stream_type = stream_type.unwrap_or("").to_ascii_lowercase();
    if stream_type == "hls" || url.contains(".m3u8") {
        StreamKind::Hls
    } else if stream_type == "dash" || url.contains(".mpd") {
        StreamKind::Dash
    } else {
        StreamKind::File
    }
}

/// What the first bytes of a response body are, when they are not a media file.
fn sniff_text_body(head: &[u8]) -> Option<&'static str> {
    let text = String::from_utf8_lossy(&head[..head.len().min(512)]);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let lower = text.to_ascii_lowercase();
    if text.starts_with("#EXTM3U") {
        Some("hls")
    } else if lower.starts_with("<?xml") && lower.contains("<mpd") || lower.starts_with("<mpd") {
        Some("dash")
    } else if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        Some("html")
    } else {
        None
    }
}

fn container_extension(url: &str) -> &'static str {
    let path = reqwest::Url::parse(url)
        .map(|u| u.path().to_ascii_lowercase())
        .unwrap_or_default();
    for ext in ["mkv", "webm", "mov", "m4v", "avi", "flv", "ts"] {
        if path.ends_with(&format!(".{}", ext)) {
            return ext;
        }
    }
    "mp4"
}

/// Fetches the subtitle files with the stream's headers. A subtitle that cannot be
/// fetched, is not a 2xx response, or is not a subtitle file is skipped with a log line;
/// it never fails the download. The default subtitle comes first.
async fn fetch_subtitles(
    job: &Job<'_>,
    client: &reqwest::Client,
    headers: &RequestHeaders,
    subtitles: &[SubtitleItem],
) -> Vec<SubtitleInput> {
    let mut ordered: Vec<&SubtitleItem> = subtitles.iter().collect();
    ordered.sort_by_key(|s| s.is_default != Some(true));

    let mut fetched = Vec::new();
    for (idx, sub) in ordered.into_iter().enumerate() {
        let body = async {
            let url = validate_stream_url(&sub.url)?;
            let res = apply_headers(client.get(&url), headers)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !res.status().is_success() {
                return Err(format!("HTTP status {}", res.status()));
            }
            if res.content_length().is_some_and(|len| len > MAX_SUBTITLE_BYTES as u64) {
                return Err("file is too large to be a subtitle".to_string());
            }
            res.bytes().await.map_err(|e| e.to_string())
        };

        let body = match tokio::time::timeout(SUBTITLE_TIMEOUT, body).await {
            Ok(Ok(body)) => body,
            Ok(Err(e)) => {
                warn!("Skipping subtitle \"{}\" of download {}: {}", sub.language, job.record.id, e);
                continue;
            }
            Err(_) => {
                warn!("Skipping subtitle \"{}\" of download {}: timed out", sub.language, job.record.id);
                continue;
            }
        };

        let Some(format) = detect_subtitle_format(&body) else {
            warn!(
                "Skipping subtitle \"{}\" of download {}: the response is not a subtitle file",
                sub.language, job.record.id
            );
            continue;
        };

        let path = job.work_path(&format!("sub{}", idx));
        if let Err(e) = tokio::fs::write(&path, &body).await {
            warn!("Could not save subtitle \"{}\" of download {}: {}", sub.language, job.record.id, e);
            let _ = tokio::fs::remove_file(&path).await;
            job.work.release(&path);
            continue;
        }

        fetched.push(SubtitleInput {
            path,
            format,
            language: sub.language.clone(),
        });
    }
    fetched
}

/// Muxes `input` into a working MKV with ffmpeg, embedding the first subtitle. If ffmpeg
/// fails while a subtitle is attached it is run once more without it, so a subtitle ffmpeg
/// cannot read does not cost the video. Returns the working file and whether the subtitle
/// went in.
async fn mux_with_ffmpeg(
    job: &Job<'_>,
    input: &FfmpegInput,
    subtitle: Option<&SubtitleInput>,
    reporter: &mut ProgressReporter<'_>,
    progress_range: (f64, f64),
) -> Result<(PathBuf, bool), FfmpegError> {
    let output = job.work_path("video");
    let mut subtitle = subtitle;

    loop {
        let args = build_ffmpeg_args(input, subtitle, &output);
        match run_ffmpeg(args, reporter, job.record.duration_s, progress_range).await {
            Ok(()) => return Ok((output, subtitle.is_some())),
            Err(FfmpegError::Failed { code, stderr_tail }) if subtitle.is_some() => {
                warn!(
                    "ffmpeg failed for download {} with a subtitle attached (exit code {:?}: {}); retrying without it",
                    job.record.id, code, stderr_tail
                );
                subtitle = None;
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&output).await;
                job.work.release(&output);
                return Err(e);
            }
        }
    }
}

/// Claims a free name `<base>.<ext>` (or `<base> (2).<ext>`, ...) in the download folder
/// by creating it, so two downloads finishing together cannot pick the same one and an
/// existing file is never overwritten. Returns the path and its stem.
async fn reserve_final_path(job: &Job<'_>, ext: &str) -> Result<(PathBuf, String), String> {
    for n in 1..=999u32 {
        let stem = if n == 1 {
            job.base.clone()
        } else {
            format!("{} ({})", job.base, n)
        };
        let path = job.dir.join(format!("{}.{}", stem, ext));
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(_) => {
                // Tracked only now: before this point the name may belong to another file.
                job.work.track(&path);
                return Ok((path, stem));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("Could not create {}: {}", path.display(), e)),
        }
    }
    Err(format!(
        "Too many files named \"{}\" in {}",
        job.base,
        job.dir.display()
    ))
}

/// Moves the finished video to its final name and keeps the subtitles that are not inside
/// it as side files next to it.
async fn finalize(
    job: &Job<'_>,
    video: &Path,
    ext: &str,
    subtitles: &[SubtitleInput],
    embedded: Option<usize>,
) -> Result<Finished, String> {
    let size = match tokio::fs::metadata(video).await {
        Ok(meta) if meta.len() > 0 => meta.len() as i64,
        Ok(_) => return Err("The download produced an empty file.".to_string()),
        Err(e) => return Err(format!("The downloaded file is missing: {}", e)),
    };

    let (final_path, stem) = reserve_final_path(job, ext).await?;
    tokio::fs::rename(video, &final_path)
        .await
        .map_err(|e| format!("Could not move the download to {}: {}", final_path.display(), e))?;
    job.work.release(video);
    job.work.release(&final_path);

    let mut used: Vec<String> = Vec::new();
    for (idx, sub) in subtitles.iter().enumerate() {
        if embedded == Some(idx) {
            continue;
        }
        let language = sanitize_filename(&sub.language).to_lowercase().replace(' ', "_");
        let mut label = truncate_to_bytes(&language, 24).to_string();
        if label.is_empty() {
            label = "sub".to_string();
        }
        if used.contains(&label) {
            label = format!("{}-{}", label, idx + 1);
        }
        let side = job.dir.join(format!("{}.{}.{}", stem, label, sub.format.extension()));
        match tokio::fs::rename(&sub.path, &side).await {
            Ok(()) => {
                job.work.release(&sub.path);
                used.push(label);
            }
            Err(e) => warn!("Could not keep subtitle {}: {}", side.display(), e),
        }
    }

    Ok(Finished { path: final_path, size })
}

async fn download_stream_with_ffmpeg(
    job: &Job<'_>,
    url: &str,
    headers: &RequestHeaders,
    kind: StreamKind,
    subtitles: &[SubtitleInput],
) -> Result<Finished, String> {
    let hls_options = probe_ffmpeg().await.map_err(|e| e.to_string())?;
    let input = FfmpegInput::Remote {
        url: url.to_string(),
        headers: headers.clone(),
        hls: (kind == StreamKind::Hls).then_some(hls_options),
    };

    let mut reporter = ProgressReporter::new(job.pool, &job.record.id);
    let (video, embedded) = mux_with_ffmpeg(job, &input, subtitles.first(), &mut reporter, (0.05, 0.99))
        .await
        .map_err(|e| e.to_string())?;

    finalize(job, &video, "mkv", subtitles, embedded.then_some(0)).await
}

async fn download_file(
    job: &Job<'_>,
    client: &reqwest::Client,
    url: &str,
    headers: &RequestHeaders,
    subtitles: &[SubtitleInput],
) -> Result<Finished, String> {
    let request = apply_headers(client.get(url), headers);
    let mut res = match tokio::time::timeout(STALL_TIMEOUT, request.send()).await {
        Ok(Ok(res)) => res,
        Ok(Err(e)) => return Err(format!("Could not reach the stream host: {}", e)),
        Err(_) => return Err("The stream host did not answer in time.".to_string()),
    };

    if !res.status().is_success() {
        return Err(format!("The stream host answered HTTP {}.", res.status()));
    }

    let expected = res.content_length().filter(|len| *len > 0);
    let temp = job.work_path("download");
    let file = tokio::fs::File::create(&temp)
        .await
        .map_err(|e| format!("Could not create {}: {}", temp.display(), e))?;
    let mut file = BufWriter::with_capacity(1 << 20, file);
    let write_error = |e: std::io::Error| format!("Could not write to {}: {}", temp.display(), e);

    let mut reporter = ProgressReporter::new(job.pool, &job.record.id);
    let mut downloaded: u64 = 0;
    loop {
        let chunk = match tokio::time::timeout(STALL_TIMEOUT, res.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                return Err(format!(
                    "The download was interrupted after {} bytes: {}",
                    downloaded, e
                ))
            }
            Err(_) => {
                return Err(format!(
                    "The download stalled after {} bytes: no data for {} seconds.",
                    downloaded,
                    STALL_TIMEOUT.as_secs()
                ))
            }
        };

        if downloaded == 0 {
            match sniff_text_body(&chunk) {
                Some(manifest @ ("hls" | "dash")) => {
                    // A playlist, not a media file: hand the URL to ffmpeg instead of
                    // saving the playlist text.
                    drop(file);
                    let _ = tokio::fs::remove_file(&temp).await;
                    job.work.release(&temp);
                    let kind = if manifest == "hls" { StreamKind::Hls } else { StreamKind::Dash };
                    return download_stream_with_ffmpeg(job, url, headers, kind, subtitles).await;
                }
                Some(_) => {
                    return Err("The stream host returned a web page instead of a video.".to_string())
                }
                None => {}
            }
        }

        file.write_all(&chunk).await.map_err(write_error)?;
        downloaded += chunk.len() as u64;

        let fraction = expected.map(|total| 0.05 + 0.90 * (downloaded as f64 / total as f64).min(1.0));
        reporter.report(fraction, Some(downloaded as i64)).await;
    }

    file.flush().await.map_err(write_error)?;
    drop(file);

    if downloaded == 0 {
        return Err("The stream host sent an empty response.".to_string());
    }
    if let Some(total) = expected {
        if downloaded != total {
            return Err(format!(
                "The download is incomplete: received {} of {} bytes.",
                downloaded, total
            ));
        }
    }

    // Remux into MKV with the subtitle inside when ffmpeg is available; otherwise keep the
    // file exactly as it was served.
    let mut reporter = ProgressReporter::new(job.pool, &job.record.id);
    let input = FfmpegInput::Local(temp.clone());
    match mux_with_ffmpeg(job, &input, subtitles.first(), &mut reporter, (0.95, 0.99)).await {
        Ok((video, embedded)) => {
            let finished = finalize(job, &video, "mkv", subtitles, embedded.then_some(0)).await?;
            let _ = tokio::fs::remove_file(&temp).await;
            job.work.release(&temp);
            Ok(finished)
        }
        Err(e) => {
            match e {
                FfmpegError::NotInstalled => {
                    info!("ffmpeg is not installed; keeping download {} in its original container", job.record.id)
                }
                other => warn!("Keeping download {} in its original container: {}", job.record.id, other),
            }
            finalize(job, &temp, container_extension(url), subtitles, None).await
        }
    }
}

async fn transfer<S>(
    pool: &SqlitePool,
    record: &DownloadRecord,
    source: S,
    dir: Option<PathBuf>,
    work: &WorkFiles,
) -> Result<Finished, String>
where
    S: Future<Output = Result<DownloadSource, String>>,
{
    let source = source.await?;
    let url = validate_stream_url(&source.url)?;

    let dir = match dir {
        Some(dir) => dir,
        None => tokio::task::spawn_blocking(get_download_dir)
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?,
    };
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Could not create the download folder {}: {}", dir.display(), e))?;

    let job = Job {
        pool,
        record,
        dir,
        base: output_base_name(&record.series_title, record.episode_num, &record.category, &record.quality),
        tag: work_tag(&record.id),
        work,
    };

    if let Err(e) = db::update_download_progress(pool, &record.id, Some(0.05), None).await {
        warn!("Could not save download progress for {}: {}", record.id, e);
    }

    let client = stream_client();
    let headers = request_headers(&source);
    let subtitles = fetch_subtitles(&job, &client, &headers, &source.subtitles).await;

    match classify_stream(&url, source.stream_type.as_deref()) {
        StreamKind::File => download_file(&job, &client, &url, &headers, &subtitles).await,
        kind => download_stream_with_ffmpeg(&job, &url, &headers, kind, &subtitles).await,
    }
}

async fn cancelled(mut cancel: watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            // Nobody can cancel any more.
            std::future::pending::<()>().await;
        }
    }
}

/// Runs one download from source resolution to the final row update. When `cancel` turns
/// true the work is dropped (which kills ffmpeg) and the working files are removed.
async fn run_job<S>(
    pool: &SqlitePool,
    record: &DownloadRecord,
    source: S,
    dir: Option<PathBuf>,
    cancel: watch::Receiver<bool>,
) -> DownloadOutcome
where
    S: Future<Output = Result<DownloadSource, String>>,
{
    let id = record.id.as_str();

    // A row deleted before the job got going means the download is no longer wanted.
    if matches!(db::get_download_by_id(pool, id).await, Ok(None)) {
        return DownloadOutcome::Cancelled;
    }
    if let Err(e) = db::update_download_status(pool, id, db::STATUS_DOWNLOADING, Some(0.0), None, None, None, None).await {
        warn!("Could not mark download {} as started: {}", id, e);
    }

    let work = WorkFiles::default();
    let result = tokio::select! {
        result = transfer(pool, record, source, dir, &work) => Some(result),
        _ = cancelled(cancel) => None,
    };
    // Nothing of the transfer is running any more, so its leftovers can go.
    work.remove_all().await;

    match result {
        None => {
            info!("Download {} was cancelled", id);
            DownloadOutcome::Cancelled
        }
        Some(Ok(finished)) => {
            let path = finished.path.to_string_lossy().to_string();
            if let Err(e) = db::update_download_status(
                pool,
                id,
                db::STATUS_COMPLETED,
                Some(1.0),
                Some(&path),
                Some(finished.size),
                None,
                None,
            )
            .await
            {
                error!("Download {} finished but its row could not be updated: {}", id, e);
            }
            info!("Download {} completed: {}", id, path);
            DownloadOutcome::Completed { path: finished.path, size: finished.size }
        }
        Some(Err(message)) => {
            warn!("Download {} failed: {}", id, message);
            if let Err(e) = db::update_download_status(
                pool,
                id,
                db::STATUS_FAILED,
                None,
                None,
                None,
                None,
                Some(&message),
            )
            .await
            {
                error!("Download {} failed and its row could not be updated: {}", id, e);
            }
            DownloadOutcome::Failed(message)
        }
    }
}

/// Downloads `source` for `record` into `dir` and waits for the result. The row of
/// `record` must exist; it ends as `COMPLETED` or `FAILED`.
pub async fn download_into(
    pool: &SqlitePool,
    record: &DownloadRecord,
    source: DownloadSource,
    dir: &Path,
) -> DownloadOutcome {
    let (_keep_open, cancel) = watch::channel(false);
    run_job(pool, record, async move { Ok(source) }, Some(dir.to_path_buf()), cancel).await
}

/// Like [`download_into`], stopping as soon as `cancel` holds `true`.
pub async fn download_into_cancellable(
    pool: &SqlitePool,
    record: &DownloadRecord,
    source: DownloadSource,
    dir: &Path,
    cancel: watch::Receiver<bool>,
) -> DownloadOutcome {
    run_job(pool, record, async move { Ok(source) }, Some(dir.to_path_buf()), cancel).await
}

// ---------------------------------------------------------------------------------------
// Running downloads and the queue
// ---------------------------------------------------------------------------------------

struct ActiveDownload {
    cancel: watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
}

static ACTIVE: Lazy<Mutex<HashMap<String, ActiveDownload>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static QUEUE_WAKE: Lazy<Notify> = Lazy::new(Notify::new);
static QUEUE_STARTED: AtomicBool = AtomicBool::new(false);
static RECONCILED: AtomicBool = AtomicBool::new(false);

fn spawn_job<S>(pool: SqlitePool, record: DownloadRecord, source: S, permit: Option<OwnedSemaphorePermit>)
where
    S: Future<Output = Result<DownloadSource, String>> + Send + 'static,
{
    let id = record.id.clone();
    let (cancel_tx, cancel_rx) = watch::channel(false);

    // Held across the spawn so the task cannot unregister itself before it is registered.
    let mut active = lock(&ACTIVE);
    let handle = tokio::spawn(async move {
        let _permit = permit;
        run_job(&pool, &record, source, None, cancel_rx).await;
        lock(&ACTIVE).remove(&record.id);
    });
    active.insert(id, ActiveDownload { cancel: cancel_tx, handle });
}

/// Starts downloading `source` for `record` in the background. The row of `record` must
/// already exist with status `DOWNLOADING`.
pub fn start_download(pool: SqlitePool, record: DownloadRecord, source: DownloadSource) {
    spawn_job(pool, record, async move { Ok(source) }, None);
}

/// Stops the running download `id`, if there is one, and waits until ffmpeg is gone and
/// the partial files are removed. Returns whether a download was running.
pub async fn cancel_download(id: &str) -> bool {
    let Some(active) = lock(&ACTIVE).remove(id) else {
        return false;
    };
    let _ = active.cancel.send(true);
    if tokio::time::timeout(Duration::from_secs(10), active.handle).await.is_err() {
        warn!("Download {} did not stop within 10 seconds of being cancelled", id);
    }
    true
}

/// Tells the queue worker that a `QUEUED` row was added.
pub fn notify_queue() {
    QUEUE_WAKE.notify_one();
}

/// Cleans up after a previous run of the app: rows left `DOWNLOADING` go back to `QUEUED`
/// and working files left in the download folder are deleted. Does its work once per
/// process and must run before the first download starts; `spawn_queue_worker` calls it.
pub async fn reconcile_on_startup(pool: &SqlitePool) -> Result<(), AppError> {
    if RECONCILED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let requeued = db::reconcile_stale_downloads(pool).await?;
    if requeued > 0 {
        info!("Re-queued {} download(s) interrupted by the previous exit", requeued);
    }

    let removed = tokio::task::spawn_blocking(|| get_download_dir().map(|dir| remove_stale_part_files(&dir)))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    if removed > 0 {
        info!("Removed {} partial download file(s)", removed);
    }
    Ok(())
}

/// Starts the background worker that downloads `QUEUED` rows, at most two at a time, after
/// reconciling what a previous run left behind. Call it once, after `AppState` is managed.
pub fn spawn_queue_worker(app: tauri::AppHandle) {
    if QUEUE_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(queue_loop(app));
}

async fn queue_loop(app: tauri::AppHandle) {
    let pool = match app.try_state::<AppState>() {
        Some(state) => state.pool.clone(),
        None => {
            error!("The download queue cannot start: the app state is not available");
            QUEUE_STARTED.store(false, Ordering::SeqCst);
            return;
        }
    };

    if let Err(e) = reconcile_on_startup(&pool).await {
        warn!("Could not reconcile downloads from the previous run: {}", e);
    }

    let slots = Arc::new(Semaphore::new(MAX_QUEUE_DOWNLOADS));
    loop {
        let Ok(permit) = slots.clone().acquire_owned().await else {
            return;
        };

        match db::claim_next_queued_download(&pool).await {
            Ok(Some(record)) => {
                let resolver_app = app.clone();
                let resolver_record = record.clone();
                spawn_job(
                    pool.clone(),
                    record,
                    async move { resolve_queued_source(&resolver_app, &resolver_record).await },
                    Some(permit),
                );
            }
            Ok(None) => {
                drop(permit);
                let _ = tokio::time::timeout(QUEUE_POLL_INTERVAL, QUEUE_WAKE.notified()).await;
            }
            Err(e) => {
                drop(permit);
                warn!("The download queue could not read the next row: {}", e);
                tokio::time::sleep(QUEUE_POLL_INTERVAL).await;
            }
        }
    }
}

/// The one call into the provider manager. `preferred_provider` is `None` to try every
/// provider, or a provider key to use only that one.
async fn fetch_episode_sources(
    manager: &ProviderManager,
    record: &DownloadRecord,
    mal_id: Option<i64>,
    title_romaji: Option<String>,
    preferred_provider: Option<&str>,
) -> Result<SourcesResult, AppError> {
    manager
        .get_sources_for_episode(
            record.anilist_id,
            mal_id,
            record.episode_num,
            &record.category,
            title_romaji,
            preferred_provider,
        )
        .await
}

static TITLE_CACHE: Lazy<Mutex<HashMap<i64, (Option<i64>, Option<String>)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// MyAnimeList id and romaji title of an anime, which providers search by. A download row
/// only stores the display title, so they are looked up once per anime.
async fn lookup_search_keys(state: &AppState, anilist_id: i64) -> (Option<i64>, Option<String>) {
    if let Some(cached) = lock(&TITLE_CACHE).get(&anilist_id).cloned() {
        return cached;
    }

    let details = tokio::time::timeout(
        Duration::from_secs(15),
        state.anilist_client.fetch_details(anilist_id),
    )
    .await;
    match details {
        Ok(Ok(media)) => {
            let keys = (
                media.get("idMal").and_then(|v| v.as_i64()),
                media
                    .get("title")
                    .and_then(|t| t.get("romaji"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .filter(|t| !t.trim().is_empty()),
            );
            lock(&TITLE_CACHE).insert(anilist_id, keys.clone());
            keys
        }
        Ok(Err(e)) => {
            warn!("Could not look up anime {} on AniList: {}", anilist_id, e);
            (None, None)
        }
        Err(_) => {
            warn!("Looking up anime {} on AniList timed out", anilist_id);
            (None, None)
        }
    }
}

/// The provider a row asks for, as the manager spells it. `auto`, an empty value and any
/// name the manager does not know (for example a display name saved by an older version)
/// all mean "try every provider".
pub fn preferred_provider<'a>(wanted: &str, known: &[&'a str]) -> Option<&'a str> {
    let wanted = wanted.trim();
    if wanted.is_empty() || wanted.eq_ignore_ascii_case("auto") {
        return None;
    }
    known
        .iter()
        .find(|name| **name == wanted)
        .or_else(|| known.iter().find(|name| name.eq_ignore_ascii_case(wanted)))
        .copied()
}

/// Finds a stream for a queued row the same way the player does.
async fn resolve_queued_source(app: &tauri::AppHandle, record: &DownloadRecord) -> Result<DownloadSource, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "The app state is not available".to_string())?;

    let (mal_id, romaji) = lookup_search_keys(&state, record.anilist_id).await;
    let title = romaji.or_else(|| Some(record.series_title.clone()).filter(|t| !t.trim().is_empty()));
    let known = state.provider_manager.provider_names();
    let preferred = preferred_provider(&record.provider, &known);

    let sources = fetch_episode_sources(&state.provider_manager, record, mal_id, title, preferred)
        .await
        .map_err(|e| e.to_string())?;

    let stream = pick_stream(&sources.streams, &record.quality)
        .ok_or_else(|| "No playable stream was found for this episode.".to_string())?;
    Ok(DownloadSource::from_stream(stream, sources.subtitles.clone()))
}
