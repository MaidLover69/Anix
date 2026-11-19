pub mod engine;

use crate::commands::player::sanitized_command;
use crate::db;
use crate::error::AppError;
use crate::models::downloads::{
    HEADROOM_BYTES, P1080_BYTES, P360_BYTES, P480_BYTES, P720_BYTES,
};
use crate::models::{DownloadRecord, SourcesResult, StreamItem, SubtitleItem};
use crate::providers::manager::ProviderManager;
use crate::state::AppState;
use once_cell::sync::{Lazy, OnceCell};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
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

/// Most queued downloads that can run at the same time; Settings › Downloads picks 1 to 3.
const MAX_QUEUE_DOWNLOADS: usize = 3;
const QUEUE_POLL_INTERVAL: Duration = Duration::from_secs(5);
const PROGRESS_WRITE_INTERVAL: Duration = Duration::from_secs(1);
/// A direct download with no data for this long is treated as dead.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const SUBTITLE_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_SUBTITLE_BYTES: usize = 10 * 1024 * 1024;
const STDERR_TAIL_BYTES: usize = 4096;
const ERROR_TAIL_CHARS: usize = 600;

/// The HTTP client used for stream bodies, playlists and subtitles: no limit on the whole
/// request, so a long body is not cut off. It reaches public addresses only and checks every
/// redirect hop with `check_download_url`.
fn stream_client() -> Result<reqwest::Client, String> {
    static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();
    CLIENT
        .get_or_try_init(|| crate::clients::net_guard::guarded_download_client(check_download_url))
        .cloned()
        .map_err(|e| e.reason())
}

/// Origins of loopback test servers that downloads may reach. Empty in the app.
static PRIVATE_ORIGINS_ALLOWED: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Lets downloads reach `origin` (`http://127.0.0.1:<port>`) although it is a private address,
/// for tests whose servers run on loopback. Every other private address stays refused.
#[doc(hidden)]
pub fn allow_private_origin_for_tests(origin: &str) {
    if let Ok(url) = reqwest::Url::parse(origin) {
        lock(&PRIVATE_ORIGINS_ALLOWED).push(url.origin().ascii_serialization());
    }
}

/// Ok for a URL the downloader may fetch itself: not written as a private address (names are
/// checked by the client's resolver). Playlists, segments, keys and subtitles come from
/// extensions, so they are checked like the extensions' own requests.
fn check_download_url(url: &reqwest::Url) -> Result<(), String> {
    if lock(&PRIVATE_ORIGINS_ALLOWED).contains(&url.origin().ascii_serialization()) {
        return Ok(());
    }
    crate::clients::net_guard::check_public_url(url).map_err(|e| format!("Download refused: {}", e))
}

/// `validate_stream_url` plus `check_download_url`, for every URL the downloader requests.
fn guarded_stream_url(raw: &str) -> Result<String, String> {
    let url = validate_stream_url(raw)?;
    check_download_url(&reqwest::Url::parse(&url).map_err(|e| format!("Invalid stream URL: {}", e))?)?;
    Ok(url)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageCheck {
    pub ok: bool,
    pub free_bytes: i64,
    pub needed_bytes: i64,
    /// Size of the disk the download folder is on; 0 when it cannot be read.
    #[serde(default)]
    pub total_bytes: i64,
    /// The download folder.
    #[serde(default)]
    pub dir: String,
}

impl StorageCheck {
    pub fn new(ok: bool, free_bytes: i64, needed_bytes: i64) -> Self {
        Self {
            ok,
            free_bytes,
            needed_bytes,
            total_bytes: 0,
            dir: String::new(),
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

fn read_settings() -> Option<serde_json::Value> {
    let settings_path = db::get_config_dir().ok()?.join("settings.json");
    let text = std::fs::read_to_string(settings_path).ok()?;
    let settings: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            warn!("settings.json could not be parsed, using the defaults for downloads: {}", e);
            return None;
        }
    };
    Some(settings)
}

fn read_download_dir_setting() -> Option<PathBuf> {
    configured_download_dir(&read_settings()?, dirs::home_dir().as_deref())
}

/// The subtitle languages a download fetches: `None` (every track) when "Download all
/// subtitle languages" is on, else the user's subtitle languages in order.
pub fn subtitle_languages_for_download(settings: &serde_json::Value) -> Option<Vec<String>> {
    if settings.get("download_all_subtitles").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    Some(crate::providers::languages::LanguagePrefs::from_settings(settings).sub)
}

/// [`subtitle_languages_for_download`] for the saved settings. Reads a file, so async code
/// calls it through `spawn_blocking`.
pub fn subtitle_languages_from_saved_settings() -> Option<Vec<String>> {
    let settings = read_settings().unwrap_or_else(|| serde_json::json!({}));
    subtitle_languages_for_download(&settings)
}

/// The tracks a download fetches: for each wanted language (in order, matched by language
/// not spelling) one track, the default-flagged one first. With no match, or no wanted
/// language, the source's default (else first) track. `None` keeps every track, the default
/// first. The first track is the one embedded in the video.
pub fn select_subtitles(tracks: &[SubtitleItem], wanted: Option<&[String]>) -> Vec<SubtitleItem> {
    let mut by_default: Vec<&SubtitleItem> = tracks.iter().collect();
    by_default.sort_by_key(|t| t.is_default != Some(true));
    let Some(wanted) = wanted else { return by_default.into_iter().cloned().collect() };

    let mut chosen: Vec<&SubtitleItem> = Vec::new();
    for code in wanted {
        let found = by_default
            .iter()
            .find(|t| crate::providers::languages::normalize_language(&t.language).as_deref() == Some(code.as_str()));
        chosen.extend(found.copied());
    }
    if chosen.is_empty() {
        chosen.extend(by_default.first());
    }
    chosen.into_iter().cloned().collect()
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

/// Refuses a folder downloads cannot be saved in: not absolute, not creatable, or not
/// writable (checked by creating and removing a small file).
pub fn check_writable_dir(dir: &Path) -> Result<(), String> {
    let refuse = |why: String| Err(format!("Anix cannot save downloads in {}: {}. Choose another folder.", dir.display(), why));
    if !dir.is_absolute() {
        return refuse("it is not a full path".to_string());
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        return refuse(e.to_string());
    }
    let probe = dir.join(format!(".anix-write-check-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => refuse(e.to_string()),
    }
}

/// Free and total bytes of the disk `dir` is on; `None` when it cannot be read.
pub fn disk_space(dir: &Path) -> Option<(i64, i64)> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let block = stat.f_frsize as i64;
    Some(((stat.f_bavail as i64).saturating_mul(block), (stat.f_blocks as i64).saturating_mul(block)))
}

/// Below this much free space SQLite and the settings and extension files can no longer be saved.
pub const LOW_DISK_BYTES: i64 = 64 * 1024 * 1024;

/// Whether the disk holding `dir` is (nearly) full. Unknown counts as not full.
pub fn disk_is_low(dir: &Path) -> bool {
    disk_space(dir).is_some_and(|(free, _)| free < LOW_DISK_BYTES)
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

    let (free_bytes, total_bytes) = disk_space(&dir).unwrap_or((10_000_000_000i64, 0));
    let ok = free_bytes >= (needed_bytes + HEADROOM_BYTES);
    Ok(StorageCheck {
        ok,
        free_bytes,
        needed_bytes,
        total_bytes,
        dir: dir.to_string_lossy().into_owned(),
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

/// `Episode 03`, the folder of one episode inside its title folder.
pub fn episode_dir_name(episode_num: f64) -> String {
    format!("Episode {:02}", episode_num)
}

/// `<root>/<title>/Episode NN`: where a new download is saved. The title goes through the
/// same sanitising and length limit as file names, so it is always one folder inside `root`.
pub fn output_dir(root: &Path, series_title: &str, episode_num: f64) -> PathBuf {
    let title = sanitize_filename(series_title);
    let title = if title.is_empty() { "Untitled" } else { &title };
    let title = truncate_to_bytes(title, MAX_BASE_NAME_BYTES).trim_end();
    root.join(title).join(episode_dir_name(episode_num))
}

/// A short stable tag derived from the download id; it keeps the working files of two
/// downloads of the same episode apart. SHA-256, so every Anix build computes the same tag
/// and an update does not orphan the resumable files of queued downloads.
pub fn work_tag(download_id: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(download_id.as_bytes())[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

fn work_file_name(base: &str, tag: &str, role: &str) -> String {
    format!("{}.{}.{}{}", base, tag, role, PART_SUFFIX)
}

/// Working files a download keeps across a restart of the app, so it can resume: the bytes of
/// a direct file so far, what it was expected to total, and the folder of fetched HLS segments.
pub const RESUME_ROLES: [&str; 3] = ["download", "resume", "hls"];

/// Whether `path` is one of the resumable working files ([`RESUME_ROLES`]) of a download.
fn is_resume_file(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    RESUME_ROLES.iter().any(|role| name.ends_with(&format!(".{}{}", role, PART_SUFFIX)))
}

/// Deletes the resumable working files a failed try of `record` kept in `dir` for Try again.
/// Only these exact `*.anix-part` names are touched, never a finished video. Returns how many
/// entries were removed. Call it only while the download is not running.
pub fn remove_resume_files(dir: &Path, record: &DownloadRecord) -> usize {
    // The episode folder of the current layout, and the folder itself for older downloads.
    let episode = output_dir(dir, &record.series_title, record.episode_num);
    let folders = [episode.clone(), dir.to_path_buf()];
    let removed = folders
        .iter()
        .flat_map(|folder| RESUME_ROLES.iter().map(move |role| work_file_path(folder, record, role)))
        .filter(|path| match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path).is_ok(),
            Ok(_) => std::fs::remove_file(path).is_ok(),
            Err(_) => false,
        })
        .count();
    if removed > 0 {
        prune_empty_folders(&episode, dir);
    }
    removed
}

/// Where a new download of `record` is saved under `root`: `<title>/Episode NN`, except that a
/// download queued under 1.4.1 and part-way done keeps its flat folder, so it resumes.
pub fn download_folder(root: &Path, record: &DownloadRecord) -> PathBuf {
    let flat_partial = RESUME_ROLES.iter().any(|role| work_file_path(root, record, role).exists());
    if flat_partial {
        root.to_path_buf()
    } else {
        output_dir(root, &record.series_title, record.episode_num)
    }
}

/// Whether a download that failed with `error` keeps its resumable files for Try again. The
/// network dropping, stalling or timing out does, and so does the host refusing a request (an
/// expired link, a 403 or 404, a link that could not be renewed): the saved segments are still
/// good, and a later try can renew the link. Anything else (a web page, a full disk, ffmpeg)
/// starts over, so a bad or unwanted partial never lingers.
///
/// shortcut: matches the English failure messages written in this module, so a reworded message
/// falls back to starting over; switch to a typed error if more callers need the reason.
pub fn keeps_partial_files_after(error: &str) -> bool {
    const KEPT: [&str; 12] = [
        "Could not reach the stream host",
        "The stream host did not answer in time",
        "A segment of the stream was interrupted",
        "The stream stalled",
        "A segment of the stream is incomplete",
        "The download was interrupted after",
        "The download stalled after",
        "The download is incomplete",
        "A segment of the stream could not be fetched: HTTP ",
        "The stream host answered HTTP ",
        "The stream link expired and could not be renewed",
        "The renewed stream is not the same",
    ];
    KEPT.iter().any(|start| error.starts_with(start))
}

/// Where a download keeps the working file `role` in `dir`.
pub fn work_file_path(dir: &Path, record: &DownloadRecord, role: &str) -> PathBuf {
    let base = output_base_name(&record.series_title, record.episode_num, &record.category, &record.quality);
    dir.join(work_file_name(&base, &work_tag(&record.id), role))
}

/// Deletes working files (`*.anix-part`, and `*.anilili-part` from before the rename) left in
/// `dir` by a run that was interrupted, except the resumable ones ([`RESUME_ROLES`]) of the
/// downloads tagged `keep` (see [`work_tag`]). The HLS segment folder is the only directory
/// removed. Returns how many entries were removed. Only call it while no download is running.
pub fn remove_stale_part_files(dir: &Path, keep: &[String]) -> usize {
    stale_part_files(dir, keep, 0, None)
}

/// How many folders below the download folder hold working files: `<title>/Episode NN`.
const WORK_DEPTH: u32 = 2;

/// `skip` is a folder left alone with everything in it (the current download folder).
fn stale_part_files(dir: &Path, keep: &[String], depth: u32, skip: Option<&Path>) -> usize {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let kept = |name: &str| {
        keep.iter().any(|tag| {
            RESUME_ROLES
                .iter()
                .any(|role| name.ends_with(&format!(".{}.{}{}", tag, role, PART_SUFFIX)))
        })
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_part = name.ends_with(PART_SUFFIX) || name.ends_with(LEGACY_PART_SUFFIX);
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if skip.is_some_and(|skip| entry.path().starts_with(skip)) {
            continue;
        }
        if !is_part && is_dir && depth < WORK_DEPTH {
            let inner = stale_part_files(&entry.path(), keep, depth + 1, skip);
            if inner > 0 {
                // A folder the clean-up emptied (a cancelled new download) goes too.
                let _ = std::fs::remove_dir(entry.path());
            }
            removed += inner;
            continue;
        }
        if !is_part || kept(&name) {
            continue;
        }
        let gone = match entry.file_type() {
            Ok(t) if t.is_file() => std::fs::remove_file(entry.path()).is_ok(),
            Ok(t) if t.is_dir() && name.ends_with(&format!(".hls{}", PART_SUFFIX)) => {
                std::fs::remove_dir_all(entry.path()).is_ok()
            }
            _ => false,
        };
        if gone {
            removed += 1;
        }
    }
    removed
}

/// The list, in the app data folder, of download folders that have held working files.
const WORK_DIRS_FILE: &str = "download-work-dirs.txt";

/// Adds `dir` to the folder list `list` unless it is there already.
pub fn remember_work_dir(list: &Path, dir: &Path) {
    let line = dir.to_string_lossy();
    let known = std::fs::read_to_string(list).unwrap_or_default();
    if known.lines().any(|l| l == line) {
        return;
    }
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(list)
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{}\n", line).as_bytes()));
    if let Err(e) = written {
        warn!("Could not note the download folder {}: {}", dir.display(), e);
    }
}

/// Deletes the working files left in the folders of `list` other than `current` (queued
/// downloads resume only in `current`, so nothing there is kept), then leaves `current` and
/// any folder that is not there right now (an unmounted disk) in the list. Returns how many
/// entries were removed. Only call it while no download is running.
pub fn remove_part_files_in_old_dirs(list: &Path, current: &Path) -> usize {
    let known = std::fs::read_to_string(list).unwrap_or_default();
    let mut removed = 0;
    let mut still: Vec<&str> = vec![];
    for line in known.lines().filter(|l| !l.is_empty()) {
        let dir = Path::new(line);
        if dir == current {
            continue;
        }
        if dir.is_dir() {
            removed += stale_part_files(dir, &[], 0, Some(current));
        } else {
            still.push(line);
        }
    }
    let current_line = current.to_string_lossy();
    still.push(&current_line);
    let _ = std::fs::write(list, still.join("\n") + "\n");
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
    /// ISO 639-1 code the provider gave the result: the subtitle language of a sub, the audio
    /// language of a dub.
    pub language: Option<String>,
    /// Subtitle languages to fetch, as codes in order; `None` fetches every track.
    pub subtitle_languages: Option<Vec<String>>,
}

/// The audio and subtitle language codes stored on a download row. A sub is Japanese audio
/// with the subtitles the provider named, else the default (or first) subtitle track.
pub fn download_languages(
    category: &str,
    language: Option<&str>,
    subtitles: &[SubtitleItem],
) -> (Option<String>, Option<String>) {
    let language = language.map(str::trim).filter(|l| !l.is_empty()).map(str::to_ascii_lowercase);
    if category.eq_ignore_ascii_case("dub") {
        return (language, None);
    }
    let track = subtitles
        .iter()
        .find(|s| s.is_default == Some(true))
        .or_else(|| subtitles.first())
        .and_then(|s| crate::providers::languages::normalize_language(&s.language));
    (Some("ja".to_string()), language.or(track))
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
            language: None,
            subtitle_languages: None,
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

pub(crate) fn quality_height(quality: &str) -> Option<u32> {
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
            .any(|line| line.split_whitespace().next() == Some(option))
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
    /// An HLS playlist on disk whose segments are files next to it. Must be an absolute path.
    LocalHls { playlist: PathBuf, hls: HlsDemuxerOptions },
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
        FfmpegInput::LocalHls { playlist, hls } => {
            if hls.allowed_extensions {
                text(&mut args, ["-allowed_extensions", "ALL"]);
            }
            if hls.extension_picky {
                text(&mut args, ["-extension_picky", "0"]);
            }
            text(&mut args, ["-i"]);
            args.push(playlist.as_os_str().to_os_string());
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
    // kill_on_drop does not run when the app exits; the kernel then ends ffmpeg instead
    // of leaving it writing a download nobody will finish.
    #[cfg(target_os = "linux")]
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
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

/// Episode folders that running downloads use, with how many each.
static BUSY_FOLDERS: Lazy<Mutex<HashMap<PathBuf, usize>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// A running download uses `episode` (a `<title>/Episode NN` folder): until it releases it, the
/// folder is not pruned, so a sub and a dub of one episode do not remove each other's folder.
pub fn claim_episode_folder(episode: &Path) {
    *lock(&BUSY_FOLDERS).entry(episode.to_path_buf()).or_insert(0) += 1;
}

/// Ends a [`claim_episode_folder`]; the folders are pruned ([`prune_empty_folders`]) once the
/// last user of `episode` is done.
pub fn release_episode_folder(episode: &Path, root: &Path) {
    {
        let mut busy = lock(&BUSY_FOLDERS);
        if let Some(n) = busy.get_mut(episode) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                busy.remove(episode);
            }
        }
    }
    prune_empty_folders(episode, root);
}

/// Holds a claim on an episode folder and releases it (pruning) on drop, also when the job panics.
struct FolderClaim {
    episode: PathBuf,
    root: PathBuf,
}

impl Drop for FolderClaim {
    fn drop(&mut self) {
        release_episode_folder(&self.episode, &self.root);
    }
}

/// Files a download has created and not yet handed over as results.
#[derive(Clone, Default)]
struct WorkFiles {
    files: Arc<Mutex<Vec<PathBuf>>>,
    /// The `<title>/Episode NN` folder of a new download and the download folder it is in.
    folder: Arc<Mutex<Option<FolderClaim>>>,
}

impl WorkFiles {
    fn track(&self, path: &Path) {
        lock(&self.files).push(path.to_path_buf());
    }

    fn release(&self, path: &Path) {
        lock(&self.files).retain(|p| p != path);
    }

    /// Notes the new download's episode folder, which it prunes at the end.
    fn use_folder(&self, episode: PathBuf, root: PathBuf) {
        claim_episode_folder(&episode);
        *lock(&self.folder) = Some(FolderClaim { episode, root });
    }

    /// Removes the new download's episode and title folders if nothing is left in them and no
    /// other running download uses the episode folder.
    fn prune_folders(&self) {
        drop(lock(&self.folder).take());
    }

    async fn remove_all(&self) {
        self.remove_where(|_| true).await
    }

    /// Removes the tracked files `doomed` picks and forgets the rest, which stay on disk.
    async fn remove_where(&self, doomed: impl Fn(&Path) -> bool) {
        let paths: Vec<PathBuf> = std::mem::take(&mut *lock(&self.files));
        for path in paths.into_iter().filter(|p| doomed(p)) {
            // The HLS segment folder is the one tracked directory.
            if tokio::fs::remove_file(&path).await.is_err() {
                let _ = tokio::fs::remove_dir_all(&path).await;
            }
        }
    }
}

/// Looks the stream up again, the way the queue found it the first time.
pub type SourceFn = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<DownloadSource, String>> + Send>> + Send + Sync>;

/// Speed and time left of a running download, shown on Downloads.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DownloadSpeed {
    pub speed_bps: f64,
    pub eta_s: Option<u64>,
}

static SPEEDS: Lazy<Mutex<HashMap<String, DownloadSpeed>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// The speed of every running download, by download id.
pub fn download_speeds() -> HashMap<String, DownloadSpeed> {
    lock(&SPEEDS).clone()
}

struct SpeedEntry(String);

impl Drop for SpeedEntry {
    fn drop(&mut self) {
        lock(&SPEEDS).remove(&self.0);
    }
}

/// Runs `work` while saving its progress and speed about twice a second. `fraction` turns the
/// meter into progress; `remaining` is the bytes still to come, when it can tell.
async fn with_progress<T>(
    job: &Job<'_>,
    meter: &engine::SpeedMeter,
    fraction: impl Fn(&engine::SpeedMeter) -> Option<f64>,
    remaining: impl Fn(&engine::SpeedMeter) -> Option<u64>,
    work: impl Future<Output = T>,
) -> T {
    tokio::pin!(work);
    let _entry = SpeedEntry(job.record.id.clone());
    let mut reporter = ProgressReporter::new(job.pool, &job.record.id);
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = tick.tick() => {
                let bps = meter.bytes_per_sec(Instant::now());
                let eta = remaining(meter).and_then(|left| engine::eta_seconds(left, bps));
                lock(&SPEEDS).insert(job.record.id.clone(), DownloadSpeed { speed_bps: bps, eta_s: eta });
                reporter.report(fraction(meter), Some(meter.bytes() as i64)).await;
            }
        }
    }
}

/// Why a fetch failed, in the words the Downloads page shows. `have` is the bytes saved so far
/// for a direct file; `None` for the files of an HLS stream.
fn fetch_failure(error: engine::FetchError, have: Option<u64>) -> String {
    use engine::FetchError::*;
    match (error, have) {
        (Status { code, .. }, None) => format!("A segment of the stream could not be fetched: HTTP {}.", code),
        (Status { code, .. }, Some(_)) => format!("The stream host answered HTTP {}.", code),
        (Connect(e), None) => format!("Could not reach the stream host: {}", e),
        (Connect(e), Some(_)) if e.starts_with("The download is incomplete") => e,
        (Connect(e), Some(n)) => format!("The download was interrupted after {} bytes: {}", n, e),
        (Timeout, Some(n)) => format!("The download stalled after {} bytes: no data for {} seconds.", n, STALL_TIMEOUT.as_secs()),
        (Timeout, None) => "The stream host did not answer in time.".to_string(),
        (Changed, _) => "The file changed on the server while it was downloading.".to_string(),
        (Integrity(e) | Io(e), _) => e,
        (Manifest(kind), _) => format!("The body is a {} playlist.", kind),
    }
}

/// The headers of every request the engine makes, as name and value pairs.
fn header_pairs(headers: &RequestHeaders) -> Vec<(String, String)> {
    let mut pairs = vec![("User-Agent".to_string(), headers.user_agent.clone())];
    pairs.extend(headers.headers.iter().cloned());
    pairs
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
    /// The height of the HLS variant that is downloaded; 0 when unknown.
    got_height: std::sync::atomic::AtomicU32,
    /// Asks the extension for the stream again, when a link expires mid-download.
    renew: Option<SourceFn>,
}

impl Job<'_> {
    /// Path of a working file; it is removed again unless the download hands it over.
    fn work_path(&self, role: &str) -> PathBuf {
        let path = self.dir.join(work_file_name(&self.base, &self.tag, role));
        // A job of the same episode may have pruned the folder meanwhile.
        let _ = std::fs::create_dir_all(&self.dir);
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
    for ext in crate::local_media::VIDEO_EXTENSIONS {
        if path.ends_with(&format!(".{}", ext)) {
            return ext;
        }
    }
    "mp4"
}

/// Fetches the subtitle files with the stream's headers. A subtitle that cannot be
/// fetched, is not a 2xx response, or is not a subtitle file is skipped with a log line;
/// it never fails the download. `subtitles` is already chosen and ordered ([`select_subtitles`]).
async fn fetch_subtitles(
    job: &Job<'_>,
    client: &reqwest::Client,
    headers: &RequestHeaders,
    subtitles: &[SubtitleItem],
) -> Vec<SubtitleInput> {
    let mut fetched = Vec::new();
    for (idx, sub) in subtitles.iter().enumerate() {
        let body = async {
            let url = guarded_stream_url(&sub.url)?;
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
            crate::stream_proxy::read_limited(res, MAX_SUBTITLE_BYTES).await
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

/// What an HLS playlist says about the stream's length.
#[derive(Debug, Clone, PartialEq)]
pub enum HlsPlaylist {
    /// A media playlist: the sum of its segment durations, in seconds.
    Media { duration_s: f64 },
    /// A master playlist: the absolute URL of its first variant.
    Master { first_variant: String },
}

/// Reads an HLS playlist. `None` when the text is not one, or a media playlist has no
/// segment durations.
pub fn parse_hls_playlist(text: &str, base: &str) -> Option<HlsPlaylist> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.trim_start().starts_with("#EXTM3U") {
        return None;
    }
    let mut lines = text.lines().map(str::trim);
    let mut total = 0.0;
    let mut segments = 0usize;
    while let Some(line) = lines.next() {
        if line.starts_with("#EXT-X-STREAM-INF") {
            let uri = lines.by_ref().find(|l| !l.is_empty() && !l.starts_with('#'))?;
            let url = reqwest::Url::parse(base).ok()?.join(uri).ok()?;
            return Some(HlsPlaylist::Master { first_variant: url.to_string() });
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let value = rest.split(',').next().unwrap_or("").trim();
            if let Ok(seconds) = value.parse::<f64>() {
                if seconds.is_finite() && seconds >= 0.0 {
                    total += seconds;
                    segments += 1;
                }
            }
        }
    }
    (segments > 0 && total > 0.0).then_some(HlsPlaylist::Media { duration_s: total })
}

/// The length of an HLS stream from its playlist, following a master playlist to its
/// first variant. `None` when it cannot be read; the download then runs without a
/// percentage.
async fn hls_duration(url: &str, headers: &RequestHeaders) -> Option<f64> {
    let mut current = url.to_string();
    let client = stream_client().ok()?;
    for _ in 0..3 {
        let target = guarded_stream_url(&current).ok()?;
        let response = apply_headers(client.get(&target), headers)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let final_url = response.url().to_string();
        let text = response.text().await.ok()?;
        match parse_hls_playlist(&text, &final_url)? {
            HlsPlaylist::Media { duration_s } => return Some(duration_s),
            HlsPlaylist::Master { first_variant } => current = first_variant,
        }
    }
    None
}

/// An HLS stream the downloader fetches file by file, so an interrupted download resumes at
/// the first segment it does not have yet.
#[derive(Debug, Clone, PartialEq)]
pub struct HlsPlan {
    /// The media playlist with every URI replaced by the local name of its file. ffmpeg reads
    /// it; it also tells whether files left by an earlier run belong to the same stream.
    pub playlist: String,
    /// Absolute URL and local name of every file, in playlist order: keys, init sections and
    /// media segments.
    pub files: Vec<(String, String)>,
    /// For each of `files`: whether it is a media segment encrypted with AES-128, whose bytes
    /// cannot be recognised as video before they are decrypted.
    pub encrypted: Vec<bool>,
    pub duration_s: f64,
}

/// The value of `name` in an HLS attribute list (`METHOD=AES-128,URI="k.bin"`), unquoted.
pub fn hls_attribute<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attrs;
    while !rest.is_empty() {
        let (key, after) = rest.split_once('=')?;
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"')?;
            (&quoted[..end], quoted[end + 1..].trim_start_matches(','))
        } else {
            match after.split_once(',') {
                Some((value, next)) => (value, next),
                None => (after, ""),
            }
        };
        if key.trim() == name {
            return Some(value.trim());
        }
        rest = next;
    }
    None
}

/// The variant of a master playlist ffmpeg would pick: the highest resolution, then the
/// highest bandwidth. `None` when it has none, or when audio comes as a separate rendition,
/// which only ffmpeg's own HLS reader combines.
pub fn best_hls_variant(text: &str, base: &str) -> Option<String> {
    hls_variant_for(text, base, None)
}

/// Like `best_hls_variant`, but with a wanted picture height (a `RESOLUTION` height): the
/// exact one, else the closest lower, else the closest higher. Variants without a
/// `RESOLUTION` are only used when none has one, or when no height is wanted.
pub fn hls_variant_for(text: &str, base: &str, wanted: Option<u32>) -> Option<String> {
    hls_variant_pick(text, base, wanted).map(|(url, _)| url)
}

/// `hls_variant_for` with the picked variant's height (0 when it states none).
fn hls_variant_pick(text: &str, base: &str, wanted: Option<u32>) -> Option<(String, u32)> {
    let base = reqwest::Url::parse(base).ok()?;
    let mut lines = text.lines().map(str::trim);
    // (height, pixels, bandwidth, url)
    let mut variants: Vec<(u64, u64, u64, String)> = Vec::new();
    while let Some(line) = lines.next() {
        if let Some(attrs) = line.strip_prefix("#EXT-X-MEDIA:") {
            if hls_attribute(attrs, "TYPE") == Some("AUDIO") && hls_attribute(attrs, "URI").is_some() {
                return None;
            }
        }
        let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") else { continue };
        let uri = lines.by_ref().find(|l| !l.is_empty() && !l.starts_with('#'))?;
        let (width, height) = hls_attribute(attrs, "RESOLUTION")
            .and_then(|r| r.split_once('x'))
            .and_then(|(w, h)| Some((w.parse::<u64>().ok()?, h.parse::<u64>().ok()?)))
            .unwrap_or((0, 0));
        let bandwidth = hls_attribute(attrs, "BANDWIDTH").and_then(|b| b.parse::<u64>().ok()).unwrap_or(0);
        variants.push((height, width * height, bandwidth, base.join(uri).ok()?.to_string()));
    }
    // The first of equal variants wins (max_by_key would take the last).
    type Variant = (u64, u64, u64, String);
    fn best(candidates: Vec<&Variant>) -> Option<&Variant> {
        candidates.into_iter().fold(None, |best, v| match best {
            Some(b) if (b.1, b.2) >= (v.1, v.2) => Some(b),
            _ => Some(v),
        })
    }
    let rated: Vec<&(u64, u64, u64, String)> = variants.iter().filter(|v| v.0 > 0).collect();
    let chosen = match wanted.filter(|_| !rated.is_empty()).map(u64::from) {
        None => best(variants.iter().collect()),
        Some(wanted) => {
            // Heights in the order exact, closest lower, closest higher.
            let pick_height = rated
                .iter()
                .map(|v| v.0)
                .filter(|h| *h <= wanted)
                .max()
                .or_else(|| rated.iter().map(|v| v.0).min())?;
            best(rated.into_iter().filter(|v| v.0 == pick_height).collect())
        }
    }?;
    Some((chosen.3.clone(), chosen.0 as u32))
}

/// The base name of the finished file: named by the height that was actually downloaded when
/// that is not the one asked for (720p asked, 480p the closest the stream had).
pub fn downloaded_base_name(series_title: &str, episode_num: f64, category: &str, asked: &str, got: Option<u32>) -> String {
    match got.filter(|h| *h > 0 && quality_height(asked) != Some(*h)) {
        Some(h) => output_base_name(series_title, episode_num, category, &format!("{}p", h)),
        None => output_base_name(series_title, episode_num, category, asked),
    }
}

/// Plans the download of a finished (VOD) media playlist. `None` for anything the plan does
/// not cover — a live or master playlist, byte ranges, SAMPLE-AES or unknown tags with a URI —
/// which ffmpeg then downloads on its own, without resume.
pub fn plan_hls_download(text: &str, base: &str) -> Option<HlsPlan> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.trim_start().starts_with("#EXTM3U") || !text.lines().any(|l| l.trim() == "#EXT-X-ENDLIST") {
        return None;
    }
    let base = reqwest::Url::parse(base).ok()?;
    let absolute = |uri: &str| -> Option<String> {
        let url = base.join(uri).ok()?;
        matches!(url.scheme(), "http" | "https").then(|| url.to_string())
    };

    let mut playlist: Vec<String> = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut encrypted: Vec<bool> = Vec::new();
    let mut encrypting = false;
    let (mut segments, mut keys, mut maps) = (0usize, 0usize, 0usize);
    let mut duration_s = 0.0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with("#EXT-X-STREAM-INF") || line.starts_with("#EXT-X-BYTERANGE") {
            return None;
        }
        let local = if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            match hls_attribute(attrs, "METHOD")? {
                "NONE" => {
                    encrypting = false;
                    None
                }
                "AES-128" => {
                    encrypting = true;
                    keys += 1;
                    Some((hls_attribute(attrs, "URI")?, format!("key{}.key", keys - 1)))
                }
                _ => return None,
            }
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            if hls_attribute(attrs, "BYTERANGE").is_some() {
                return None;
            }
            maps += 1;
            Some((hls_attribute(attrs, "URI")?, format!("init{}.mp4", maps - 1)))
        } else if line.starts_with('#') {
            if line.contains("URI=") {
                return None;
            }
            if let Some(rest) = line.strip_prefix("#EXTINF:") {
                duration_s += rest.split(',').next()?.trim().parse::<f64>().ok().filter(|d| d.is_finite() && *d >= 0.0)?;
            }
            playlist.push(line.to_string());
            continue;
        } else {
            segments += 1;
            let name = format!("seg{}.{}", segments - 1, if maps > 0 { "m4s" } else { "ts" });
            files.push((absolute(line)?, name.clone()));
            encrypted.push(encrypting);
            playlist.push(name);
            continue;
        };
        match local {
            Some((uri, name)) => {
                files.push((absolute(uri)?, name.clone()));
                // An init section after an AES-128 key is encrypted too (RFC 8216 4.3.2.5); a key is not.
                encrypted.push(encrypting && name.starts_with("init"));
                playlist.push(line.replacen(&format!("URI=\"{}\"", uri), &format!("URI=\"{}\"", name), 1));
            }
            None => playlist.push(line.to_string()),
        }
    }
    (segments > 0).then(|| HlsPlan { playlist: playlist.join("\n") + "\n", files, encrypted, duration_s })
}

/// Whether two local playlists describe the same stream: the same number of segments, media sequence
/// and key and init-section lines (so the same IVs). Only then can a renewed stream take over the
/// segments already saved.
pub fn same_hls_shape(saved: &str, fresh: &str) -> bool {
    let shape = |text: &str| {
        let segments = text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).count();
        let keys: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with("#EXT-X-KEY") || l.starts_with("#EXT-X-MAP") || l.starts_with("#EXT-X-MEDIA-SEQUENCE"))
            .collect();
        (segments, keys.iter().map(|l| l.to_string()).collect::<Vec<_>>())
    };
    shape(saved) == shape(fresh)
}

/// Where every file of `plan` comes from, without query strings (they often carry per-session
/// tokens). Saved next to the segments: the local playlist alone cannot tell two variants or
/// mirrors of a stream apart, as every URI in it is a local name.
pub fn hls_source_stamp(plan: &HlsPlan) -> String {
    plan.files
        .iter()
        .map(|(url, _)| url.split(['?', '#']).next().unwrap_or(url))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Reads the stream's playlist, following a master playlist to its best variant, and plans
/// the download. `None` when it cannot be read or planned; ffmpeg then reads it itself. A
/// private address (the variant, or a redirect hop) is an error instead, because handing the
/// playlist to ffmpeg would reach that address anyway.
async fn fetch_hls_plan(url: &str, headers: &RequestHeaders, quality: &str) -> Result<Option<(HlsPlan, u32)>, String> {
    let client = stream_client()?;
    let mut current = url.to_string();
    let mut height = 0;
    for _ in 0..3 {
        let target = guarded_stream_url(&current)?;
        let Some((final_url, text)) = get_playlist(&client, &target, headers).await? else { return Ok(None) };
        if text.contains("#EXT-X-STREAM-INF") {
            let Some((variant, picked)) = hls_variant_pick(&text, &final_url, quality_height(quality)) else { return Ok(None) };
            (current, height) = (variant, picked);
            continue;
        }
        return Ok(plan_hls_download(&text, &final_url).map(|plan| (plan, height)));
    }
    Ok(None)
}

/// The playlist at `target` and the URL it ended up at, retried like a segment. `None` when the
/// host keeps refusing it; a refused redirect or a 403 is an error.
async fn get_playlist(client: &reqwest::Client, target: &str, headers: &RequestHeaders) -> Result<Option<(String, String)>, String> {
    use engine::FetchError;
    let policy = engine::RetryPolicy::APP;
    let mut attempt = 0;
    loop {
        let error = match apply_headers(client.get(target), headers).timeout(Duration::from_secs(30)).send().await {
            Ok(response) if response.status().is_success() => {
                let final_url = response.url().to_string();
                match response.text().await {
                    Ok(text) => return Ok(Some((final_url, text))),
                    Err(e) => FetchError::Connect(e.to_string()),
                }
            }
            Ok(response) => FetchError::Status { code: response.status().as_u16(), retry_after: engine::retry_after_of(&response) },
            Err(e) if e.is_redirect() => return Err(redirect_refusal(&e)),
            Err(e) if e.is_timeout() => FetchError::Timeout,
            Err(e) => FetchError::Connect(e.to_string()),
        };
        engine::note_failure(target, &error);
        if matches!(error, FetchError::Status { code: 403, .. }) {
            return Err(PLAYLIST_REFUSED.to_string());
        }
        attempt += 1;
        if !engine::is_retryable(&error) || attempt >= policy.max_tries {
            return Ok(None);
        }
        engine::wait_before_retry(&policy, attempt - 1, &error).await;
    }
}

/// `get_playlist`'s error for a 403: the signed link expired, which a renewed stream can fix.
const PLAYLIST_REFUSED: &str = "The stream's playlist answered HTTP 403";

/// Why a redirect was not followed: the hop's refusal from `check_download_url`, or too many
/// hops. reqwest's own text names only the first URL.
fn redirect_refusal(e: &reqwest::Error) -> String {
    let reason = std::error::Error::source(e).map_or_else(|| e.to_string(), |r| r.to_string());
    if reason.starts_with("Download refused") {
        reason
    } else {
        format!("Download refused: {}", reason)
    }
}

/// Asks the extension for the stream again and returns the new URL of every file of the plan.
/// The new stream must be the one being downloaded (`saved` is its local playlist), or the
/// segments already saved could not be joined with the new ones.
fn renewal(source: SourceFn, quality: String, saved: String) -> engine::RenewFn {
    Arc::new(move || {
        let (source, quality, saved) = (source.clone(), quality.clone(), saved.clone());
        Box::pin(async move {
            let fresh = source().await?;
            let url = guarded_stream_url(&fresh.url)?;
            let (plan, _) = fetch_hls_plan(&url, &request_headers(&fresh), &quality)
                .await?
                .ok_or_else(|| "the new stream could not be read".to_string())?;
            if !same_hls_shape(&saved, &plan.playlist) {
                return Err("the new stream does not have the same segments or keys".to_string());
            }
            plan.files.iter().map(|(url, _)| guarded_stream_url(url)).collect()
        })
    })
}

/// Downloads a planned HLS stream: every file into the download's segment folder, skipping
/// the ones an earlier run already finished, then ffmpeg muxes them into the MKV.
async fn download_hls_files(
    job: &Job<'_>,
    plan: &HlsPlan,
    headers: &RequestHeaders,
    hls: HlsDemuxerOptions,
    subtitles: &[SubtitleInput],
) -> Result<Finished, String> {
    let folder = job.work_path("hls");
    let index = folder.join("index.m3u8");
    let source = folder.join("source.txt");
    let stamp = hls_source_stamp(plan);
    // Files from an earlier run are only reused for the very same playlist from the same place.
    let same_stream = tokio::fs::read_to_string(&index).await.is_ok_and(|old| old == plan.playlist)
        && tokio::fs::read_to_string(&source).await.is_ok_and(|old| old == stamp);
    if !same_stream {
        let _ = tokio::fs::remove_dir_all(&folder).await;
    }
    tokio::fs::create_dir_all(&folder)
        .await
        .map_err(|e| format!("Could not create {}: {}", folder.display(), e))?;
    if !same_stream {
        tokio::fs::write(&index, &plan.playlist)
            .await
            .map_err(|e| format!("Could not write {}: {}", index.display(), e))?;
        tokio::fs::write(&source, &stamp)
            .await
            .map_err(|e| format!("Could not write {}: {}", source.display(), e))?;
    } else {
        info!("Resuming download {} from its saved segments", job.record.id);
    }

    let client = stream_client()?;
    let mut urls = Vec::with_capacity(plan.files.len());
    for (url, _) in &plan.files {
        urls.push(guarded_stream_url(url)?);
    }
    let paths: Vec<PathBuf> = plan.files.iter().map(|(_, name)| folder.join(name)).collect();
    let renew = job.renew.clone().map(|source| renewal(source, job.record.quality.clone(), plan.playlist.clone()));
    let options = engine::SegmentOptions {
        headers: header_pairs(headers),
        retry: engine::RetryPolicy::APP,
        limits: engine::HOST_LIMITS.clone(),
        renew,
    };
    let meter = engine::SpeedMeter::new(engine::SPEED_WINDOW);
    let fetched = with_progress(
        job,
        &meter,
        |m| {
            let (done, all) = m.items();
            (all > 0).then(|| 0.05 + 0.85 * done as f64 / all as f64)
        },
        |m| {
            let (done, all) = m.items();
            // The size is not known up front, so it is estimated from the segments so far.
            (done > 0).then(|| (m.bytes() as f64 * (all - done) as f64 / done as f64) as u64)
        },
        engine::fetch_files(&client, &urls, &paths, &plan.encrypted, &options, Some(&meter)),
    )
    .await;
    if let Err(e) = fetched {
        return Err(fetch_failure(e, None));
    }
    let media: Vec<(PathBuf, bool)> = plan
        .files
        .iter()
        .zip(&paths)
        .zip(&plan.encrypted)
        .filter(|(((_, name), _), _)| name.starts_with("seg"))
        .map(|((_, p), enc)| (p.clone(), *enc))
        .collect();
    let (media, media_encrypted): (Vec<PathBuf>, Vec<bool>) = media.into_iter().unzip();
    engine::verify_files_before_mux(&media, &media_encrypted)?;
    let mut reporter = ProgressReporter::new(job.pool, &job.record.id);
    reporter.report(Some(0.90), Some(meter.bytes() as i64)).await;

    let input = FfmpegInput::LocalHls { playlist: index, hls };
    let (video, _) =
        mux_with_ffmpeg(job, &input, subtitles.first(), &mut reporter, Some(plan.duration_s), (0.90, 0.99))
            .await
            .map_err(|e| e.to_string())?;
    finalize(job, &video, "mkv", subtitles).await
}

/// `bytes <start>-<end>/<total>` of a `Content-Range` header as `(start, total)`.
pub fn parse_content_range(value: &str) -> Option<(u64, u64)> {
    let (range, total) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    Some((start.trim().parse().ok()?, total.trim().parse().ok()?))
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
    duration_s: Option<f64>,
    progress_range: (f64, f64),
) -> Result<(PathBuf, bool), FfmpegError> {
    let output = job.work_path("video");
    let mut subtitle = subtitle;

    loop {
        let args = build_ffmpeg_args(input, subtitle, &output);
        match run_ffmpeg(args, reporter, duration_s, progress_range).await {
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
    let got = job.got_height.load(std::sync::atomic::Ordering::Relaxed);
    let record = job.record;
    let base = downloaded_base_name(&record.series_title, record.episode_num, &record.category, &record.quality, Some(got));
    let _ = tokio::fs::create_dir_all(&job.dir).await;
    for n in 1..=999u32 {
        let stem = if n == 1 {
            base.clone()
        } else {
            format!("{} ({})", base, n)
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
        base,
        job.dir.display()
    ))
}

/// Moves the finished video to its final name and keeps every subtitle as a side file next to
/// it. The first one is also inside the video, but the web player reads only side files.
async fn finalize(
    job: &Job<'_>,
    video: &Path,
    ext: &str,
    subtitles: &[SubtitleInput],
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


/// Deletes a finished download's video and the subtitle side files `finalize` saved next to
/// it (`<stem>.<language>.<vtt|ass|srt>`). Anything that is not a plain file with a video
/// extension downloads produce is refused, so a bad row cannot remove other files. A video
/// that is already gone is fine. Returns how many files were removed.
pub fn delete_finished_files(video: &Path) -> Result<usize, String> {
    let ext = video
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem().and_then(|s| s.to_str())) else {
        return Err(format!("{} is not a downloaded video.", video.display()));
    };
    if !video.is_absolute() || !crate::local_media::VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("{} is not a downloaded video.", video.display()));
    }
    let mut removed = 0;
    match std::fs::symlink_metadata(video) {
        Ok(meta) if meta.file_type().is_file() => {
            std::fs::remove_file(video)
                .map_err(|e| format!("Could not delete {}: {}", video.display(), e))?;
            removed += 1;
        }
        Ok(_) => return Err(format!("{} is not a regular file; it was left alone.", video.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Could not read {}: {}", video.display(), e)),
    }
    let prefix = format!("{}.", stem);
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(removed) };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(&prefix)) else { continue };
        let is_side = matches!(rest.split_once('.'), Some((label, ext))
            if !label.is_empty() && !ext.contains('.') && ["vtt", "ass", "srt"].contains(&ext));
        if is_side && entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => removed += 1,
                Err(e) => warn!("Could not delete subtitle {}: {}", entry.path().display(), e),
            }
        }
    }
    Ok(removed)
}

/// [`delete_finished_files`], then removes the episode folder and the title folder when they
/// are empty. Only folders strictly inside `root` are removed, so a download made before the
/// folder was changed, or a flat one, leaves its folder alone.
pub fn delete_download_files(video: &Path, root: &Path) -> Result<usize, String> {
    let removed = delete_finished_files(video)?;
    if let Some(episode) = video.parent() {
        prune_empty_folders(episode, root);
    }
    Ok(removed)
}

/// Removes `episode` and then its parent (the title folder) while they are empty. Only folders
/// strictly inside `root` go, and never through a `..`.
pub fn prune_empty_folders(episode: &Path, root: &Path) {
    if episode.components().any(|c| c == std::path::Component::ParentDir) || lock(&BUSY_FOLDERS).contains_key(episode) {
        return;
    }
    for dir in [Some(episode), episode.parent()].into_iter().flatten() {
        if dir == root || !dir.starts_with(root) || std::fs::remove_dir(dir).is_err() {
            break;
        }
    }
}

async fn download_stream_with_ffmpeg(
    job: &Job<'_>,
    url: &str,
    headers: &RequestHeaders,
    kind: StreamKind,
    subtitles: &[SubtitleInput],
) -> Result<Finished, String> {
    let hls_options = probe_ffmpeg().await.map_err(|e| e.to_string())?;
    if kind == StreamKind::Hls {
        let (planned, headers) = match fetch_hls_plan(url, headers, &job.record.quality).await {
            // An expired link goes through renewal when there is a way to renew it.
            Err(e) if e == PLAYLIST_REFUSED => match &job.renew {
                None => (None, headers.clone()),
                Some(renew) => {
                    let fresh = renew().await.map_err(|e| format!("The stream link expired and could not be renewed: {}", e))?;
                    let fresh_headers = request_headers(&fresh);
                    (fetch_hls_plan(&fresh.url, &fresh_headers, &job.record.quality).await?, fresh_headers)
                }
            },
            other => (other?, headers.clone()),
        };
        if let Some((plan, height)) = planned {
            job.got_height.store(height, std::sync::atomic::Ordering::Relaxed);
            return download_hls_files(job, &plan, &headers, hls_options, subtitles).await;
        }
    }
    let input = FfmpegInput::Remote {
        url: url.to_string(),
        headers: headers.clone(),
        hls: (kind == StreamKind::Hls).then_some(hls_options),
    };

    // Without a length ffmpeg's progress cannot become a percentage, and the download
    // would sit at its starting value until it finishes.
    let duration_s = match job.record.duration_s {
        Some(d) if d > 0.0 => Some(d),
        _ if kind == StreamKind::Hls => hls_duration(url, headers).await,
        _ => None,
    };

    let mut reporter = ProgressReporter::new(job.pool, &job.record.id);
    let mut retries = 0;
    let video = loop {
        match mux_with_ffmpeg(job, &input, subtitles.first(), &mut reporter, duration_s, (0.05, 0.99)).await {
            Ok((video, _)) => break video,
            // ffmpeg reads the stream itself here, so a dropped connection or a 5xx is retried.
            Err(FfmpegError::Failed { stderr_tail, .. }) if retries < FFMPEG_NETWORK_RETRIES && ffmpeg_network_failure(&stderr_tail) => {
                warn!("ffmpeg lost the stream of download {} ({}); trying again", job.record.id, stderr_tail);
                tokio::time::sleep(engine::backoff_delay(&engine::RetryPolicy::APP, retries, None, 0)).await;
                retries += 1;
            }
            Err(e) => return Err(e.to_string()),
        }
    };

    finalize(job, &video, "mkv", subtitles).await
}

/// Times ffmpeg is started again when it reads a stream itself and the network fails.
const FFMPEG_NETWORK_RETRIES: u32 = 3;

/// Whether the end of ffmpeg's stderr says the network failed (not that the stream is bad).
pub fn ffmpeg_network_failure(stderr_tail: &str) -> bool {
    const SIGNS: [&str; 13] = [
        "Server returned 5",
        "Server returned 429",
        "Server returned 408",
        "Connection reset",
        "Connection timed out",
        "Connection refused",
        "Connection closed",
        "Operation timed out",
        "Network is unreachable",
        "Input/output error",
        "I/O error",
        "Error in the pull function",
        "End of file",
    ];
    SIGNS.iter().any(|sign| stderr_tail.contains(sign))
}

async fn download_file(
    job: &Job<'_>,
    client: &reqwest::Client,
    url: &str,
    headers: &RequestHeaders,
    subtitles: &[SubtitleInput],
) -> Result<Finished, String> {
    // A run of the app that was stopped leaves the bytes so far and a note of the total and
    // validator they belong to.
    let temp = job.work_path("download");
    let note = job.work_path("resume");
    let options = engine::DirectOptions { headers: header_pairs(headers), ..Default::default() };
    for part in 0..options.max_connections {
        job.work.track(&engine::part_path(&temp, part));
    }
    let meter = engine::SpeedMeter::new(engine::SPEED_WINDOW);
    let result = with_progress(
        job,
        &meter,
        |m| m.expected().map(|total| 0.05 + 0.90 * (m.bytes() as f64 / total as f64).min(1.0)),
        |m| m.expected().map(|total| total.saturating_sub(m.bytes())),
        engine::download_partial(client, url, &temp, &note, &options, Some(&meter)),
    )
    .await;
    match result {
        Ok(report) => {
            if report.resumed_from > 0 {
                info!("Resumed download {} at byte {}", job.record.id, report.resumed_from);
            }
        }
        Err(engine::FetchError::Manifest(manifest)) => {
            // A playlist, not a media file: hand the URL to ffmpeg instead of saving the text.
            let _ = tokio::fs::remove_file(&temp).await;
            let _ = tokio::fs::remove_file(&note).await;
            job.work.release(&temp);
            let kind = if manifest == "hls" { StreamKind::Hls } else { StreamKind::Dash };
            return download_stream_with_ffmpeg(job, url, headers, kind, subtitles).await;
        }
        Err(e) => {
            let have = tokio::fs::metadata(&temp).await.map(|m| m.len()).unwrap_or(0);
            return Err(fetch_failure(e, Some(have)));
        }
    }

    // Remux into MKV with the subtitle inside when ffmpeg is available; otherwise keep the
    // file exactly as it was served.
    let mut reporter = ProgressReporter::new(job.pool, &job.record.id);
    let input = FfmpegInput::Local(temp.clone());
    match mux_with_ffmpeg(job, &input, subtitles.first(), &mut reporter, job.record.duration_s, (0.95, 0.99)).await {
        Ok((video, _)) => {
            let finished = finalize(job, &video, "mkv", subtitles).await?;
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
            finalize(job, &temp, container_extension(url), subtitles).await
        }
    }
}

async fn transfer<S>(
    pool: &SqlitePool,
    record: &DownloadRecord,
    source: S,
    place: Place,
    work: &WorkFiles,
    renew: Option<SourceFn>,
) -> Result<Finished, String>
where
    S: Future<Output = Result<DownloadSource, String>>,
{
    let source = source.await?;
    let url = guarded_stream_url(&source.url)?;

    let chosen = select_subtitles(&source.subtitles, source.subtitle_languages.as_deref());
    let (audio, sub) = download_languages(
        &record.category,
        source.language.as_deref(),
        // The first chosen track is the one embedded in the video.
        &chosen[..chosen.len().min(1)],
    );
    if let Err(e) = db::set_download_languages(pool, &record.id, audio.as_deref(), sub.as_deref()).await {
        warn!("Could not save the languages of download {}: {}", record.id, e);
    }

    let (root, nested) = match place {
        Place::Flat(dir) => (dir, false),
        Place::Under(root) => (root, true),
        Place::Default => (tokio::task::spawn_blocking(|| {
            let dir = get_download_dir()?;
            engine::LIMITER.set_limit(read_settings().as_ref().and_then(engine::speed_limit_bps));
            // So startup can clean this folder up after the user has chosen another one.
            if let Ok(app) = db::get_app_dir() {
                remember_work_dir(&app.join(WORK_DIRS_FILE), &dir);
            }
            Ok::<_, AppError>(dir)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?, true),
    };
    let dir = if nested { download_folder(&root, record) } else { root.clone() };
    if nested && dir != root {
        work.use_folder(dir.clone(), root.clone());
    }
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
        got_height: std::sync::atomic::AtomicU32::new(0),
        renew,
    };

    if let Err(e) = db::update_download_progress(pool, &record.id, Some(0.05), None).await {
        warn!("Could not save download progress for {}: {}", record.id, e);
    }

    let client = stream_client()?;
    let headers = request_headers(&source);
    let subtitles = fetch_subtitles(&job, &client, &headers, &chosen).await;

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

/// Where a job saves: straight into a folder (old layout, tests), under a download folder in
/// `<title>/Episode NN`, or under the configured download folder.
enum Place {
    Flat(PathBuf),
    Under(PathBuf),
    Default,
}

/// Runs one download from source resolution to the final row update. When `cancel` turns
/// true the work is dropped (which kills ffmpeg) and the working files are removed.
async fn run_job<S>(
    pool: &SqlitePool,
    record: &DownloadRecord,
    source: S,
    place: Place,
    cancel: watch::Receiver<bool>,
    renew: Option<SourceFn>,
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
        result = transfer(pool, record, source, place, &work, renew) => Some(result),
        _ = cancelled(cancel) => None,
    };
    // Nothing of the transfer is running any more, so its leftovers can go. A network failure
    // keeps the resumable ones, so Try again continues; a success or a later Remove deletes them.
    match &result {
        Some(Err(message)) if keeps_partial_files_after(message) => {
            work.remove_where(|path| !is_resume_file(path)).await
        }
        _ => work.remove_all().await,
    }
    work.prune_folders();

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
            let title = crate::i18n::fill(
                crate::i18n::current(),
                "download_failed",
                &[("title", &record.series_title), ("episode", &record.episode_num.to_string())],
            );
            if let Err(e) = crate::notices::push(pool, crate::notices::DOWNLOAD, &title, &message, Some("/downloads")).await {
                warn!("Could not add a notice for failed download {}: {}", id, e);
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
    run_job(pool, record, async move { Ok(source) }, Place::Flat(dir.to_path_buf()), cancel, None).await
}

/// Like [`download_into`], saving in `<root>/<title>/Episode NN` as new downloads do.
pub async fn download_under(
    pool: &SqlitePool,
    record: &DownloadRecord,
    source: DownloadSource,
    root: &Path,
) -> DownloadOutcome {
    let (_keep_open, cancel) = watch::channel(false);
    run_job(pool, record, async move { Ok(source) }, Place::Under(root.to_path_buf()), cancel, None).await
}

/// Like [`download_into`], stopping as soon as `cancel` holds `true`.
pub async fn download_into_cancellable(
    pool: &SqlitePool,
    record: &DownloadRecord,
    source: DownloadSource,
    dir: &Path,
    cancel: watch::Receiver<bool>,
) -> DownloadOutcome {
    run_job(pool, record, async move { Ok(source) }, Place::Flat(dir.to_path_buf()), cancel, None).await
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

fn spawn_job<S>(
    pool: SqlitePool,
    record: DownloadRecord,
    source: S,
    place: Place,
    permit: Option<OwnedSemaphorePermit>,
    renew: Option<SourceFn>,
) where
    S: Future<Output = Result<DownloadSource, String>> + Send + 'static,
{
    let id = record.id.clone();

    // Held across the spawn so the task cannot unregister itself before it is registered.
    let mut active = lock(&ACTIVE);
    if active.contains_key(&id) {
        // A second job would write the same working files, and the first could no longer be
        // cancelled. The running one owns the row and writes its result.
        warn!("Download {} is already running; it is not started a second time", id);
        return;
    }
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let handle = tokio::spawn(async move {
        let permit = permit;
        run_job(&pool, &record, source, place, cancel_rx, renew).await;
        lock(&ACTIVE).remove(&record.id);
        drop(permit);
        // A free slot may let the next queued download start.
        QUEUE_WAKE.notify_one();
    });
    active.insert(id, ActiveDownload { cancel: cancel_tx, handle });
}

/// Streams the player already resolved, by download id, for rows waiting in the queue.
static PLAYER_SOURCES: Lazy<Mutex<HashMap<String, (DownloadSource, Instant)>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Remembers the stream the player picked for the `QUEUED` row `id` (the row must be created by
/// the caller) and wakes the queue, so the download follows the same limits and renewal as any
/// other. A restart forgets it, and the row is then resolved like any queued one.
pub fn queue_with_source(id: &str, source: DownloadSource) {
    queue_with_source_at(id, source, Instant::now());
}

/// [`queue_with_source`] as of `at`, for tests.
#[doc(hidden)]
pub fn queue_with_source_at(id: &str, source: DownloadSource, at: Instant) {
    lock(&PLAYER_SOURCES).insert(id.to_string(), (source, at));
    notify_queue();
}

/// Signed links expire; a row that waited longer than this is resolved again instead.
const PLAYER_SOURCE_MAX_AGE: Duration = Duration::from_secs(5 * 60);

/// The remembered stream of row `id`, taken out, if it is still fresh.
#[doc(hidden)]
pub fn take_queued_source(id: &str) -> Option<DownloadSource> {
    lock(&PLAYER_SOURCES).remove(id).filter(|(_, at)| at.elapsed() < PLAYER_SOURCE_MAX_AGE).map(|(source, _)| source)
}

/// Forgets the source of a row that could not be created.
pub fn forget_queued_source(id: &str) {
    lock(&PLAYER_SOURCES).remove(id);
}

/// `start_download_in` runs a job into `dir` instead of the download folder, for tests.
#[doc(hidden)]
pub fn start_download_in(pool: SqlitePool, record: DownloadRecord, source: DownloadSource, dir: PathBuf) {
    spawn_job(pool, record, async move { Ok(source) }, Place::Flat(dir), None, None);
}

/// Puts a `FAILED` download back in the queue (Downloads › Try again). Any other row, or one
/// whose failed job is still winding down, is left alone, so a double click or a stale page can
/// never start a second job for the same row. Returns whether the row was queued again.
pub async fn retry_download(pool: &SqlitePool, id: &str) -> Result<bool, String> {
    // Checked before the row changes: a job leaves ACTIVE only after writing its final status.
    // The window is milliseconds (the final status is written, then the job leaves), so
    // reporting "not queued" is enough; the row keeps its status and a later click works.
    if lock(&ACTIVE).contains_key(id) {
        return Ok(false);
    }
    let queued = db::requeue_failed_download(pool, id).await.map_err(|e| e.to_string())?;
    if queued {
        notify_queue();
    }
    Ok(queued)
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

    // Queued rows keep what they had fetched, so they resume instead of starting over.
    let keep: Vec<String> = db::get_all_downloads(pool)
        .await?
        .iter()
        .filter(|r| r.status == db::STATUS_QUEUED)
        .map(|r| work_tag(&r.id))
        .collect();
    let removed = tokio::task::spawn_blocking(move || {
        let dir = get_download_dir()?;
        // Folders the user has moved downloads away from keep no partials either.
        let old = db::get_app_dir().map(|app| remove_part_files_in_old_dirs(&app.join(WORK_DIRS_FILE), &dir));
        Ok::<_, AppError>(remove_stale_part_files(&dir, &keep) + old.unwrap_or(0))
    })
    .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    if removed > 0 {
        info!("Removed {} partial download file(s)", removed);
    }
    Ok(())
}

/// Starts the background worker that downloads `QUEUED` rows, as many at a time as Settings › Downloads allows, after
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
        // Settings › Downloads: how many may run at once (the semaphore only holds the most).
        let allowed = tokio::task::spawn_blocking(|| engine::parallel_downloads(&read_settings().unwrap_or_default()))
            .await
            .unwrap_or(1);
        if MAX_QUEUE_DOWNLOADS - slots.available_permits() >= allowed {
            let _ = tokio::time::timeout(QUEUE_POLL_INTERVAL, QUEUE_WAKE.notified()).await;
            continue;
        }
        let Ok(permit) = slots.clone().acquire_owned().await else {
            return;
        };

        match db::claim_next_queued_download(&pool).await {
            Ok(Some(record)) => {
                let (resolver_app, resolver_record) = (app.clone(), record.clone());
                let resolve: SourceFn = Arc::new(move || {
                    let (app, record) = (resolver_app.clone(), resolver_record.clone());
                    Box::pin(async move { resolve_queued_source(&app, &record).await })
                });
                // A stream the player picked is used as it is; the extension is asked only to renew it.
                let first: Pin<Box<dyn Future<Output = Result<DownloadSource, String>> + Send>> = match take_queued_source(&record.id) {
                    Some(source) => Box::pin(async move { Ok(source) }),
                    None => resolve(),
                };
                spawn_job(pool.clone(), record, first, Place::Default, Some(permit), Some(resolve));
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

/// Per AniList id: the MAL id and the title used for file names.
type TitleKeys = (Option<i64>, Option<String>);
static TITLE_CACHE: Lazy<Mutex<HashMap<i64, TitleKeys>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// MyAnimeList id and romaji title of an anime, which providers search by. A download row
/// only stores the display title, so they are looked up once per anime.
async fn lookup_search_keys(state: &AppState, anilist_id: i64) -> (Option<i64>, Option<String>) {
    if let Some(cached) = lock(&TITLE_CACHE).get(&anilist_id).cloned() {
        return cached;
    }

    let details = tokio::time::timeout(
        Duration::from_secs(15),
        state.catalogue.details(anilist_id),
    )
    .await;
    match details {
        Ok(Ok(served)) => {
            let media = served.value;
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
            warn!("Could not look up anime {} in the catalogue: {}", anilist_id, e);
            (None, None)
        }
        Err(_) => {
            warn!("Looking up anime {} in the catalogue timed out", anilist_id);
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
    let mut source = DownloadSource::from_stream(stream, sources.subtitles.clone());
    source.language = sources.language.clone();
    source.subtitle_languages = tokio::task::spawn_blocking(subtitle_languages_from_saved_settings)
        .await
        .map_err(|e| e.to_string())?;
    Ok(source)
}
