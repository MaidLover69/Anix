use crate::error::AppError;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};

pub const MAL_API_URL: &str = "https://api.myanimelist.net/v2";
pub const MAL_TOKEN_URL: &str = "https://myanimelist.net/v1/oauth2/token";

fn default_expires_in() -> i64 {
    3600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default = "default_expires_in")]
    pub expires_in: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MalUser {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub picture: Option<String>,
}

/// The viewer's list entry for one anime, in MAL vocabulary.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct MalListStatus {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub num_episodes_watched: i32,
    #[serde(default)]
    pub is_rewatching: bool,
}

#[derive(Deserialize)]
struct AnimeWithStatus {
    #[serde(default)]
    my_list_status: Option<MalListStatus>,
}

/// MAL's own description of a failure (`{"error": "...", "message": "..."}`), when it sent one.
fn error_detail(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let text = |key: &str| {
        json.get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    match (text("error"), text("message").or_else(|| text("hint"))) {
        (Some(error), Some(message)) => Some(format!("{}: {}", error, message)),
        (Some(only), None) | (None, Some(only)) => Some(only.to_string()),
        (None, None) => None,
    }
}

fn describe(status: u16, body: &str) -> String {
    match error_detail(body) {
        Some(detail) => format!("MyAnimeList HTTP {}: {}", status, detail),
        None => format!("MyAnimeList HTTP {}", status),
    }
}

/// The body of a successful API response; 401 is an auth failure, 429 a rate limit.
async fn api_body(response: Response) -> Result<String, AppError> {
    let status = response.status().as_u16();
    let body = response.text().await?;
    match status {
        200..=299 => Ok(body),
        401 => Err(AppError::AuthRequired),
        429 => Err(AppError::RateLimited { retry_after_s: 60 }),
        _ => Err(AppError::Network(describe(status, &body))),
    }
}

pub struct MalClient {
    client: Client,
    api_url: String,
    token_url: String,
}

impl MalClient {
    pub fn new(client: Client) -> Self {
        Self::with_base_urls(client, MAL_API_URL, MAL_TOKEN_URL)
    }

    /// A client for another API root and token endpoint (tests point it at a local server).
    pub fn with_base_urls(client: Client, api_url: &str, token_url: &str) -> Self {
        Self {
            client,
            api_url: api_url.trim_end_matches('/').to_string(),
            token_url: token_url.to_string(),
        }
    }

    async fn request_tokens(&self, params: &[(&str, &str)]) -> Result<MalTokenResponse, AppError> {
        let response = self.client.post(&self.token_url).form(params).send().await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        match status {
            200..=299 => serde_json::from_str(&body)
                .map_err(|e| AppError::Parse(format!("MyAnimeList token response: {}", e))),
            // The grant itself (code, verifier or refresh token) was refused.
            400 | 401 => {
                tracing::warn!("{}", describe(status, &body));
                Err(AppError::AuthRequired)
            }
            _ => Err(AppError::Network(describe(status, &body))),
        }
    }

    /// Trades an authorization code for the first token pair. No redirect URI is sent because
    /// the authorize request does not carry one either (the client has a single registered one).
    pub async fn exchange_code(
        &self,
        client_id: &str,
        code: &str,
        code_verifier: &str,
    ) -> Result<MalTokenResponse, AppError> {
        self.request_tokens(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", code_verifier),
        ])
        .await
    }

    /// A new token pair; the refresh token rotates. `AuthRequired` means MAL refused the
    /// refresh token, any other error that MAL could not be asked.
    pub async fn refresh_token(
        &self,
        client_id: &str,
        refresh_token: &str,
    ) -> Result<MalTokenResponse, AppError> {
        self.request_tokens(&[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .await
    }

    pub async fn get_user(&self, access_token: &str) -> Result<MalUser, AppError> {
        let response = self
            .client
            .get(format!("{}/users/@me", self.api_url))
            .bearer_auth(access_token)
            .send()
            .await?;
        let body = api_body(response).await?;
        serde_json::from_str(&body).map_err(|e| AppError::Parse(format!("MyAnimeList user: {}", e)))
    }

    /// The viewer's list entry for one anime, or `None` when it is not on their list.
    pub async fn get_list_status(
        &self,
        access_token: &str,
        mal_id: i32,
    ) -> Result<Option<MalListStatus>, AppError> {
        let response = self
            .client
            .get(format!("{}/anime/{}", self.api_url, mal_id))
            .query(&[("fields", "my_list_status")])
            .bearer_auth(access_token)
            .send()
            .await?;
        let body = api_body(response).await?;
        let anime: AnimeWithStatus = serde_json::from_str(&body)
            .map_err(|e| AppError::Parse(format!("MyAnimeList list status: {}", e)))?;
        Ok(anime.my_list_status.filter(|entry| entry.status.is_some()))
    }

    /// Adds the anime to the list or updates the given fields only. `status` is in MAL
    /// vocabulary (`watching`, `completed`, `on_hold`, `dropped`, `plan_to_watch`).
    pub async fn update_list_status(
        &self,
        access_token: &str,
        mal_id: i32,
        progress: Option<i32>,
        status: Option<&str>,
    ) -> Result<MalListStatus, AppError> {
        let mut form: Vec<(&str, String)> = Vec::new();
        if let Some(progress) = progress {
            form.push(("num_watched_episodes", progress.to_string()));
        }
        if let Some(status) = status {
            form.push(("status", status.to_string()));
        }

        let response = self
            .client
            .put(format!("{}/anime/{}/my_list_status", self.api_url, mal_id))
            .bearer_auth(access_token)
            .form(&form)
            .send()
            .await?;
        let body = api_body(response).await?;
        serde_json::from_str(&body)
            .map_err(|e| AppError::Parse(format!("MyAnimeList list update: {}", e)))
    }
}
