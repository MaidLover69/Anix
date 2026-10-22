//! MyAnimeList's official API (`api.myanimelist.net/v2`), read with the app's client id.

use super::ids::ExtIds;
use super::model::{self, genres, positive_i32, score_100_of_10, text, Found, SearchQuery};
use crate::error::AppError;
use crate::models::{CoverImage, Media, MediaTitle};
use reqwest::Client;
use serde_json::Value;

pub const LABEL: &str = "MyAnimeList";

pub const FIELDS: &str = "id,title,main_picture,alternative_titles,start_date,synopsis,mean,popularity,num_list_users,genres,media_type,status,num_episodes,start_season,broadcast,average_episode_duration,rating,studios";

async fn get(client: &Client, base: &str, client_id: &str, path: &str, query: &[(&str, String)]) -> Result<Value, AppError> {
    let request = client
        .get(format!("{}{}", base.trim_end_matches('/'), path))
        .header("X-MAL-CLIENT-ID", client_id)
        .query(query);
    model::read_json(LABEL, request).await
}

/// MAL's ranking list that matches an AniList list request without a search text, if any.
/// The status and format rankings are ordered by score, so they only serve that sort (and
/// "trending" for airing shows); "newest" has no ranking.
pub fn ranking_type(q: &SearchQuery) -> Option<&'static str> {
    let by_score = q.sort() == "SCORE_DESC";
    if q.status.is_some() || q.format.is_some() {
        let ranking = match (q.status.as_deref(), q.format.as_deref()) {
            (Some("RELEASING"), None) if by_score || q.sort() == "TRENDING_DESC" => "airing",
            (Some("NOT_YET_RELEASED"), None) if by_score => "upcoming",
            (None, Some("TV")) if by_score => "tv",
            (None, Some("MOVIE")) if by_score => "movie",
            (None, Some("OVA")) if by_score => "ova",
            (None, Some("ONA")) if by_score => "ona",
            (None, Some("SPECIAL")) if by_score => "special",
            _ => return None,
        };
        return Some(ranking);
    }
    Some(match q.sort() {
        "SCORE_DESC" => "all",
        "FAVOURITES_DESC" => "favorite",
        "TRENDING_DESC" => "airing",
        "START_DATE_DESC" => return None,
        _ => "bypopularity",
    })
}

/// The season before `season`, as MAL names them.
pub fn previous_season((year, season): (i64, &'static str)) -> (i64, &'static str) {
    match season {
        "winter" => (year - 1, "fall"),
        "spring" => (year, "winter"),
        "summer" => (year, "spring"),
        _ => (year, "summer"),
    }
}

/// The current anime season by the calendar, as MAL names it.
pub fn current_season(now: i64) -> (i64, &'static str) {
    let days = now.div_euclid(86_400);
    // Inverse of days_from_civil, enough for the month.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    let season = match month {
        1..=3 => "winter",
        4..=6 => "spring",
        7..=9 => "summer",
        _ => "fall",
    };
    (year, season)
}

pub async fn search(client: &Client, base: &str, client_id: &str, q: &SearchQuery) -> Result<Vec<Found>, AppError> {
    let limit = q.per_page.clamp(1, 100).to_string();
    let offset = q.offset().to_string();
    let mut params = vec![("fields", FIELDS.to_string()), ("limit", limit), ("offset", offset)];
    if !q.hide_adult {
        params.push(("nsfw", "true".to_string()));
    }
    let json = match q.text() {
        Some(text) => {
            params.push(("q", text.to_string()));
            get(client, base, client_id, "/anime", &params).await?
        }
        // A ranking cannot be narrowed to a genre, or ordered by start date (the seasonal
        // list mixes in shows from earlier years); another catalogue can.
        None if !q.genres.is_empty() => return Err(model::unsupported(LABEL, "list by genre")),
        None => {
            let ranking = ranking_type(q).ok_or_else(|| model::unsupported(LABEL, "list in this order with these filters"))?;
            params.push(("ranking_type", ranking.to_string()));
            get(client, base, client_id, "/anime/ranking", &params).await?
        }
    };
    model::kept(LABEL, q, parse_list(&json))
}

pub async fn details(client: &Client, base: &str, client_id: &str, mal_id: i64) -> Result<Found, AppError> {
    let fields = format!("{},related_anime,recommendations", FIELDS);
    let json = get(client, base, client_id, &format!("/anime/{}", mal_id), &[("fields", fields)]).await?;
    parse_details(&json).ok_or_else(|| AppError::Parse(format!("{}: anime {} not found", LABEL, mal_id)))
}

/// The seasonal list with broadcast times, for the schedule.
pub async fn season(client: &Client, base: &str, client_id: &str, year: i64, season: &str) -> Result<Value, AppError> {
    let params = [
        ("fields", FIELDS.to_string()),
        ("limit", "500".to_string()),
        ("sort", "anime_num_list_users".to_string()),
        ("nsfw", "true".to_string()),
    ];
    get(client, base, client_id, &format!("/anime/season/{}/{}", year, season), &params).await
}

/// `data[].node` of a list, search, ranking or seasonal answer.
pub fn parse_list(json: &Value) -> Vec<Found> {
    json["data"]
        .as_array()
        .map(|list| list.iter().filter_map(|item| parse_node(&item["node"])).collect())
        .unwrap_or_default()
}

pub fn parse_details(json: &Value) -> Option<Found> {
    let mut found = parse_node(json)?;
    found.related = json["related_anime"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|r| {
                    let mut rel = parse_node(&r["node"])?;
                    rel.relation = r["relation_type"].as_str().map(relation_type);
                    Some(rel)
                })
                .collect()
        })
        .unwrap_or_default();
    // Recommendations travel with the relations, marked as such (see `model::RECOMMENDATION`).
    let recommended = json["recommendations"].as_array().into_iter().flatten().filter_map(|r| {
        let mut rec = parse_node(&r["node"])?;
        rec.relation = Some(model::RECOMMENDATION.to_string());
        Some(rec)
    });
    found.related.extend(recommended);
    Some(found)
}

pub fn relation_type(mal: &str) -> String {
    match mal {
        "alternative_setting" | "alternative_version" => "ALTERNATIVE",
        "parent_story" | "full_story" => "PARENT",
        "spin_off" => "SPIN_OFF",
        "side_story" => "SIDE_STORY",
        "sequel" => "SEQUEL",
        "prequel" => "PREQUEL",
        "summary" => "SUMMARY",
        "character" => "CHARACTER",
        _ => "OTHER",
    }
    .to_string()
}

pub fn format(media_type: &str) -> Option<String> {
    let f = match media_type {
        "tv" => "TV",
        "movie" => "MOVIE",
        "ova" => "OVA",
        "ona" => "ONA",
        "special" | "tv_special" => "SPECIAL",
        "music" => "MUSIC",
        _ => return None,
    };
    Some(f.to_string())
}

pub fn parse_node(node: &Value) -> Option<Found> {
    let mal_id = node["id"].as_i64().filter(|id| *id > 0)?;
    let alt = &node["alternative_titles"];
    let title = text(&node["title"]);
    let mean = &node["mean"];
    let duration = node["average_episode_duration"].as_i64().filter(|s| *s > 0).map(|s| ((s + 30) / 60) as i32);
    let status = match node["status"].as_str() {
        Some("finished_airing") => Some("FINISHED"),
        Some("currently_airing") => Some("RELEASING"),
        Some("not_yet_aired") => Some("NOT_YET_RELEASED"),
        _ => None,
    };
    let media = Media {
        id_mal: i32::try_from(mal_id).ok(),
        title: MediaTitle {
            romaji: title.clone(),
            english: text(&alt["en"]),
            native: text(&alt["ja"]),
            user_preferred: title,
        },
        cover_image: CoverImage {
            large: text(&node["main_picture"]["medium"]),
            extra_large: text(&node["main_picture"]["large"]),
            color: None,
        },
        description: text(&node["synopsis"]),
        format: node["media_type"].as_str().and_then(format),
        season: node["start_season"]["season"].as_str().map(str::to_ascii_uppercase),
        season_year: positive_i32(&node["start_season"]["year"]),
        episodes: positive_i32(&node["num_episodes"]),
        duration,
        status: status.map(String::from),
        average_score: score_100_of_10(mean),
        mean_score: score_100_of_10(mean),
        score_mal: mean.as_f64(),
        popularity: positive_i32(&node["num_list_users"]),
        is_adult: node["rating"].as_str() == Some("rx"),
        genres: genres(node["genres"].as_array().into_iter().flatten().filter_map(|g| g["name"].as_str())),
        ..Default::default()
    };
    Some(Found {
        media,
        ids: ExtIds { mal: Some(mal_id), ..Default::default() },
        synonyms: alt["synonyms"]
            .as_array()
            .map(|a| a.iter().filter_map(text).collect())
            .unwrap_or_default(),
        studios: node["studios"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| text(&s["name"])).collect())
            .unwrap_or_default(),
        relation: None,
        related: Vec::new(),
    })
}
