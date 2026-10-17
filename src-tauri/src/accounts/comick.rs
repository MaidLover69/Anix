//! The Comick integrations API (`https://api.comick.dev/integrations/v1`) and its OAuth token
//! endpoints, for a public client.
//!
//! Titles are identified by opaque string HIDs. Access tokens last 15 minutes; refresh tokens
//! last 30 days and rotate on every successful refresh, the old one becoming invalid at once.

use super::oauth;
use crate::error::AppError;
use reqwest::header::RETRY_AFTER;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_expires_in() -> i64 {
    900
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ComickTokens {
    pub access_token: String,
    /// Missing when the user did not allow offline access.
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default = "default_expires_in")]
    pub expires_in: i64,
    /// Space-separated granted scopes.
    #[serde(default)]
    pub scope: Option<String>,
}

impl ComickTokens {
    pub fn can_write(&self) -> bool {
        scope_allows_writes(self.scope.as_deref())
    }
}

/// Whether a granted scope string includes `library:write`. An absent scope is taken to be
/// what was asked for.
pub fn scope_allows_writes(scope: Option<&str>) -> bool {
    match scope {
        None => true,
        Some(scope) => scope.split_whitespace().any(|s| s == "library:write"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ComickUser {
    /// Public account UUID.
    pub id: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ComickProgress {
    /// Kept as text: Comick allows "12.5", "0" and labels that are not numbers.
    #[serde(default)]
    pub number: Option<String>,
}

/// One entry of the connected user's library.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ComickEntry {
    pub hid: String,
    #[serde(default)]
    pub title: String,
    /// 1 watching, 2 completed, 3 on hold, 4 dropped, 5 plan to watch.
    #[serde(default)]
    pub status: i64,
    #[serde(default)]
    pub progress: Option<ComickProgress>,
    #[serde(default)]
    pub rating: Option<f64>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub total_units: Option<i32>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct ExternalIds {
    #[serde(default)]
    pub anilist: Option<String>,
    #[serde(default)]
    pub myanimelist: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct ComickTitle {
    pub hid: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub external_ids: ExternalIds,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct ComickLibraryPage {
    #[serde(default)]
    pub data: Vec<ComickEntry>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// The body of a PUT to `/me/library/{hid}`; omitted fields keep their remote value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ComickWrite {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ComickProgress>,
}

/// Why a refresh did not produce new tokens.
#[derive(Debug, Clone)]
pub enum RefreshError {
    /// Comick refused the refresh token (`invalid_grant` and the like): reconnect.
    Refused,
    /// Comick did not process the request (unreachable, rate limited, unavailable); the stored
    /// refresh token is still the current one.
    NotProcessed(AppError),
    /// The request may have been processed but its answer was lost, so the stored refresh
    /// token may already be invalid. Using it again could revoke the whole token family.
    Lost(AppError),
}

/// Comick's own description of a failure, when it sent one.
fn error_detail(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    let text = |key: &str| {
        json.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    match (text("error"), text("error_description").or_else(|| text("message"))) {
        (Some(error), Some(message)) => Some(format!("{}: {}", error, message)),
        (Some(only), None) | (None, Some(only)) => Some(only.to_string()),
        (None, None) => None,
    }
}

fn describe(status: u16, body: &str) -> String {
    match error_detail(body) {
        Some(detail) => format!("Comick HTTP {}: {}", status, detail),
        None => format!("Comick HTTP {}", status),
    }
}

fn retry_after(response: &Response) -> u64 {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(60)
}

/// The body of an API response. `Ok(None)` is a 404, which the caller interprets.
async fn api_body(response: Response) -> Result<Option<String>, AppError> {
    let status = response.status().as_u16();
    let wait = retry_after(&response);
    let body = response.text().await?;
    match status {
        200..=299 => Ok(Some(body)),
        401 => Err(AppError::AuthRequired),
        404 => Ok(None),
        429 => Err(AppError::RateLimited { retry_after_s: wait }),
        _ => Err(AppError::Network(describe(status, &body))),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(body: &str, what: &str) -> Result<T, AppError> {
    serde_json::from_str(body).map_err(|e| AppError::Parse(format!("Comick {}: {}", what, e)))
}

fn not_found(what: &str) -> AppError {
    AppError::Network(format!("Comick HTTP 404: {} not found", what))
}

#[derive(Deserialize)]
struct Wrapped<T> {
    data: T,
}

/// Up to 100 characters of `text`, the longest search Comick accepts.
fn search_text(text: &str) -> String {
    text.trim().chars().take(100).collect()
}

pub struct ComickClient {
    client: Client,
    api_url: String,
    token_url: String,
    revoke_url: String,
    client_id: Option<String>,
}

impl ComickClient {
    /// The real endpoints and the client id of this build.
    pub fn new(client: Client) -> Self {
        Self::with_urls(
            client,
            oauth::COMICK_API_URL,
            oauth::COMICK_TOKEN_URL,
            oauth::COMICK_REVOKE_URL,
            oauth::comick_client_id(),
        )
    }

    /// Other endpoints and client id (tests point it at a local server).
    pub fn with_urls(
        client: Client,
        api_url: &str,
        token_url: &str,
        revoke_url: &str,
        client_id: Option<&str>,
    ) -> Self {
        Self {
            client,
            api_url: api_url.trim_end_matches('/').to_string(),
            token_url: token_url.to_string(),
            revoke_url: revoke_url.to_string(),
            client_id: client_id.map(String::from),
        }
    }

    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    fn require_client_id(&self) -> Result<&str, AppError> {
        self.client_id()
            .ok_or_else(|| AppError::Other("Comick is not available in this build.".to_string()))
    }

    async fn post_token_form(&self, params: &[(&str, &str)]) -> Result<Response, AppError> {
        Ok(self.client.post(&self.token_url).form(params).send().await?)
    }

    /// Trades the authorization code of a validated callback for the first tokens.
    pub async fn exchange_code(&self, code: &str, code_verifier: &str) -> Result<ComickTokens, AppError> {
        let client_id = self.require_client_id()?;
        let response = self
            .post_token_form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client_id),
                ("code", code),
                ("redirect_uri", oauth::COMICK_REDIRECT_URI),
                ("code_verifier", code_verifier),
                ("resource", oauth::COMICK_RESOURCE),
            ])
            .await?;
        let status = response.status().as_u16();
        let wait = retry_after(&response);
        let body = response.text().await?;
        match status {
            200..=299 => parse(&body, "token response"),
            400 | 401 => {
                tracing::warn!("{}", describe(status, &body));
                Err(AppError::AuthRequired)
            }
            429 => Err(AppError::RateLimited { retry_after_s: wait }),
            _ => Err(AppError::Network(describe(status, &body))),
        }
    }

    /// New tokens for `refresh_token`. Only one refresh may run at a time per connection, and
    /// the caller must store both returned tokens in one write.
    pub async fn refresh(&self, refresh_token: &str) -> Result<ComickTokens, RefreshError> {
        let client_id = self.require_client_id().map_err(RefreshError::NotProcessed)?;
        let sent = self
            .post_token_form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("refresh_token", refresh_token),
                ("resource", oauth::COMICK_RESOURCE),
            ])
            .await;
        let response = match sent {
            Ok(response) => response,
            // No connection was made: nothing reached Comick.
            Err(e @ AppError::Unreachable(_)) => return Err(RefreshError::NotProcessed(e)),
            Err(e) => return Err(RefreshError::Lost(e)),
        };
        let status = response.status().as_u16();
        let wait = retry_after(&response);
        let body = match response.text().await {
            Ok(body) => body,
            Err(e) if (200..300).contains(&status) => return Err(RefreshError::Lost(e.into())),
            Err(e) => return Err(RefreshError::NotProcessed(e.into())),
        };
        match status {
            200..=299 => parse(&body, "token response").map_err(RefreshError::Lost),
            400 | 401 => {
                tracing::warn!("{}", describe(status, &body));
                Err(RefreshError::Refused)
            }
            429 => Err(RefreshError::NotProcessed(AppError::RateLimited { retry_after_s: wait })),
            _ => Err(RefreshError::NotProcessed(AppError::Network(describe(status, &body)))),
        }
    }

    /// Revokes one token; `hint` is `access_token` or `refresh_token`.
    pub async fn revoke(&self, token: &str, hint: &str) -> Result<(), AppError> {
        let client_id = self.require_client_id()?;
        let response = self
            .client
            .post(&self.revoke_url)
            .form(&[("client_id", client_id), ("token", token), ("token_type_hint", hint)])
            .send()
            .await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        match status {
            200..=299 => Ok(()),
            _ => Err(AppError::Network(describe(status, &body))),
        }
    }

    pub async fn me(&self, access_token: &str) -> Result<ComickUser, AppError> {
        let response = self
            .client
            .get(format!("{}/me", self.api_url))
            .bearer_auth(access_token)
            .send()
            .await?;
        let body = api_body(response).await?.ok_or_else(|| not_found("the integrations API"))?;
        parse(&body, "user")
    }

    /// The user's entry for one title, or `None` when the title is not in their library.
    pub async fn get_entry(&self, access_token: &str, hid: &str) -> Result<Option<ComickEntry>, AppError> {
        let response = self
            .client
            .get(format!("{}/me/library/{}", self.api_url, urlencoding::encode(hid)))
            .bearer_auth(access_token)
            .send()
            .await?;
        match api_body(response).await? {
            Some(body) => Ok(Some(parse::<Wrapped<ComickEntry>>(&body, "library entry")?.data)),
            None => Ok(None),
        }
    }

    /// Adds the title or updates the fields given. `status` is required when adding.
    pub async fn put_entry(
        &self,
        access_token: &str,
        hid: &str,
        write: &ComickWrite,
    ) -> Result<ComickEntry, AppError> {
        let response = self
            .client
            .put(format!("{}/me/library/{}", self.api_url, urlencoding::encode(hid)))
            .bearer_auth(access_token)
            .json(write)
            .send()
            .await?;
        let body = api_body(response).await?.ok_or_else(|| not_found("title"))?;
        Ok(parse::<Wrapped<ComickEntry>>(&body, "library update")?.data)
    }

    /// Anime whose title or alternative titles match `query`.
    pub async fn search_anime(&self, access_token: &str, query: &str) -> Result<Vec<ComickTitle>, AppError> {
        let query = search_text(query);
        if query.chars().count() < 2 {
            return Ok(Vec::new());
        }
        let response = self
            .client
            .get(format!("{}/titles", self.api_url))
            .query(&[("q", query.as_str()), ("media_type", "anime"), ("limit", "20")])
            .bearer_auth(access_token)
            .send()
            .await?;
        let body = api_body(response).await?.ok_or_else(|| not_found("the title search"))?;
        Ok(parse::<Wrapped<Vec<ComickTitle>>>(&body, "title search")?.data)
    }

    /// One page of the user's anime library.
    pub async fn library_page(
        &self,
        access_token: &str,
        cursor: Option<&str>,
    ) -> Result<ComickLibraryPage, AppError> {
        let mut query = vec![("media_type", "anime"), ("limit", "100")];
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor));
        }
        let response = self
            .client
            .get(format!("{}/me/library", self.api_url))
            .query(&query)
            .bearer_auth(access_token)
            .send()
            .await?;
        let body = api_body(response).await?.ok_or_else(|| not_found("the library"))?;
        parse(&body, "library")
    }
}

// ---- Matching and vocabulary ----------------------------------------------------------------

/// The HID of the result that is `anilist_id` according to Comick's external ids, or failing
/// that the one that is `mal_id`.
pub fn match_hid(results: &[ComickTitle], anilist_id: i32, mal_id: Option<i32>) -> Option<String> {
    let is = |value: &Option<String>, id: i32| {
        value
            .as_deref()
            .map(str::trim)
            .and_then(|v| v.parse::<i64>().ok())
            == Some(id as i64)
    };
    results
        .iter()
        .find(|title| is(&title.external_ids.anilist, anilist_id))
        .or_else(|| {
            let mal_id = mal_id.filter(|id| *id > 0)?;
            results.iter().find(|title| {
                title.external_ids.anilist.as_deref().map_or(true, |v| v.trim().is_empty())
                    && is(&title.external_ids.myanimelist, mal_id)
            })
        })
        .map(|title| title.hid.clone())
        .filter(|hid| !hid.is_empty())
}

/// Comick's status number for an AniList list status.
pub fn comick_status(anilist_status: &str) -> u8 {
    match anilist_status {
        "CURRENT" | "REPEATING" => 1,
        "COMPLETED" => 2,
        "PAUSED" => 3,
        "DROPPED" => 4,
        _ => 5,
    }
}

pub fn anilist_status_from_comick(status: i64) -> &'static str {
    match status {
        1 => "CURRENT",
        2 => "COMPLETED",
        3 => "PAUSED",
        4 => "DROPPED",
        _ => "PLANNING",
    }
}

/// Saved progress as a whole episode count: `Ok(0)` when nothing is saved, the whole part of a
/// number such as "12.5", and `Err(())` for a label that is not a number, which must be left
/// alone rather than overwritten.
pub fn progress_episodes(progress: Option<&ComickProgress>) -> Result<i32, ()> {
    let Some(number) = progress.and_then(|p| p.number.as_deref()).map(str::trim) else {
        return Ok(0);
    };
    if number.is_empty() {
        return Ok(0);
    }
    match number.parse::<f64>() {
        Ok(value) if value.is_finite() && value >= 0.0 => Ok(value.floor().min(i32::MAX as f64) as i32),
        _ => Err(()),
    }
}
