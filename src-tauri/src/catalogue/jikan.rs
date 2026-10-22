//! Jikan (`api.jikan.moe/v4`), an unofficial MyAnimeList mirror; asked last because it is
//! often slow and allows three requests a second.

use super::ids::ExtIds;
use super::model::{self, genres, positive_i32, score_100_of_10, text, Found, SearchQuery};
use crate::clients::http::APP_USER_AGENT;
use crate::error::AppError;
use crate::models::{CoverImage, Media, MediaTitle, Trailer};
use reqwest::Client;
use serde_json::Value;

pub const LABEL: &str = "Jikan";
const MAX_PAGE: i32 = 25;

async fn get(client: &Client, base: &str, path: &str, query: &[(&str, String)]) -> Result<Value, AppError> {
    let request = client
        .get(format!("{}{}", base.trim_end_matches('/'), path))
        .header("User-Agent", APP_USER_AGENT)
        .query(query);
    model::read_json(LABEL, request).await
}

pub async fn search(client: &Client, base: &str, q: &SearchQuery) -> Result<Vec<Found>, AppError> {
    let mut params = vec![
        ("page", q.page.max(1).to_string()),
        ("limit", q.per_page.clamp(1, MAX_PAGE).to_string()),
    ];
    if q.hide_adult {
        params.push(("sfw", "true".to_string()));
    }
    if let Some(text) = q.text() {
        params.push(("q", text.to_string()));
    } else {
        let (order_by, sort) = match q.sort() {
            "SCORE_DESC" => ("score", "desc"),
            "START_DATE_DESC" => ("start_date", "desc"),
            "FAVOURITES_DESC" => ("favorites", "desc"),
            _ => ("members", "desc"),
        };
        params.push(("order_by", order_by.to_string()));
        params.push(("sort", sort.to_string()));
    }
    let status = match (q.status.as_deref(), q.sort()) {
        (Some("RELEASING"), _) => Some("airing"),
        (Some("FINISHED"), _) => Some("complete"),
        (Some("NOT_YET_RELEASED"), _) => Some("upcoming"),
        (None, "TRENDING_DESC" | "START_DATE_DESC") if q.text().is_none() => Some("airing"),
        _ => None,
    };
    if let Some(status) = status {
        params.push(("status", status.to_string()));
    }
    if let Some(kind) = q.format.as_deref().map(str::to_ascii_lowercase).filter(|f| f != "tv_short") {
        params.push(("type", kind));
    }
    let json = get(client, base, "/anime", &params).await?;
    model::kept(LABEL, q, parse_list(&json))
}

pub async fn details(client: &Client, base: &str, mal_id: i64) -> Result<Found, AppError> {
    let json = get(client, base, &format!("/anime/{}", mal_id), &[]).await?;
    parse_anime(&json["data"]).ok_or_else(|| AppError::Parse(format!("{}: anime {} not found", LABEL, mal_id)))
}

pub fn parse_list(json: &Value) -> Vec<Found> {
    json["data"]
        .as_array()
        .map(|list| list.iter().filter_map(parse_anime).collect())
        .unwrap_or_default()
}

/// `"24 min per ep"`, `"1 hr 50 min"` as minutes.
pub fn duration_minutes(value: &str) -> Option<i32> {
    let mut minutes = 0;
    let words: Vec<&str> = value.split_whitespace().collect();
    for pair in words.windows(2) {
        if let Ok(n) = pair[0].parse::<i32>() {
            match pair[1] {
                "hr" | "hrs" | "hour" | "hours" => minutes += n * 60,
                "min" | "mins" => minutes += n,
                _ => {}
            }
        }
    }
    (minutes > 0).then_some(minutes)
}

/// The video id of `https://www.youtube-nocookie.com/embed/<id>?…`.
fn youtube_id_of_embed(value: &Value) -> Option<String> {
    let rest = value.as_str()?.split("/embed/").nth(1)?;
    let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    (!id.is_empty()).then_some(id)
}

pub fn parse_anime(a: &Value) -> Option<Found> {
    let mal_id = a["mal_id"].as_i64().filter(|id| *id > 0)?;
    let names = |key: &str| -> Vec<&str> {
        a[key].as_array().into_iter().flatten().filter_map(|g| g["name"].as_str()).collect()
    };
    let status = match a["status"].as_str() {
        Some("Finished Airing") => Some("FINISHED"),
        Some("Currently Airing") => Some("RELEASING"),
        Some("Not yet aired") => Some("NOT_YET_RELEASED"),
        _ => None,
    };
    let format = a["type"].as_str().and_then(|t| super::mal::format(&t.to_ascii_lowercase().replace(' ', "_")));
    let score = score_100_of_10(&a["score"]);
    let images = &a["images"]["jpg"];
    let media = Media {
        id_mal: i32::try_from(mal_id).ok(),
        title: MediaTitle {
            romaji: text(&a["title"]),
            english: text(&a["title_english"]),
            native: text(&a["title_japanese"]),
            user_preferred: text(&a["title"]),
        },
        cover_image: CoverImage {
            large: text(&images["image_url"]),
            extra_large: text(&images["large_image_url"]).or_else(|| text(&images["image_url"])),
            color: None,
        },
        description: text(&a["synopsis"]),
        format,
        season: a["season"].as_str().map(str::to_ascii_uppercase),
        season_year: positive_i32(&a["year"]),
        episodes: positive_i32(&a["episodes"]),
        duration: a["duration"].as_str().and_then(duration_minutes),
        status: status.map(String::from),
        average_score: score,
        mean_score: score,
        score_mal: a["score"].as_f64(),
        popularity: positive_i32(&a["members"]),
        favourites: positive_i32(&a["favorites"]),
        is_adult: a["rating"].as_str().is_some_and(|r| r.starts_with("Rx")),
        genres: genres(names("genres").into_iter().chain(names("themes"))),
        // Real answers (2026) leave `youtube_id` empty and name the video only in `embed_url`.
        trailer: text(&a["trailer"]["youtube_id"]).or_else(|| youtube_id_of_embed(&a["trailer"]["embed_url"])).map(|id| Trailer {
            id: Some(id),
            site: Some("youtube".to_string()),
            thumbnail: None,
        }),
        ..Default::default()
    };
    Some(Found {
        media,
        ids: ExtIds { mal: Some(mal_id), ..Default::default() },
        synonyms: a["title_synonyms"]
            .as_array()
            .map(|list| list.iter().filter_map(text).collect())
            .unwrap_or_default(),
        studios: names("studios").into_iter().map(String::from).collect(),
        relation: None,
        related: Vec::new(),
    })
}
