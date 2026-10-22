//! IMDb's suggestion index, the search of last resort: titles, a poster and the year only.

use super::ids::{imdb_number, ExtIds};
use super::model::{self, positive_i32, text, Found};
use crate::clients::http::USER_AGENT;
use crate::error::AppError;
use crate::models::{CoverImage, Media, MediaTitle};
use reqwest::Client;
use serde_json::Value;

pub const LABEL: &str = "IMDb";

/// `query` as a suggestion path: IMDb wants letters and digits, the rest as `_`.
pub fn suggestion_key(query: &str) -> String {
    let clean: String = query.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    urlencoding::encode(clean.trim_matches('_')).into_owned()
}

/// Titles matching `query` (or the one title with that `tt…` id).
pub async fn search(client: &Client, base: &str, query: &str) -> Result<Vec<Found>, AppError> {
    let key = suggestion_key(query);
    if key.is_empty() {
        return Ok(Vec::new());
    }
    let request = client
        .get(format!("{}/{}.json", base.trim_end_matches('/'), key))
        .header("User-Agent", USER_AGENT);
    Ok(parse(&model::read_json(LABEL, request).await?))
}

pub fn parse(json: &Value) -> Vec<Found> {
    json["d"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let format = match s["qid"].as_str()? {
                "tvSeries" | "tvMiniSeries" => "TV",
                "movie" | "tvMovie" => "MOVIE",
                "tvSpecial" | "video" => "SPECIAL",
                _ => return None,
            };
            let imdb = s["id"].as_str().and_then(imdb_number)?;
            let title = text(&s["l"]);
            let poster = text(&s["i"]["imageUrl"]);
            Some(Found {
                media: Media {
                    imdb_id: text(&s["id"]),
                    title: MediaTitle {
                        romaji: title.clone(),
                        english: title.clone(),
                        native: None,
                        user_preferred: title,
                    },
                    cover_image: CoverImage { large: poster.clone(), extra_large: poster, color: None },
                    format: Some(format.to_string()),
                    season_year: positive_i32(&s["y"]),
                    ..Default::default()
                },
                ids: ExtIds { imdb: Some(imdb), ..Default::default() },
                ..Default::default()
            })
        })
        .collect()
}
