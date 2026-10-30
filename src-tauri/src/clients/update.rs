//! The update check: the project's website publishes the latest version as `version.json`.
//! When it also lists the release's AppImage with its SHA-256, an AppImage installation can
//! replace itself in place (after the user clicks Install); other packages open the download
//! page.

use crate::clients::http::APP_USER_AGENT;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The project's website. It serves `version.json` (the update check), the download page and
/// the link to the extension list. Set `ANIX_SITE_URL` when building to use another address.
pub const SITE_URL: &str = site_url(option_env!("ANIX_SITE_URL"));
const DEFAULT_SITE_URL: &str = "https://anix-linux.vercel.app";

const fn site_url(custom: Option<&'static str>) -> &'static str {
    match custom {
        Some(url) if !url.is_empty() => url,
        _ => DEFAULT_SITE_URL,
    }
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_DOCUMENT_BYTES: usize = 256 * 1024;

/// What the frontend's update dialog shows. `release_url` is opened in the user's browser.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub changelog: String,
    pub published_at: String,
    pub release_url: String,
    /// The release's AppImage, when the site lists one with its checksum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appimage: Option<AppImageAsset>,
}

/// `"appimage": { "url", "sha256", "size" }` in `version.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppImageAsset {
    pub url: String,
    /// Lower-case hex.
    pub sha256: String,
    pub size: Option<u64>,
}

impl UpdateInfo {
    pub fn new(version: String, changelog: String, published_at: String, release_url: String) -> Self {
        Self {
            version,
            changelog,
            published_at,
            release_url,
            appimage: None,
        }
    }
}

/// True when `candidate` is a higher dotted version than `current` ("1.3.0" > "1.2.9"), read
/// the same way as `isNewerVersion` in the frontend.
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches(['v', 'V'])
            .split(['.', '+', '-'])
            .map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(candidate), parts(current));
    for i in 0..a.len().max(b.len()).max(3) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

fn is_web_address(url: &str) -> bool {
    reqwest::Url::parse(url).map_or(false, |parsed| {
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().map_or(false, |host| !host.is_empty())
    })
}

/// The AppImage entry of `version.json`: a web address and a 64-digit SHA-256. Anything else
/// means there is nothing the in-app updater may install.
pub fn parse_appimage_asset(json: &serde_json::Value) -> Option<AppImageAsset> {
    let url = json["url"].as_str()?.trim();
    let sha256 = json["sha256"].as_str()?.trim().to_ascii_lowercase();
    if !is_web_address(url) || sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(AppImageAsset { url: url.to_string(), sha256, size: json["size"].as_u64().filter(|s| *s > 0) })
}

fn tidy_site(site: &str) -> &str {
    site.trim().trim_end_matches('/')
}

pub fn version_document_url(site: &str) -> String {
    format!("{}/version.json", tidy_site(site))
}

pub fn default_download_url(site: &str) -> String {
    format!("{}/download", tidy_site(site))
}

/// The latest published version, from the app's website.
pub async fn fetch_latest_release(client: &Client) -> Result<Option<UpdateInfo>, String> {
    fetch_update_info(client, SITE_URL).await
}

/// Reads `<site>/version.json`. `Ok(None)` means the site publishes no update information (the
/// document is not there); a site that cannot be reached, answers with an error or sends
/// something unreadable is an error, so the UI can tell it apart from "up to date".
pub async fn fetch_update_info(client: &Client, site: &str) -> Result<Option<UpdateInfo>, String> {
    let mut res = client
        .get(version_document_url(site))
        .header("User-Agent", APP_USER_AGENT)
        .header("Accept", "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("Could not reach the update server: {}", crate::error::AppError::from(e).reason()))?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(format!("The update server answered HTTP {}", res.status().as_u16()));
    }

    let mut body: Vec<u8> = Vec::new();
    loop {
        let chunk = res
            .chunk()
            .await
            .map_err(|e| format!("Could not read the update information: {}", crate::error::AppError::from(e).reason()))?;
        let Some(chunk) = chunk else { break };
        if body.len() + chunk.len() > MAX_DOCUMENT_BYTES {
            return Err("Unexpected response from the update server: the document is too large".to_string());
        }
        body.extend_from_slice(&chunk);
    }

    // A site without the document may answer every address with its own page.
    let text = String::from_utf8_lossy(&body);
    if text.trim_start().starts_with('<') {
        return Ok(None);
    }
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Unexpected response from the update server: {}", e))?;
    Ok(parse_version_document(&json, site))
}

/// Reads `{ "version", "notes", "published_at", "download_url" }`; everything but `version` is
/// optional. A document without a version is not a usable update.
pub fn parse_version_document(json: &serde_json::Value, site: &str) -> Option<UpdateInfo> {
    let version = json["version"].as_str()?.trim().trim_start_matches(['v', 'V']).to_string();
    if version.is_empty() {
        return None;
    }
    // The address is opened in the user's browser, so only web addresses are passed on.
    let release_url = json["download_url"]
        .as_str()
        .map(str::trim)
        .filter(|url| is_web_address(url))
        .map(String::from)
        .unwrap_or_else(|| default_download_url(site));
    let notes = json["notes"].as_str().map(str::trim).filter(|notes| !notes.is_empty());

    Some(UpdateInfo {
        version,
        changelog: notes.unwrap_or("No changelog available.").to_string(),
        published_at: json["published_at"].as_str().unwrap_or("").trim().to_string(),
        release_url,
        appimage: parse_appimage_asset(&json["appimage"]),
    })
}

// ---------------------------------------------------------------------------------------
// Installing an AppImage update
// ---------------------------------------------------------------------------------------

/// An AppImage is never larger than this; a bigger download is refused.
const MAX_APPIMAGE_BYTES: u64 = 1536 * 1024 * 1024;
const STALL_TIMEOUT: Duration = Duration::from_secs(60);

fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_or(false, |c| unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0)
}

/// The AppImage file the updater may replace: `$APPIMAGE` (`appimage`) names a file that can
/// be written, in a folder that can be written, and Anix is not running as root (`euid`).
/// Otherwise the reason, and the update dialog offers the download page instead.
pub fn appimage_target(appimage: Option<&std::ffi::OsStr>, euid: u32) -> Result<PathBuf, String> {
    let path = PathBuf::from(appimage.filter(|p| !p.is_empty()).ok_or("Anix is not running as an AppImage.")?);
    if euid == 0 {
        return Err("Anix is running as root; updates are only installed for a normal user.".to_string());
    }
    let folder = path.parent().filter(|_| path.is_absolute()).ok_or("The AppImage location is not a full path.")?;
    if !path.is_file() {
        return Err(format!("{} is not a file.", path.display()));
    }
    if !writable(&path) || !writable(folder) {
        return Err(format!("Anix cannot write to {}.", path.display()));
    }
    Ok(path)
}

/// The SHA-256 of a file, in lower-case hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

/// Where the previous version is kept after an update: `<name>.bak` next to it.
pub fn backup_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    target.with_file_name(name)
}

/// Puts `downloaded` in place of `target` once its SHA-256 is `sha256`. The old file is kept
/// as [`backup_path`] and replaced in one rename, so `target` is always a whole AppImage. On a
/// checksum mismatch `downloaded` is deleted and `target` is left as it was.
pub fn install_verified(downloaded: &Path, target: &Path, sha256: &str) -> Result<PathBuf, String> {
    let fail = |message: String| {
        let _ = std::fs::remove_file(downloaded);
        Err(message)
    };
    match sha256_file(downloaded) {
        Ok(hash) if hash.eq_ignore_ascii_case(sha256.trim()) => {}
        Ok(_) => return fail("The downloaded update does not match its checksum, so it was not installed. Anix is unchanged.".to_string()),
        Err(e) => return fail(format!("The downloaded update could not be read: {}", e)),
    }

    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(target).map(|m| m.permissions().mode()).unwrap_or(0o755) | 0o755;
    if let Err(e) = std::fs::set_permissions(downloaded, std::fs::Permissions::from_mode(mode & 0o7777)) {
        return fail(format!("The update could not be made executable: {}", e));
    }
    // Synced after the chmod, so a crash after the rename cannot leave a whole AppImage that
    // no longer runs.
    if let Err(e) = std::fs::File::open(downloaded).and_then(|f| f.sync_all()) {
        return fail(format!("Could not save the update: {}", e));
    }
    let backup = backup_path(target);
    let _ = std::fs::remove_file(&backup);
    if std::fs::hard_link(target, &backup).is_err() {
        // A copied backup is synced like the update, so a crash cannot leave both truncated.
        let copied = std::fs::copy(target, &backup).and_then(|_| std::fs::File::open(&backup)?.sync_all());
        if let Err(e) = copied {
            return fail(format!("The current version could not be backed up, so nothing was changed: {}", e));
        }
    }
    if let Err(e) = std::fs::rename(downloaded, target) {
        return fail(format!("The update could not replace {}: {}", target.display(), e));
    }
    // Makes the rename and the backup's name durable too. The update is already in place, so
    // a failure here is only logged.
    if let Some(Err(e)) = target.parent().map(|dir| std::fs::File::open(dir).and_then(|dir| dir.sync_all())) {
        tracing::warn!("The folder of the updated AppImage could not be synced: {}", e);
    }
    Ok(backup)
}

/// Downloads `asset` next to `target` and installs it with [`install_verified`]. `progress`
/// hears the fraction done. Returns the backup of the previous version.
pub async fn download_and_install(
    client: &Client,
    asset: &AppImageAsset,
    target: &Path,
    mut progress: impl FnMut(f64),
) -> Result<PathBuf, String> {
    let mut name = std::ffi::OsString::from(".");
    name.push(target.file_name().unwrap_or_default());
    name.push(".anix-update");
    let partial = target.with_file_name(name);

    let result = async {
        let mut res = client
            .get(&asset.url)
            .header("User-Agent", APP_USER_AGENT)
            .send()
            .await
            .map_err(|e| format!("Could not download the update: {}", crate::error::AppError::from(e).reason()))?;
        if !res.status().is_success() {
            return Err(format!("The update download answered HTTP {}", res.status().as_u16()));
        }
        let total = asset.size.or(res.content_length()).filter(|t| *t > 0);
        let mut file = tokio::fs::File::create(&partial)
            .await
            .map_err(|e| format!("Could not save the update next to {}: {}", target.display(), e))?;
        let mut written: u64 = 0;
        loop {
            let chunk = match tokio::time::timeout(STALL_TIMEOUT, res.chunk()).await {
                Ok(Ok(Some(chunk))) => chunk,
                Ok(Ok(None)) => break,
                Ok(Err(e)) => return Err(format!("The update download was interrupted: {}", e)),
                Err(_) => return Err("The update download stalled.".to_string()),
            };
            written += chunk.len() as u64;
            if written > MAX_APPIMAGE_BYTES {
                return Err("The update download is larger than any AppImage should be.".to_string());
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| format!("Could not save the update: {}", e))?;
            if let Some(total) = total {
                progress((written as f64 / total as f64).min(1.0));
            }
        }
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|e| format!("Could not save the update: {}", e))?;
        // On disk before it is renamed over the AppImage: after a crash or power loss the
        // rename must never leave a truncated file at the AppImage's path.
        file.sync_all()
            .await
            .map_err(|e| format!("Could not save the update: {}", e))?;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(e);
    }

    let (partial, target, sha256) = (partial.clone(), target.to_path_buf(), asset.sha256.clone());
    tokio::task::spawn_blocking(move || install_verified(&partial, &target, &sha256))
        .await
        .map_err(|e| e.to_string())?
}
