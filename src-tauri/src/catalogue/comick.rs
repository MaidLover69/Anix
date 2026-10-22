//! Comick's anime catalogue (`api.comick.dev/v1.0/search?media_type=anime`). Results carry
//! AniList and MAL ids; there is no details endpoint by id, and genres are numeric ids.

use super::ids::ExtIds;
use super::model::{self, positive_i32, score_100_of_10, text, Found, SearchQuery};
use crate::clients::http::USER_AGENT;
use crate::error::AppError;
use crate::models::{CoverImage, Media, MediaTitle};
use reqwest::Client;
use serde_json::Value;

pub const LABEL: &str = "Comick";
pub const COVER_HOST: &str = "https://meo.comick.pictures";
const MAX_PAGE: i32 = 50;

pub async fn search(client: &Client, base: &str, q: &SearchQuery) -> Result<Vec<Found>, AppError> {
    // Its results carry neither, so a filter would leave nothing.
    if !q.genres.is_empty() || q.format.is_some() {
        return Err(model::unsupported(LABEL, "filter by genre or format"));
    }
    let mut params = vec![
        ("media_type", "anime".to_string()),
        ("page", q.page.max(1).to_string()),
        ("limit", q.per_page.clamp(1, MAX_PAGE).to_string()),
    ];
    match q.text() {
        Some(text) => params.push(("q", text.to_string())),
        None => {
            let sort = match q.sort() {
                "SCORE_DESC" => "rating",
                "START_DATE_DESC" => "created_at",
                "TRENDING_DESC" => "view",
                _ => "follow",
            };
            params.push(("sort", sort.to_string()));
        }
    }
    let request = client
        .get(format!("{}/search", base.trim_end_matches('/')))
        .header("User-Agent", USER_AGENT)
        .query(&params);
    let json = model::read_json(LABEL, request).await?;
    model::kept(LABEL, q, parse_list(&json))
}

pub fn parse_list(json: &Value) -> Vec<Found> {
    json.as_array()
        .map(|list| list.iter().filter_map(parse_title).collect())
        .unwrap_or_default()
}

fn external(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|id| *id > 0)
}

pub fn parse_title(t: &Value) -> Option<Found> {
    if t["media_type"].as_str().is_some_and(|m| m != "anime") {
        return None;
    }
    let ids = ExtIds {
        anilist: external(&t["external_ids"]["anilist"]),
        mal: external(&t["external_ids"]["myanimelist"]),
        ..Default::default()
    };
    let titles = t["md_titles"].as_array().cloned().unwrap_or_default();
    let in_lang = |lang: &str| {
        let of_lang = titles.iter().filter(|x| x["lang"].as_str() == Some(lang));
        of_lang
            .clone()
            .find(|x| x["is_default"].as_bool() == Some(true))
            .or_else(|| of_lang.clone().next())
            .and_then(|x| text(&x["title"]))
    };
    let cover = t["md_covers"][0]["b2key"].as_str().map(|key| format!("{}/{}", COVER_HOST, key));
    let status = match t["status"].as_i64() {
        Some(1) => Some("RELEASING"),
        Some(2) => Some("FINISHED"),
        Some(3) => Some("CANCELLED"),
        Some(4) => Some("HIATUS"),
        _ => None,
    };
    let score = score_100_of_10(&t["rating"]);
    let media = Media {
        id_mal: ids.mal.and_then(|id| i32::try_from(id).ok()),
        title: MediaTitle {
            romaji: text(&t["title"]),
            english: in_lang("en"),
            native: in_lang("ja"),
            user_preferred: text(&t["title"]),
        },
        cover_image: CoverImage { large: cover.clone(), extra_large: cover, color: None },
        description: text(&t["desc"]),
        season_year: positive_i32(&t["year"]),
        episodes: positive_i32(&t["anime_profiles"]["episodes"]),
        status: status.map(String::from),
        average_score: score,
        mean_score: score,
        popularity: positive_i32(&t["user_follow_count"]),
        is_adult: matches!(t["content_rating"].as_str(), Some("erotica") | Some("pornographic")),
        ..Default::default()
    };
    let synonyms = titles.iter().filter_map(|x| text(&x["title"])).filter(|s| s.is_ascii()).take(8).collect();
    Some(Found { media, ids, synonyms, ..Default::default() })
}
