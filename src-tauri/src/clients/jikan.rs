use crate::cache::L1Cache;
use crate::error::AppError;
use once_cell::sync::Lazy;
use reqwest::header::RETRY_AFTER;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

const JIKAN_BASE: &str = "https://api.jikan.moe/v4";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Attempts per request when Jikan answers 429.
const MAX_ATTEMPTS: u32 = 4;
const MAX_RETRY_WAIT: Duration = Duration::from_secs(4);
const PAGE_DELAY: Duration = Duration::from_millis(350);
const MAX_EPISODE_PAGES: u32 = 20;

const CACHE_TTL_SECS: u64 = 30 * 60;
/// A list cut short by an error is kept only this long, so the missing pages are retried soon.
const PARTIAL_LIST_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MalAnimeDetails {
    pub mal_id: i32,
    pub title: String,
    pub title_english: Option<String>,
    pub title_japanese: Option<String>,
    pub synopsis: Option<String>,
    pub score: Option<f64>,
    pub episodes: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MalEpisodeInfo {
    pub number: f64,
    pub title: Option<String>,
    pub title_japanese: Option<String>,
    pub title_romanji: Option<String>,
    pub filler: bool,
    pub synopsis: Option<String>,
}

#[derive(Clone)]
struct CachedEpisodes {
    list: Arc<Vec<MalEpisodeInfo>>,
    complete: bool,
    fetched_at: Instant,
}

static EPISODES_CACHE: Lazy<L1Cache<i32, CachedEpisodes>> =
    Lazy::new(|| L1Cache::new(300, CACHE_TTL_SECS));
static DETAILS_CACHE: Lazy<L1Cache<i32, MalAnimeDetails>> =
    Lazy::new(|| L1Cache::new(300, CACHE_TTL_SECS));

/// How long to wait before retrying a rate-limited request: the server's `Retry-After` when it
/// sends one, otherwise a growing pause, never longer than `MAX_RETRY_WAIT`.
pub fn retry_delay(attempt: u32, retry_after_s: Option<u64>) -> Duration {
    let wanted = match retry_after_s {
        Some(seconds) => Duration::from_secs(seconds.max(1)),
        None => Duration::from_millis(1000 * (attempt as u64 + 1)),
    };
    wanted.min(MAX_RETRY_WAIT)
}

/// GETs a Jikan URL, waiting and retrying while rate-limited. `Ok(None)` is a 404.
async fn jikan_get(client: &Client, url: &str) -> Result<Option<serde_json::Value>, AppError> {
    let mut retry_after_s = None;
    for attempt in 0..MAX_ATTEMPTS {
        let response = client
            .get(url)
            .header("User-Agent", crate::clients::http::APP_USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            return Ok(Some(response.json().await?));
        }
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status != StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::Network(format!("Jikan HTTP {}", status.as_u16())));
        }
        retry_after_s = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        if attempt + 1 < MAX_ATTEMPTS {
            sleep(retry_delay(attempt, retry_after_s)).await;
        }
    }
    Err(AppError::RateLimited {
        retry_after_s: retry_after_s.unwrap_or(2),
    })
}

pub async fn fetch_mal_details(
    client: &Client,
    mal_id: i32,
) -> Result<MalAnimeDetails, AppError> {
    DETAILS_CACHE
        .try_get_with(mal_id, async move {
            let url = format!("{}/anime/{}", JIKAN_BASE, mal_id);
            let json = jikan_get(client, &url)
                .await?
                .ok_or_else(|| AppError::Other(format!("MyAnimeList has no anime {}", mal_id)))?;
            let data = &json["data"];
            Ok::<_, AppError>(MalAnimeDetails {
                mal_id,
                title: data["title"].as_str().unwrap_or("").to_string(),
                title_english: data["title_english"].as_str().map(String::from),
                title_japanese: data["title_japanese"].as_str().map(String::from),
                synopsis: data["synopsis"].as_str().map(String::from),
                score: data["score"].as_f64(),
                episodes: data["episodes"].as_i64().map(|v| v as i32),
            })
        })
        .await
        .map_err(|shared| (*shared).clone())
}

fn parse_episode(ep: &serde_json::Value) -> MalEpisodeInfo {
    MalEpisodeInfo {
        number: ep["mal_id"].as_f64().unwrap_or(0.0),
        title: ep["title"].as_str().map(String::from),
        title_japanese: ep["title_japanese"].as_str().map(String::from),
        title_romanji: ep["title_romanji"].as_str().map(String::from),
        filler: ep["filler"].as_bool().unwrap_or(false),
        synopsis: ep["synopsis"].as_str().map(String::from),
    }
}

async fn load_episodes(client: &Client, mal_id: i32) -> Result<CachedEpisodes, AppError> {
    let mut list = Vec::new();
    let mut complete = true;
    let mut page = 1;

    loop {
        let url = format!("{}/anime/{}/episodes?page={}", JIKAN_BASE, mal_id, page);
        let json = match jikan_get(client, &url).await {
            Ok(Some(json)) => json,
            Ok(None) => break,
            // Pages already fetched are kept; the list is just marked as cut short.
            Err(e) if !list.is_empty() => {
                tracing::warn!(
                    "Jikan episode list for {} stopped at page {}: {}",
                    mal_id,
                    page,
                    e.reason()
                );
                complete = false;
                break;
            }
            Err(e) => return Err(e),
        };

        if let Some(episodes) = json["data"].as_array() {
            list.extend(episodes.iter().map(parse_episode));
        }

        let has_next_page = json["pagination"]["has_next_page"].as_bool().unwrap_or(false);
        if !has_next_page || page >= MAX_EPISODE_PAGES {
            break;
        }
        page += 1;
        sleep(PAGE_DELAY).await;
    }

    Ok(CachedEpisodes {
        list: Arc::new(list),
        complete,
        fetched_at: Instant::now(),
    })
}

/// MyAnimeList's episode list for an anime. Results are cached, and concurrent callers for the
/// same anime share one walk through the pages instead of rate-limiting each other.
pub async fn fetch_mal_episodes(
    client: &Client,
    mal_id: i32,
) -> Result<Vec<MalEpisodeInfo>, AppError> {
    if let Some(hit) = EPISODES_CACHE.get(&mal_id).await {
        if hit.complete || hit.fetched_at.elapsed() < PARTIAL_LIST_TTL {
            return Ok((*hit.list).clone());
        }
        EPISODES_CACHE.invalidate(&mal_id).await;
    }
    let cached = EPISODES_CACHE
        .try_get_with(mal_id, load_episodes(client, mal_id))
        .await
        .map_err(|shared| (*shared).clone())?;
    Ok((*cached.list).clone())
}

pub async fn fetch_filler_episodes(
    client: &Client,
    mal_id: i32,
) -> Result<Vec<i32>, AppError> {
    let eps = fetch_mal_episodes(client, mal_id).await?;
    let fillers: Vec<i32> = eps
        .into_iter()
        .filter(|e| e.filler)
        .map(|e| e.number as i32)
        .collect();
    Ok(fillers)
}
