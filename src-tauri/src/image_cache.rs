//! Covers and banners saved on disk, so entries still have pictures when the catalogues and
//! the image hosts cannot be reached. The folder holds at most the size set in Settings
//! (`image_cache_mb`, 200 MB by default); when it grows past that, the oldest files go first.
//! File names are a hash of the address, so no address text ends up in a path.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_LIMIT_MB: u64 = 200;
pub const MIN_LIMIT_MB: u64 = 20;
pub const MAX_LIMIT_MB: u64 = 5000;
pub const SETTING_KEY: &str = "image_cache_mb";
/// One picture larger than this is not saved (covers are well under a megabyte).
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MIB: u64 = 1024 * 1024;

/// The picture at `url`, or none when it is not a picture, the answer is not a success, or it is
/// larger than `max` bytes (judged by `Content-Length`, then by what actually arrives).
pub async fn fetch_bytes(client: &reqwest::Client, url: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
    let mut response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let is_image = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("image/"));
    if !response.status().is_success() || !is_image || response.content_length().is_some_and(|n| n > max as u64) {
        return Ok(None);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > max {
            return Ok(None);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Some(body))
}

/// `fetch_bytes` for an address from the catalogues or an extension: public hosts only, also
/// across redirects.
pub async fn fetch_public(url: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    crate::clients::net_guard::check_public_url(&parsed)?;
    let client = crate::clients::net_guard::public_http_client().map_err(|e| e.to_string())?;
    fetch_bytes(&client, url, max).await
}

/// The limit in bytes from the settings object: the default when the key is missing or not a
/// number, and always within the allowed range.
pub fn limit_bytes(settings: &Value) -> u64 {
    let mb = settings.get(SETTING_KEY).and_then(Value::as_f64).filter(|v| v.is_finite()).map_or(DEFAULT_LIMIT_MB, |v| {
        (v.max(0.0) as u64).clamp(MIN_LIMIT_MB, MAX_LIMIT_MB)
    });
    mb * MIB
}

/// The folder the pictures live in.
pub fn dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join(crate::db::APP_DIR_NAME).join("images"))
}

/// Where the picture at `url` is (or would be) kept: a hash name directly inside `dir`.
pub fn path_for(dir: &Path, url: &str) -> PathBuf {
    let hash = Sha256::digest(url.as_bytes());
    dir.join(hash.iter().take(16).map(|b| format!("{:02x}", b)).collect::<String>())
}

pub fn find(dir: &Path, url: &str) -> Option<PathBuf> {
    Some(path_for(dir, url)).filter(|path| path.is_file())
}

fn files(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(|m| m.is_file())?;
            Some((e.path(), meta.len(), meta.modified().unwrap_or(std::time::UNIX_EPOCH)))
        })
        .collect()
}

pub fn used_bytes(dir: &Path) -> u64 {
    files(dir).iter().map(|(_, len, _)| len).sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PruneReport {
    pub removed: usize,
    pub freed: u64,
    pub kept_bytes: u64,
}

/// Removes the oldest files until the folder fits `limit_bytes`. A missing folder is empty.
pub fn prune(dir: &Path, limit_bytes: u64) -> io::Result<PruneReport> {
    let mut list = files(dir);
    let mut kept: u64 = list.iter().map(|(_, len, _)| len).sum();
    list.sort_by_key(|(_, _, modified)| *modified);
    let (mut removed, mut freed) = (0, 0);
    for (path, len, _) in list {
        if kept <= limit_bytes {
            break;
        }
        fs::remove_file(&path)?;
        kept -= len;
        freed += len;
        removed += 1;
    }
    Ok(PruneReport { removed, freed, kept_bytes: kept })
}

/// Writes the picture, then prunes the folder; the new file is the newest, so it stays unless
/// it alone is larger than the limit.
pub fn save(dir: &Path, url: &str, bytes: &[u8], limit_bytes: u64) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = path_for(dir, url);
    fs::write(&path, bytes)?;
    prune(dir, limit_bytes)?;
    Ok(path)
}
