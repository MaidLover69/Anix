use crate::clients::http::http_client;
use crate::error::AppError;
use crate::models::SkipTimes;
use crate::providers::common::fmt_ep;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipInterval {
    pub start_time: f64,
    pub end_time: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipResult {
    pub skip_type: String,
    pub interval: SkipInterval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AniSkipResponse {
    pub found: bool,
    #[serde(default)]
    pub results: Option<Vec<SkipResult>>,
}

/// Opening and ending intervals from an AniSkip v2 answer.
pub fn skip_times_from_response(data: AniSkipResponse) -> SkipTimes {
    let mut times = SkipTimes::default();
    for res in data.results.unwrap_or_default() {
        match res.skip_type.as_str() {
            "op" => {
                times.intro_start = Some(res.interval.start_time);
                times.intro_end = Some(res.interval.end_time);
            }
            "ed" => {
                times.outro_start = Some(res.interval.start_time);
                times.outro_end = Some(res.interval.end_time);
            }
            _ => {}
        }
    }
    times
}

pub async fn fetch_skip_times(
    mal_id: i64,
    episode_number: f64,
    duration_s: f64,
) -> Result<SkipTimes, AppError> {
    if duration_s <= 60.0 {
        return Ok(SkipTimes::default());
    }

    let client = http_client()?;
    let url = format!(
        "https://api.aniskip.com/v2/skip-times/{}/{}?types[]=op&types[]=ed&episodeLength={}",
        mal_id,
        fmt_ep(episode_number),
        duration_s.round() as i64
    );

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(8))
        .send()
        .await?;
    let status = response.status();
    // AniSkip answers 404 (or 400 for an episode it cannot have) when it knows no skip times.
    if status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST {
        return Ok(SkipTimes::default());
    }
    if !status.is_success() {
        return Err(AppError::Network(format!("AniSkip HTTP {}", status.as_u16())));
    }

    let data: AniSkipResponse = response
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("AniSkip response: {}", e)))?;
    Ok(skip_times_from_response(data))
}
