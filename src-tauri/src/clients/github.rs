use reqwest::Client;
use serde::{Deserialize, Serialize};

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

const RELEASES_API: &str = "https://api.github.com/repos/MaidLover69/Anilili_Linux/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/MaidLover69/Anilili_Linux/releases";

/// The latest published release of this app. `Ok(None)` means there is no release yet;
/// a network or API failure is an error, so the UI can tell it apart from "up to date".
pub async fn fetch_latest_release(client: &Client) -> Result<Option<UpdateInfo>, String> {
    let res = client
        .get(RELEASES_API)
        .header("User-Agent", "AnililiLinux/1.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {}", e))?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(format!("GitHub answered HTTP {}", res.status().as_u16()));
    }

    let json: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Unexpected response from GitHub: {}", e))?;
    Ok(parse_release(&json))
}

/// Reads a GitHub release object; a release without a tag is not a usable update.
pub fn parse_release(json: &serde_json::Value) -> Option<UpdateInfo> {
    let raw_tag = json["tag_name"].as_str()?;
    let version = raw_tag.trim_start_matches('v').to_string();
    if version.is_empty() {
        return None;
    }
    // Only ever link to GitHub: the URL is opened in the user's browser.
    let release_url = json["html_url"]
        .as_str()
        .filter(|url| url.starts_with("https://github.com/"))
        .unwrap_or(RELEASES_PAGE)
        .to_string();

    Some(UpdateInfo {
        version,
        changelog: json["body"].as_str().unwrap_or("No changelog available.").to_string(),
        published_at: json["published_at"].as_str().unwrap_or("").to_string(),
        release_url,
    })
}
