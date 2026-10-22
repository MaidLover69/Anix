//! Kitsu's JSON:API (`kitsu.app/api/edge`). Its `mappings` carry AniList and MAL ids.

use super::ids::ExtIds;
use super::model::{self, genres, positive_i32, season_from_date, text, Found, SearchQuery};
use crate::error::AppError;
use crate::models::{CoverImage, Media, MediaTitle, Trailer};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;

pub const LABEL: &str = "Kitsu";
/// Kitsu's largest page.
const MAX_PAGE: i32 = 20;
/// Requests for one page of the app's (24 a page on Discover).
const MAX_REQUESTS: i32 = 3;
const INCLUDE: [(&str, &str); 3] = [
    ("include", "mappings,categories"),
    ("fields[mappings]", "externalSite,externalId"),
    ("fields[categories]", "title"),
];

async fn get(client: &Client, base: &str, path: &str, query: &[(&str, String)]) -> Result<Value, AppError> {
    let request = client
        .get(format!("{}{}", base.trim_end_matches('/'), path))
        .header("Accept", "application/vnd.api+json")
        .query(&INCLUDE)
        .query(query);
    model::read_json(LABEL, request).await
}

/// Kitsu's category slug for an AniList genre.
pub fn category_slug(genre: &str) -> String {
    match genre {
        "Sci-Fi" => "science-fiction".to_string(),
        "Mahou Shoujo" => "magical-girl".to_string(),
        other => other.to_ascii_lowercase().replace(' ', "-"),
    }
}

/// The `page[offset]` and `page[limit]` of each request that together give the app's page
/// `q.page` of `q.per_page`. Kitsu's filtered searches read the offset as the 1-based page
/// number `offset / limit`, so their offsets are one page further.
pub fn page_requests(q: &SearchQuery, filtered: bool) -> Vec<(i32, i32)> {
    let per_page = q.per_page.max(1);
    // A chunk size that divides the page keeps every offset a multiple of the limit.
    let limit = (1..=MAX_PAGE.min(per_page)).rev().find(|d| per_page % d == 0).unwrap_or(1);
    (0..(per_page / limit).min(MAX_REQUESTS))
        .map(|i| {
            let offset = q.offset() + i * limit;
            (if filtered { offset + limit } else { offset }, limit)
        })
        .collect()
}

pub async fn search(client: &Client, base: &str, q: &SearchQuery) -> Result<Vec<Found>, AppError> {
    let mut params = Vec::new();
    if let Some(text) = q.text() {
        params.push(("filter[text]", text.to_string()));
    }
    // Kitsu keeps the anime that have every listed category (checked 2026-10-22: "romance,mecha"
    // gives 134 anime, each with both), as AniList does.
    if !q.genres.is_empty() {
        let slugs: Vec<String> = q.genres.iter().map(|g| category_slug(g)).collect();
        params.push(("filter[categories]", slugs.join(",")));
    }
    if let Some(subtype) = q.format.as_deref().and_then(subtype_of) {
        params.push(("filter[subtype]", subtype.to_string()));
    }
    let status = q.status.as_deref().and_then(status_of).or(match q.sort() {
        "TRENDING_DESC" | "START_DATE_DESC" if q.text().is_none() => Some("current"),
        _ => None,
    });
    if let Some(status) = status {
        params.push(("filter[status]", status.to_string()));
    }
    // A text search is ordered by relevance.
    if q.text().is_none() {
        let sort = match q.sort() {
            "SCORE_DESC" => "ratingRank",
            "START_DATE_DESC" => "-startDate",
            "FAVOURITES_DESC" => "-favoritesCount",
            _ => "popularityRank",
        };
        params.push(("sort", sort.to_string()));
    }
    let filtered = params.iter().any(|(name, _)| name.starts_with("filter["));
    let mut found = Vec::new();
    for (offset, limit) in page_requests(q, filtered) {
        let mut page = params.clone();
        page.push(("page[limit]", limit.to_string()));
        page.push(("page[offset]", offset.to_string()));
        let json = get(client, base, "/anime", &page).await?;
        let full = json["data"].as_array().is_some_and(|list| list.len() == limit as usize);
        found.extend(parse_list(&json));
        if !full {
            break;
        }
    }
    model::kept(LABEL, q, found)
}

pub async fn details(client: &Client, base: &str, kitsu_id: i64) -> Result<Found, AppError> {
    let json = get(client, base, &format!("/anime/{}", kitsu_id), &[]).await?;
    parse_document(&json).ok_or_else(|| AppError::Parse(format!("{}: anime {} not found", LABEL, kitsu_id)))
}

/// A single-anime answer.
pub fn parse_document(json: &Value) -> Option<Found> {
    parse_resource(&json["data"], &index_included(json))
}

fn subtype_of(format: &str) -> Option<&'static str> {
    Some(match format {
        "TV" | "TV_SHORT" => "TV",
        "MOVIE" => "movie",
        "OVA" => "OVA",
        "ONA" => "ONA",
        "SPECIAL" => "special",
        "MUSIC" => "music",
        _ => return None,
    })
}

fn status_of(status: &str) -> Option<&'static str> {
    Some(match status {
        "RELEASING" => "current",
        "FINISHED" => "finished",
        "NOT_YET_RELEASED" => "upcoming",
        _ => return None,
    })
}

pub type Included<'a> = HashMap<(&'a str, &'a str), &'a Value>;

fn index_included(json: &Value) -> Included<'_> {
    json["included"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| Some(((item["type"].as_str()?, item["id"].as_str()?), item)))
        .collect()
}

pub fn parse_list(json: &Value) -> Vec<Found> {
    let included = index_included(json);
    json["data"]
        .as_array()
        .map(|list| list.iter().filter_map(|item| parse_resource(item, &included)).collect())
        .unwrap_or_default()
}

fn related<'a>(item: &'a Value, name: &str, included: &Included<'a>) -> Vec<&'a Value> {
    item["relationships"][name]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| included.get(&(r["type"].as_str()?, r["id"].as_str()?)).copied())
        .collect()
}

pub fn parse_resource(item: &Value, included: &Included<'_>) -> Option<Found> {
    let kitsu_id = item["id"].as_str()?.parse::<i64>().ok().filter(|id| *id > 0)?;
    let a = &item["attributes"];
    let titles = &a["titles"];

    let mut ids = ExtIds { kitsu: Some(kitsu_id), ..Default::default() };
    for mapping in related(item, "mappings", included) {
        let m = &mapping["attributes"];
        let id = m["externalId"].as_str().and_then(|s| s.trim().parse::<i64>().ok()).filter(|id| *id > 0);
        match m["externalSite"].as_str() {
            Some("anilist/anime") => ids.anilist = ids.anilist.or(id),
            Some("myanimelist/anime") => ids.mal = ids.mal.or(id),
            _ => {}
        }
    }
    let categories = related(item, "categories", included);
    let (season, season_year) = season_from_date(&a["startDate"]);
    let status = match a["status"].as_str() {
        Some("current") => Some("RELEASING"),
        Some("finished") => Some("FINISHED"),
        Some("upcoming") | Some("unreleased") | Some("tba") => Some("NOT_YET_RELEASED"),
        _ => None,
    };
    let format = match a["subtype"].as_str() {
        Some("TV") => Some("TV"),
        Some("movie") => Some("MOVIE"),
        Some("OVA") => Some("OVA"),
        Some("ONA") => Some("ONA"),
        Some("special") => Some("SPECIAL"),
        Some("music") => Some("MUSIC"),
        _ => None,
    };
    let score = a["averageRating"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|s| s.round() as i32);
    let media = Media {
        id_mal: ids.mal.and_then(|id| i32::try_from(id).ok()),
        title: MediaTitle {
            romaji: text(&titles["en_jp"]).or_else(|| text(&a["canonicalTitle"])),
            english: text(&titles["en"]).or_else(|| text(&titles["en_us"])),
            native: text(&titles["ja_jp"]),
            user_preferred: text(&a["canonicalTitle"]),
        },
        cover_image: CoverImage {
            large: text(&a["posterImage"]["medium"]).or_else(|| text(&a["posterImage"]["small"])),
            extra_large: text(&a["posterImage"]["large"]).or_else(|| text(&a["posterImage"]["original"])),
            color: None,
        },
        // Kitsu's "cover" is a wide header image, AniList's banner.
        banner_image: text(&a["coverImage"]["large"]).or_else(|| text(&a["coverImage"]["original"])),
        description: text(&a["synopsis"]).or_else(|| text(&a["description"])),
        format: format.map(String::from),
        season,
        season_year,
        episodes: positive_i32(&a["episodeCount"]),
        duration: positive_i32(&a["episodeLength"]),
        status: status.map(String::from),
        average_score: score,
        mean_score: score,
        popularity: positive_i32(&a["userCount"]),
        favourites: positive_i32(&a["favoritesCount"]),
        is_adult: a["nsfw"].as_bool().unwrap_or(false) || a["ageRating"].as_str() == Some("R18"),
        genres: genres(categories.iter().filter_map(|c| c["attributes"]["title"].as_str())),
        trailer: text(&a["youtubeVideoId"]).map(|id| Trailer {
            id: Some(id),
            site: Some("youtube".to_string()),
            thumbnail: None,
        }),
        ..Default::default()
    };
    let synonyms = a["abbreviatedTitles"]
        .as_array()
        .map(|list| list.iter().filter_map(text).collect())
        .unwrap_or_default();
    Some(Found { media, ids, synonyms, ..Default::default() })
}
