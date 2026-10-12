//! The update check: the project's website publishes the latest version as `version.json`.

use crate::clients::http::APP_USER_AGENT;
use reqwest::Client;
use serde::{Deserialize, Serialize};
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
}

impl UpdateInfo {
    pub fn new(version: String, changelog: String, published_at: String, release_url: String) -> Self {
        Self {
            version,
            changelog,
            published_at,
            release_url,
        }
    }
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
        .filter(|url| {
            reqwest::Url::parse(url).map_or(false, |parsed| {
                matches!(parsed.scheme(), "http" | "https") && parsed.host_str().map_or(false, |host| !host.is_empty())
            })
        })
        .map(String::from)
        .unwrap_or_else(|| default_download_url(site));
    let notes = json["notes"].as_str().map(str::trim).filter(|notes| !notes.is_empty());

    Some(UpdateInfo {
        version,
        changelog: notes.unwrap_or("No changelog available.").to_string(),
        published_at: json["published_at"].as_str().unwrap_or("").trim().to_string(),
        release_url,
    })
}
