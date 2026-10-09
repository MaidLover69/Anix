use crate::clients::http::http_client;
use crate::error::AppError;
use crate::models::EpisodeItem;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KonohaEpisode {
    pub number: f64,
    pub title: Option<String>,
    pub image: Option<String>,
    pub filler: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KonohaResponse {
    pub episodes: Vec<KonohaEpisode>,
}

pub async fn fetch_konoha_episodes(anilist_id: i64) -> Result<Vec<EpisodeItem>, AppError> {
    let client = http_client()?;
    let cdn_url = format!(
        "https://cdn.jsdelivr.net/gh/Konoha-orgs/Konoha/anime/{}.json",
        anilist_id
    );
    let fallback_url = format!(
        "https://raw.githubusercontent.com/Konoha-orgs/Konoha/main/anime/{}.json",
        anilist_id
    );

    // The CDN mirror first, the repository itself when the CDN fails or lacks the file.
    let mut response = client.get(&cdn_url).timeout(REQUEST_TIMEOUT).send().await;
    if !matches!(&response, Ok(r) if r.status().is_success()) {
        response = client.get(&fallback_url).timeout(REQUEST_TIMEOUT).send().await;
    }
    let response = response?;
    if response.status() == StatusCode::NOT_FOUND {
        // The anime is not part of the Konoha dataset.
        return Ok(Vec::new());
    }
    if !response.status().is_success() {
        return Err(AppError::Network(format!(
            "Konoha HTTP {}",
            response.status().as_u16()
        )));
    }

    let data: KonohaResponse = response
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("Konoha episode list: {}", e)))?;

    let items = data
        .episodes
        .into_iter()
        .map(|ep| EpisodeItem {
            pipe_id: format!("{}-{}", anilist_id, crate::providers::common::fmt_ep(ep.number)),
            number: ep.number,
            title: ep.title,
            image: ep.image,
            synopsis: None,
            filler: ep.filler.unwrap_or(false),
        })
        .collect();

    Ok(items)
}
