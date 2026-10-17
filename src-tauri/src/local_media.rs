//! "Your files": video files on disk that the user linked to one AniList title, played by the
//! built-in player through the loopback stream proxy.
//!
//! The proxy never takes a path from a request. A file can only be served after
//! `register_playable` put it on the allow-list, and the commands only register files that
//! the database links to the title (linked files and the app's finished downloads). Requests
//! name the file by the random key it was registered under.

use crate::commands::local_scanner::{parse_episode_and_title, LocalAnimeFile};
use crate::db::{current_unix_millis, STATUS_COMPLETED};
use crate::error::AppError;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Provider key of local files in watch history and in `/watch?prov=`.
pub const LOCAL_PROVIDER: &str = "local";

pub const VIDEO_EXTENSIONS: [&str; 5] = ["mkv", "mp4", "m4v", "webm", "avi"];
pub const SUBTITLE_EXTENSIONS: [&str; 4] = ["srt", "ass", "ssa", "vtt"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalMediaFile {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub file_size: i64,
    /// None: the file is linked but not used for any episode.
    pub episode_number: Option<f64>,
    /// The number was set by the user and survives a re-scan.
    pub manual: bool,
    pub from_folder: bool,
    pub exists: bool,
}

/// A finished download of the app for this title.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalDownloadFile {
    pub episode_number: f64,
    pub file_path: String,
    pub file_name: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalMediaLink {
    pub anilist_id: i32,
    /// False when nothing is linked; downloads may still be listed.
    pub linked: bool,
    pub folder_path: Option<String>,
    pub files: Vec<LocalMediaFile>,
    pub downloads: Vec<LocalDownloadFile>,
    /// Episode numbers that have a file on disk right now, ascending.
    pub episodes: Vec<f64>,
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn has_extension(path: &Path, allowed: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| allowed.iter().any(|a| e.eq_ignore_ascii_case(a)))
        .unwrap_or(false)
}

pub fn is_video_file(path: &Path) -> bool {
    has_extension(path, &VIDEO_EXTENSIONS)
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

fn same_episode(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

/// Sorted, without repeats.
fn distinct_episodes(numbers: impl IntoIterator<Item = f64>) -> Vec<f64> {
    let mut list: Vec<f64> = numbers.into_iter().filter(|n| n.is_finite()).collect();
    list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    list.dedup_by(|a, b| same_episode(*a, *b));
    list
}

/// The episode a file is used for when the user has not chosen one.
pub fn episode_for_file_name(file_name: &str) -> Option<f64> {
    parse_episode_and_title(file_name)
        .1
        .filter(|n| n.is_finite() && *n >= 0.0)
}

// ---------------------------------------------------------------------------------------
// Database

async fn ensure_link(pool: &SqlitePool, anilist_id: i32) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO local_media_links (anilist_id, folder_path, updated_at)
        VALUES (?1, NULL, ?2)
        ON CONFLICT(anilist_id) DO UPDATE SET updated_at = excluded.updated_at
        "#,
    )
    .bind(anilist_id)
    .bind(current_unix_millis())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_folder(pool: &SqlitePool, anilist_id: i32) -> Result<Option<Option<String>>, AppError> {
    let row = sqlx::query("SELECT folder_path FROM local_media_links WHERE anilist_id = ?1")
        .bind(anilist_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<Option<String>, _>("folder_path")))
}

pub async fn list_files(pool: &SqlitePool, anilist_id: i32) -> Result<Vec<LocalMediaFile>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, file_path, file_size, episode_number, manual, from_folder
        FROM local_media_files
        WHERE anilist_id = ?1
        ORDER BY episode_number IS NULL, episode_number ASC, file_path ASC
        "#,
    )
    .bind(anilist_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let file_path: String = r.get("file_path");
            LocalMediaFile {
                id: r.get("id"),
                file_name: file_name_of(&file_path),
                exists: is_regular_file(Path::new(&file_path)),
                file_path,
                file_size: r.get("file_size"),
                episode_number: r.get("episode_number"),
                manual: r.get::<i64, _>("manual") != 0,
                from_folder: r.get::<i64, _>("from_folder") != 0,
            }
        })
        .collect())
}

pub async fn list_completed_downloads(
    pool: &SqlitePool,
    anilist_id: i32,
) -> Result<Vec<LocalDownloadFile>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT episode_num, file_path
        FROM downloads
        WHERE anilist_id = ?1 AND status = ?2 AND file_path IS NOT NULL AND file_path <> ''
        ORDER BY episode_num ASC, updated_at DESC
        "#,
    )
    .bind(anilist_id)
    .bind(STATUS_COMPLETED)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let file_path: String = r.get("file_path");
            LocalDownloadFile {
                episode_number: r.get("episode_num"),
                file_name: file_name_of(&file_path),
                exists: is_regular_file(Path::new(&file_path)) && is_video_file(Path::new(&file_path)),
                file_path,
            }
        })
        .collect())
}

pub async fn get_link(pool: &SqlitePool, anilist_id: i32) -> Result<LocalMediaLink, AppError> {
    let folder = get_folder(pool, anilist_id).await?;
    let files = list_files(pool, anilist_id).await?;
    let downloads = list_completed_downloads(pool, anilist_id).await?;
    let episodes = distinct_episodes(
        files
            .iter()
            .filter(|f| f.exists)
            .filter_map(|f| f.episode_number)
            .chain(downloads.iter().filter(|d| d.exists).map(|d| d.episode_number)),
    );
    Ok(LocalMediaLink {
        anilist_id,
        linked: folder.is_some(),
        folder_path: folder.flatten(),
        files,
        downloads,
        episodes,
    })
}

/// Records `scanned` for the title: new files get the number their name suggests, files whose
/// number the user did not set get it again, and with `folder` given, rows from an earlier scan
/// that are no longer in `scanned` are removed. A different folder than before drops every row
/// of the old one.
pub async fn save_scan(
    pool: &SqlitePool,
    anilist_id: i32,
    folder: Option<&str>,
    scanned: &[LocalAnimeFile],
) -> Result<(), AppError> {
    ensure_link(pool, anilist_id).await?;
    let mut tx = pool.begin().await?;

    if let Some(folder) = folder {
        let previous: Option<String> =
            sqlx::query("SELECT folder_path FROM local_media_links WHERE anilist_id = ?1")
                .bind(anilist_id)
                .fetch_one(&mut *tx)
                .await?
                .get("folder_path");
        if previous.as_deref() != Some(folder) {
            sqlx::query("DELETE FROM local_media_files WHERE anilist_id = ?1 AND from_folder = 1")
                .bind(anilist_id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE local_media_links SET folder_path = ?2, updated_at = ?3 WHERE anilist_id = ?1")
            .bind(anilist_id)
            .bind(folder)
            .bind(current_unix_millis())
            .execute(&mut *tx)
            .await?;
    }

    let from_folder = if folder.is_some() { 1 } else { 0 };
    let now = current_unix_millis();
    for file in scanned {
        let episode = episode_for_file_name(&file.file_name);
        sqlx::query(
            r#"
            INSERT INTO local_media_files
                (anilist_id, file_path, file_size, episode_number, manual, from_folder, added_at)
            VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)
            ON CONFLICT(anilist_id, file_path) DO UPDATE SET
                file_size = excluded.file_size,
                episode_number = CASE WHEN local_media_files.manual = 1
                                      THEN local_media_files.episode_number
                                      ELSE excluded.episode_number END,
                from_folder = MAX(local_media_files.from_folder, excluded.from_folder)
            "#,
        )
        .bind(anilist_id)
        .bind(&file.file_path)
        .bind(file.file_size as i64)
        .bind(episode)
        .bind(from_folder)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    if folder.is_some() {
        let keep: HashSet<&str> = scanned.iter().map(|f| f.file_path.as_str()).collect();
        let rows = sqlx::query(
            "SELECT id, file_path FROM local_media_files WHERE anilist_id = ?1 AND from_folder = 1",
        )
        .bind(anilist_id)
        .fetch_all(&mut *tx)
        .await?;
        for row in rows {
            let path: String = row.get("file_path");
            if !keep.contains(path.as_str()) {
                sqlx::query("DELETE FROM local_media_files WHERE id = ?1")
                    .bind(row.get::<i64, _>("id"))
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }

    tx.commit().await?;
    Ok(())
}

/// Sets the episode a linked file plays as (None: not used). Returns false for an unknown file.
pub async fn set_file_episode(
    pool: &SqlitePool,
    anilist_id: i32,
    file_id: i64,
    episode_number: Option<f64>,
) -> Result<bool, AppError> {
    if let Some(n) = episode_number {
        if !n.is_finite() || n < 0.0 {
            return Err(AppError::Other("Invalid episode number".to_string()));
        }
    }
    let result = sqlx::query(
        "UPDATE local_media_files SET episode_number = ?3, manual = 1 WHERE anilist_id = ?1 AND id = ?2",
    )
    .bind(anilist_id)
    .bind(file_id)
    .bind(episode_number)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn remove_file(pool: &SqlitePool, anilist_id: i32, file_id: i64) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM local_media_files WHERE anilist_id = ?1 AND id = ?2")
        .bind(anilist_id)
        .bind(file_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Forgets the folder and every linked file of the title. Files on disk are not touched.
pub async fn unlink(pool: &SqlitePool, anilist_id: i32) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM local_media_files WHERE anilist_id = ?1")
        .bind(anilist_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM local_media_links WHERE anilist_id = ?1")
        .bind(anilist_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// The file that plays `episode_number` of the title: a linked file (one the user numbered
/// first, then the largest), otherwise the newest finished download. Only files that exist
/// on disk and are videos count.
pub async fn episode_file(
    pool: &SqlitePool,
    anilist_id: i32,
    episode_number: f64,
) -> Result<Option<PathBuf>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT file_path FROM local_media_files
        WHERE anilist_id = ?1 AND episode_number IS NOT NULL AND ABS(episode_number - ?2) < 0.01
        ORDER BY manual DESC, file_size DESC, id ASC
        "#,
    )
    .bind(anilist_id)
    .bind(episode_number)
    .fetch_all(pool)
    .await?;
    let linked = rows.into_iter().map(|r| r.get::<String, _>("file_path"));

    let downloads = list_completed_downloads(pool, anilist_id)
        .await?
        .into_iter()
        .filter(|d| same_episode(d.episode_number, episode_number))
        .map(|d| d.file_path);

    Ok(linked
        .chain(downloads)
        .map(PathBuf::from)
        .find(|p| p.is_absolute() && is_video_file(p) && is_regular_file(p)))
}

// ---------------------------------------------------------------------------------------
// Sidecar subtitles

/// A subtitle file next to `video` that belongs to it: same folder, a name that is the
/// video's name without extension, optionally followed by `.<tag>` (`Show - 01.en.srt`).
pub fn is_sidecar_of(video: &Path, candidate: &Path) -> bool {
    if video.parent() != candidate.parent() || !has_extension(candidate, &SUBTITLE_EXTENSIONS) {
        return false;
    }
    let (Some(video_stem), Some(candidate_stem)) = (
        video.file_stem().and_then(|s| s.to_str()),
        candidate.file_stem().and_then(|s| s.to_str()),
    ) else {
        return false;
    };
    candidate_stem == video_stem
        || candidate_stem
            .strip_prefix(video_stem)
            .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1)
}

/// The label of a sidecar: its tag (`en`, `English`) or the format.
pub fn sidecar_label(video: &Path, subtitle: &Path) -> String {
    let format = subtitle
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_uppercase();
    let tag = match (
        video.file_stem().and_then(|s| s.to_str()),
        subtitle.file_stem().and_then(|s| s.to_str()),
    ) {
        (Some(v), Some(s)) => s.strip_prefix(v).map(|r| r.trim_start_matches('.').to_string()),
        _ => None,
    };
    match tag.filter(|t| !t.is_empty()) {
        Some(tag) => tag,
        None => format!("Subtitles ({})", format),
    }
}

/// Subtitle files next to `video`, sorted by name.
pub fn find_sidecars(video: &Path) -> Vec<PathBuf> {
    let Some(dir) = video.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_sidecar_of(video, p) && is_regular_file(p))
        .collect();
    found.sort();
    found
}

// ---------------------------------------------------------------------------------------
// Allow-list for the stream proxy

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayableKind {
    Video,
    Subtitle,
}

static PLAYABLE: Lazy<Mutex<HashMap<String, (PlayableKind, PathBuf)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn random_key() -> String {
    // RandomState is seeded from the OS; two independent states give 128 unpredictable bits.
    let part = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(current_unix_millis() as u128);
        hasher.finish()
    };
    format!("{:016x}{:016x}", part(), part())
}

/// Allows the proxy to serve `path` and returns the key it is served under. The same file
/// keeps its key for the session.
pub fn register_playable(kind: PlayableKind, path: &Path) -> String {
    let mut map = PLAYABLE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((key, _)) = map.iter().find(|(_, (k, p))| *k == kind && p == path) {
        return key.clone();
    }
    let key = random_key();
    map.insert(key.clone(), (kind, path.to_path_buf()));
    key
}

/// The file registered under `key` for `kind`, if it is still a regular file of that kind.
/// Anything that is not a registered key (a path, `..`, another kind's key) is refused.
pub fn playable_path(kind: PlayableKind, key: &str) -> Option<PathBuf> {
    if key.len() != 32 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let path = {
        let map = PLAYABLE.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(key) {
            Some((k, p)) if *k == kind => p.clone(),
            _ => return None,
        }
    };
    let allowed = match kind {
        PlayableKind::Video => is_video_file(&path),
        PlayableKind::Subtitle => has_extension(&path, &SUBTITLE_EXTENSIONS),
    };
    (allowed && is_regular_file(&path)).then_some(path)
}

/// The Content-Type a video file is served with.
pub fn video_content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        _ => "application/octet-stream",
    }
}

/// A parsed `Range: bytes=…` header for a file of `size` bytes: the inclusive byte range to
/// send, `Ok(None)` for the whole file (no header, or one this does not handle), and `Err(())`
/// when the range cannot be satisfied.
pub fn parse_byte_range(header: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = header.map(str::trim).and_then(|h| h.strip_prefix("bytes=")) else {
        return Ok(None);
    };
    // Several ranges are answered with the whole file, which is allowed.
    if spec.contains(',') {
        return Ok(None);
    }
    let Some((start, end)) = spec.trim().split_once('-') else {
        return Ok(None);
    };
    let (start, end) = (start.trim(), end.trim());
    if size == 0 {
        return Err(());
    }
    if start.is_empty() {
        let suffix: u64 = end.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok(Some((size.saturating_sub(suffix), size - 1)));
    }
    let start: u64 = start.parse().map_err(|_| ())?;
    if start >= size {
        return Err(());
    }
    let end = if end.is_empty() {
        size - 1
    } else {
        let end: u64 = end.parse().map_err(|_| ())?;
        if end < start {
            return Err(());
        }
        end.min(size - 1)
    };
    Ok(Some((start, end)))
}
