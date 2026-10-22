//! Shikimori's GraphQL API (`shikimori.io/api/graphql`). Its anime ids are MyAnimeList ids.
//! Descriptions are in Russian and are left out.

use super::ids::ExtIds;
use super::model::{self, genres, parse_datetime, positive_i32, score_100_of_10, season_of_month, text, Found, SearchQuery};
use crate::clients::http::APP_USER_AGENT;
use crate::error::AppError;
use crate::models::{
    Character, CharacterEdge, CharacterImage, CharacterName, CoverImage, Media, MediaTitle, NextAiringEpisode,
};
use reqwest::Client;
use serde_json::{json, Value};

pub const LABEL: &str = "Shikimori";
const MAX_PAGE: i32 = 50;
const MAX_CHARACTERS: usize = 18;

const FIELDS: &str = "id malId name english japanese synonyms kind rating score status episodes episodesAired duration airedOn { year month } nextEpisodeAt poster { originalUrl mainUrl } genres { name kind } studios { name }";
const DETAIL_FIELDS: &str = "related { relationKind anime { id malId name english kind status score poster { originalUrl mainUrl } } } characterRoles { rolesEn character { id name japanese poster { mainUrl } } }";

const QUERY_ARGS: &str = "$search: String, $limit: PositiveInt, $page: PositiveInt, $order: OrderEnum, $kind: AnimeKindString, $status: AnimeStatusString, $censored: Boolean, $ids: String";
const ANIMES_ARGS: &str = "search: $search, limit: $limit, page: $page, order: $order, kind: $kind, status: $status, censored: $censored, ids: $ids";

/// Runs `animes(...)` with `variables` and returns the list.
pub async fn animes(client: &Client, url: &str, variables: Value, details: bool) -> Result<Vec<Value>, AppError> {
    let fields = if details { format!("{} {}", FIELDS, DETAIL_FIELDS) } else { FIELDS.to_string() };
    let query = format!("query ({}) {{ animes({}) {{ {} }} }}", QUERY_ARGS, ANIMES_ARGS, fields);
    let request = client
        .post(url)
        .header("User-Agent", APP_USER_AGENT)
        .json(&json!({ "query": query, "variables": variables }));
    let json = model::read_json(LABEL, request).await?;
    if let Some(message) = json["errors"][0]["message"].as_str() {
        return Err(AppError::Parse(format!("{}: {}", LABEL, message)));
    }
    Ok(json["data"]["animes"].as_array().cloned().unwrap_or_default())
}

pub async fn search(client: &Client, url: &str, q: &SearchQuery) -> Result<Vec<Found>, AppError> {
    let mut vars = json!({
        "limit": q.per_page.clamp(1, MAX_PAGE),
        "page": q.page.max(1),
        "censored": q.hide_adult,
    });
    let order = match q.sort() {
        "SCORE_DESC" => "ranked",
        "START_DATE_DESC" => "aired_on",
        _ => "popularity",
    };
    if let Some(text) = q.text() {
        vars["search"] = json!(text);
    } else {
        vars["order"] = json!(order);
    }
    if let Some(kind) = q.format.as_deref().and_then(kind_of) {
        vars["kind"] = json!(kind);
    }
    let status = q.status.as_deref().and_then(status_of).or(match q.sort() {
        "TRENDING_DESC" if q.text().is_none() => Some("ongoing"),
        "START_DATE_DESC" if q.text().is_none() => Some("ongoing,released"),
        _ => None,
    });
    if let Some(status) = status {
        vars["status"] = json!(status);
    }
    let list = animes(client, url, vars, false).await?;
    model::kept(LABEL, q, list.iter().filter_map(parse_anime).collect())
}

pub async fn details(client: &Client, url: &str, mal_id: i64) -> Result<Found, AppError> {
    let list = animes(client, url, json!({ "ids": mal_id.to_string(), "limit": 1 }), true).await?;
    list.first()
        .and_then(parse_details)
        .ok_or_else(|| AppError::Parse(format!("{}: anime {} not found", LABEL, mal_id)))
}

fn kind_of(format: &str) -> Option<&'static str> {
    Some(match format {
        "TV" | "TV_SHORT" => "tv",
        "MOVIE" => "movie",
        "OVA" => "ova",
        "ONA" => "ona",
        "SPECIAL" => "special,tv_special",
        "MUSIC" => "music",
        _ => return None,
    })
}

fn status_of(status: &str) -> Option<&'static str> {
    Some(match status {
        "RELEASING" => "ongoing",
        "FINISHED" => "released",
        "NOT_YET_RELEASED" => "anons",
        _ => return None,
    })
}

pub fn parse_list(json: &Value) -> Vec<Found> {
    json["data"]["animes"]
        .as_array()
        .map(|list| list.iter().filter_map(parse_anime).collect())
        .unwrap_or_default()
}

pub fn parse_anime(a: &Value) -> Option<Found> {
    let mal_id = a["malId"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .or_else(|| a["id"].as_str().and_then(|s| s.parse().ok()))
        .filter(|id| *id > 0)?;
    let status = match a["status"].as_str() {
        Some("ongoing") => Some("RELEASING"),
        Some("released") => Some("FINISHED"),
        Some("anons") => Some("NOT_YET_RELEASED"),
        _ => None,
    };
    let format = match a["kind"].as_str() {
        Some("tv") => Some("TV"),
        Some("movie") => Some("MOVIE"),
        Some("ova") => Some("OVA"),
        Some("ona") => Some("ONA"),
        Some("special") | Some("tv_special") => Some("SPECIAL"),
        Some("music") | Some("pv") | Some("cm") => Some("MUSIC"),
        _ => None,
    };
    let aired = positive_i32(&a["episodesAired"]).unwrap_or(0);
    let next = a["nextEpisodeAt"].as_str().and_then(parse_datetime).map(|at| NextAiringEpisode {
        id: 0,
        airing_at: at,
        time_until_airing: at - crate::db::current_unix_timestamp(),
        episode: aired + 1,
    });
    let month = a["airedOn"]["month"].as_u64().map(|m| m as u32);
    let score = score_100_of_10(&a["score"]);
    let media = Media {
        id_mal: i32::try_from(mal_id).ok(),
        title: MediaTitle {
            romaji: text(&a["name"]),
            english: text(&a["english"]),
            native: text(&a["japanese"]),
            user_preferred: text(&a["name"]),
        },
        cover_image: CoverImage {
            large: text(&a["poster"]["mainUrl"]),
            extra_large: text(&a["poster"]["originalUrl"]).or_else(|| text(&a["poster"]["mainUrl"])),
            color: None,
        },
        format: format.map(String::from),
        season: month.and_then(season_of_month).map(String::from),
        // December belongs to the next year's winter season, as on AniList.
        season_year: positive_i32(&a["airedOn"]["year"]).map(|y| if month == Some(12) { y + 1 } else { y }),
        episodes: positive_i32(&a["episodes"]),
        duration: positive_i32(&a["duration"]),
        status: status.map(String::from),
        average_score: score,
        mean_score: score,
        is_adult: a["rating"].as_str() == Some("rx"),
        genres: genres(
            a["genres"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|g| g["kind"].as_str() != Some("demographic"))
                .filter_map(|g| g["name"].as_str()),
        ),
        next_airing_episode: next.filter(|_| status == Some("RELEASING")),
        ..Default::default()
    };
    Some(Found {
        media,
        ids: ExtIds { mal: Some(mal_id), ..Default::default() },
        // Shikimori's synonyms are mostly Russian; only Latin-script ones help matching.
        synonyms: a["synonyms"]
            .as_array()
            .map(|list| list.iter().filter_map(text).filter(|s| s.is_ascii()).collect())
            .unwrap_or_default(),
        studios: a["studios"]
            .as_array()
            .map(|list| list.iter().filter_map(|s| text(&s["name"])).collect())
            .unwrap_or_default(),
        relation: None,
        related: Vec::new(),
    })
}

pub fn parse_details(a: &Value) -> Option<Found> {
    let mut found = parse_anime(a)?;
    found.related = a["related"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let mut rel = parse_anime(&r["anime"])?;
            rel.relation = r["relationKind"].as_str().map(super::mal::relation_type);
            Some(rel)
        })
        .collect();
    let mut roles: Vec<(bool, &Value)> = a["characterRoles"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let main = r["rolesEn"].as_array().is_some_and(|roles| roles.iter().any(|x| x == "Main"));
            (main, &r["character"])
        })
        .collect();
    roles.sort_by_key(|(main, _)| !*main);
    found.media.characters = roles
        .into_iter()
        .filter_map(|(main, c)| {
            Some(CharacterEdge {
                role: Some(if main { "MAIN" } else { "SUPPORTING" }.to_string()),
                node: Some(Character {
                    id: positive_i32(&c["id"])?,
                    name: Some(CharacterName { full: text(&c["name"]), native: text(&c["japanese"]) }),
                    image: Some(CharacterImage { large: text(&c["poster"]["mainUrl"]), medium: text(&c["poster"]["mainUrl"]) }),
                }),
                voice_actors: Vec::new(),
            })
        })
        .take(MAX_CHARACTERS)
        .collect();
    Some(found)
}
