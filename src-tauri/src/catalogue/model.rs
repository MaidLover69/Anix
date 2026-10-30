//! What every catalogue's answers are turned into: the app's `Media` plus the ids it is known
//! by, and from there AniList's own JSON shape, which the details screen and the local-file
//! matcher read.

use super::ids::ExtIds;
use crate::error::AppError;
use crate::models::{CharacterEdge, Media};
use reqwest::header::RETRY_AFTER;
use reqwest::RequestBuilder;
use serde_json::{json, Value};

/// One anime as a fallback catalogue describes it. `media.id` is set once its AniList id
/// (or synthetic id) is known.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub media: Media,
    pub ids: ExtIds,
    pub synonyms: Vec<String>,
    pub studios: Vec<String>,
    /// AniList `MediaRelation` of a related entry (details only), or `RECOMMENDATION`.
    pub relation: Option<String>,
    pub related: Vec<Found>,
}

/// Marks a `related` entry that is a recommendation, not a relation. Kept with the relations so
/// its ids are resolved the same way; AniList has no relation of this name.
pub const RECOMMENDATION: &str = "RECOMMENDATION";

/// A search or list request in AniList's vocabulary (`search_anime`'s arguments).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchQuery {
    pub query: Option<String>,
    pub genres: Vec<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
    pub page: i32,
    pub per_page: i32,
    pub hide_adult: bool,
    /// Genres a result must not have.
    pub exclude_genres: Vec<String>,
    /// Lowest average score (0–100) a result may have; results without a score are left out.
    pub min_score: Option<i32>,
    /// Season year and AniList season (`WINTER`, `SPRING`, `SUMMER`, `FALL`).
    pub year: Option<i32>,
    pub season: Option<String>,
}

impl SearchQuery {
    /// A plain list, as the home screen asks for.
    pub fn list(sort: &str, per_page: i32, hide_adult: bool) -> Self {
        SearchQuery {
            sort: Some(sort.to_string()),
            page: 1,
            per_page,
            hide_adult,
            ..Default::default()
        }
    }

    pub fn text(&self) -> Option<&str> {
        self.query.as_deref().map(str::trim).filter(|q| !q.is_empty())
    }

    pub fn sort(&self) -> &str {
        self.sort.as_deref().unwrap_or("POPULARITY_DESC")
    }

    pub fn offset(&self) -> i32 {
        (self.page.max(1) - 1) * self.per_page.max(1)
    }

    /// Whether a result fits the filters a catalogue could not apply itself. AniList's
    /// `genre_in` wants every genre.
    pub fn keeps(&self, media: &Media) -> bool {
        let has = |g: &String| media.genres.iter().any(|m| m.eq_ignore_ascii_case(g));
        let genre_ok = self.genres.iter().all(has) && !self.exclude_genres.iter().any(has);
        let format_ok = self.format.as_deref().map_or(true, |f| media.format.as_deref() == Some(f));
        let status_ok = self.status.as_deref().map_or(true, |s| media.status.as_deref() == Some(s));
        let score_ok = self.min_score.map_or(true, |min| media.average_score.is_some_and(|s| s >= min));
        let year_ok = self.year.map_or(true, |y| media.season_year == Some(y));
        let season_ok = self.season.as_deref().map_or(true, |s| media.season.as_deref() == Some(s));
        genre_ok && format_ok && status_ok && score_ok && year_ok && season_ok && !(self.hide_adult && media.is_adult)
    }
}

const FILTERED_OUT: &str = "has nothing on this page that fits the filters";

/// The entries of a catalogue's page that fit `q`. A first page that had entries but keeps none
/// was not narrowed by the catalogue's server (it ignores that filter), so it is an error and the
/// next catalogue is asked; an empty page is a real "nothing found". A later page never falls
/// through: its list came from this catalogue, and another's page N would mix two lists.
pub fn kept(source: &str, q: &SearchQuery, found: Vec<Found>) -> Result<Vec<Found>, AppError> {
    let had_any = !found.is_empty();
    let kept: Vec<Found> = found.into_iter().filter(|f| q.keeps(&f.media)).collect();
    if had_any && kept.is_empty() && q.page <= 1 {
        return Err(AppError::Other(format!("{} {}", source, FILTERED_OUT)));
    }
    Ok(kept)
}

/// Whether `error` is [`kept`]'s "the server ignored the filter".
pub fn is_filtered_out(error: &AppError) -> bool {
    matches!(error, AppError::Other(message) if message.ends_with(FILTERED_OUT))
}

/// Whether `error` is a catalogue's answer that it has no such anime (a `Parse` error saying
/// "not found", as [`read_json`] and the AniList client raise it). The page shows "Anime not
/// found" for it, matching the same words, instead of a connection error.
pub fn is_not_found(error: &AppError) -> bool {
    matches!(error, AppError::Parse(message) if message.to_ascii_lowercase().contains("not found"))
}

/// Raised for a request a catalogue cannot answer; it never pauses the catalogue.
pub fn unsupported(source: &str, what: &str) -> AppError {
    AppError::Other(format!("{} cannot {}", source, what))
}

/// Whether `error` is [`unsupported`]: the catalogue was not asked, so it says nothing about
/// the connection. Catalogue code raises `Other` only for this and [`kept`]'s error.
pub fn is_unsupported(error: &AppError) -> bool {
    matches!(error, AppError::Other(_)) && !is_filtered_out(error)
}

/// GETs or POSTs `request` and returns its JSON. 404 and other rejections of the request are
/// `Parse` errors, which do not pause the catalogue; 429 is a rate limit; anything else
/// (403, 5xx) says the catalogue is unusable right now.
pub async fn read_json(label: &str, request: RequestBuilder) -> Result<Value, AppError> {
    let response = request.send().await?;
    let status = response.status().as_u16();
    let retry_after_s = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let body = response.text().await?;
    match status {
        200..=299 => serde_json::from_str(&body).map_err(|e| AppError::Parse(format!("{}: {}", label, e))),
        404 => Err(AppError::Parse(format!("{}: anime not found", label))),
        429 => Err(AppError::RateLimited { retry_after_s: retry_after_s.unwrap_or(60) }),
        400 | 422 => Err(AppError::Parse(format!("{} rejected the request (HTTP {})", label, status))),
        _ => Err(AppError::Network(format!("{} HTTP {}", label, status))),
    }
}

pub fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

pub fn positive_i32(value: &Value) -> Option<i32> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|n| *n > 0)
        .and_then(|n| i32::try_from(n).ok())
}

/// A 0–10 score (`8.84`, `"8.84"`) as AniList's 0–100.
pub fn score_100_of_10(value: &Value) -> Option<i32> {
    let score = value.as_f64().or_else(|| value.as_str().and_then(|s| s.parse().ok()))?;
    (score > 0.0).then(|| (score * 10.0).round() as i32)
}

const ANILIST_GENRES: [&str; 19] = [
    "Action", "Adventure", "Comedy", "Drama", "Ecchi", "Fantasy", "Hentai", "Horror", "Mahou Shoujo",
    "Mecha", "Music", "Mystery", "Psychological", "Romance", "Sci-Fi", "Slice of Life", "Sports",
    "Supernatural", "Thriller",
];

/// A catalogue's genre in AniList's spelling, or `None` for tags AniList has no genre for
/// (demographics, themes such as "Elf").
pub fn anilist_genre(name: &str) -> Option<&'static str> {
    let name = name.trim();
    let alias = match name.to_ascii_lowercase().as_str() {
        "science fiction" | "sci fi" => "Sci-Fi",
        "magical girl" | "magical girls" => "Mahou Shoujo",
        "slice-of-life" => "Slice of Life",
        _ => name,
    };
    ANILIST_GENRES.iter().copied().find(|g| g.eq_ignore_ascii_case(alias))
}

pub fn genres<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in names.into_iter().filter_map(anilist_genre) {
        if !out.iter().any(|known| known == g) {
            out.push(g.to_string());
        }
    }
    out
}

/// The AniList season a month falls in (AniList's: winter is December to February, so a
/// show starting late in September is a fall show).
pub fn season_of_month(month: u32) -> Option<&'static str> {
    match month {
        12 | 1 | 2 => Some("WINTER"),
        3..=5 => Some("SPRING"),
        6..=8 => Some("SUMMER"),
        9..=11 => Some("FALL"),
        _ => None,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `"2023-09-29"` (or `"2023-09"`, `"2023"`) as (year, month, day).
pub fn parse_date(value: &str) -> Option<(i64, Option<u32>, Option<u32>)> {
    let mut parts = value.trim().get(..10.min(value.trim().len()))?.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next().and_then(|m| m.parse().ok()).filter(|m| (1..=12).contains(m));
    let day = parts.next().and_then(|d| d.parse().ok()).filter(|d| (1..=31).contains(d));
    Some((year, month, day))
}

/// An ISO 8601 date-time with offset (`2026-10-25T17:15:00+03:00`, `…Z`, `….000Z`) as Unix
/// seconds.
pub fn parse_datetime(value: &str) -> Option<i64> {
    let value = value.trim();
    let (year, month, day) = parse_date(value)?;
    let rest = value.get(11..)?;
    let hour: i64 = rest.get(0..2)?.parse().ok()?;
    let minute: i64 = rest.get(3..5)?.parse().ok()?;
    let second: i64 = rest.get(6..8).and_then(|s| s.parse().ok()).unwrap_or(0);
    let zone = rest.get(8..).unwrap_or("").trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match zone {
        "" | "Z" => 0,
        z => {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let h: i64 = z.get(1..3)?.parse().ok()?;
            let m: i64 = z.get(4..6).or_else(|| z.get(3..5)).and_then(|m| m.parse().ok()).unwrap_or(0);
            sign * (h * 3600 + m * 60)
        }
    };
    let days = days_from_civil(year, month?, day?);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Season and year from a start date.
pub fn season_from_date(value: &Value) -> (Option<String>, Option<i32>) {
    match value.as_str().and_then(parse_date) {
        // December belongs to the next year's winter season.
        Some((year, month, _)) => (
            month.and_then(season_of_month).map(String::from),
            i32::try_from(if month == Some(12) { year + 1 } else { year }).ok(),
        ),
        None => (None, None),
    }
}

/// The details JSON in AniList's shape (`fetch_anime_details`); `found.media.id` must be set.
pub fn anilist_json(found: &Found) -> Value {
    let m = &found.media;
    let next = m.next_airing_episode.as_ref().map(|n| {
        json!({ "id": n.id, "airingAt": n.airing_at, "timeUntilAiring": n.time_until_airing, "episode": n.episode })
    });
    let trailer = m.trailer.as_ref().map(|t| json!({ "id": t.id, "site": t.site, "thumbnail": t.thumbnail }));
    let characters: Vec<Value> = m.characters.iter().map(character_json).collect();
    let is_recommendation = |r: &&Found| r.relation.as_deref() == Some(RECOMMENDATION);
    let recommendations: Vec<Value> = found
        .related
        .iter()
        .filter(|r| r.media.id != 0 && is_recommendation(r))
        .map(|r| json!({ "mediaRecommendation": media_core(&r.media) }))
        .collect();
    let relations: Vec<Value> = found
        .related
        .iter()
        .filter(|r| r.media.id != 0 && !is_recommendation(r))
        .map(|r| {
            json!({
                "relationType": r.relation.clone().unwrap_or_else(|| "OTHER".to_string()),
                "node": media_core(&r.media),
            })
        })
        .collect();
    let studios: Vec<Value> = found
        .studios
        .iter()
        .enumerate()
        .map(|(i, name)| json!({ "isMain": i == 0, "node": { "id": 0, "name": name } }))
        .collect();

    let mut value = media_core(m);
    let extra = json!({
        "bannerImage": m.banner_image,
        "description": m.description,
        "season": m.season,
        "seasonYear": m.season_year,
        "startDate": { "year": m.season_year },
        "episodes": m.episodes,
        "duration": m.duration,
        "meanScore": m.mean_score,
        "popularity": m.popularity,
        "favourites": m.favourites,
        "isAdult": m.is_adult,
        "genres": m.genres,
        "synonyms": found.synonyms,
        "nextAiringEpisode": next,
        "studios": { "edges": studios },
        "trailer": trailer,
        "characters": { "edges": characters },
        "relations": { "edges": relations },
        "recommendations": { "nodes": recommendations },
    });
    if let (Some(base), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
        base.extend(extra.clone());
    }
    value
}

fn media_core(m: &Media) -> Value {
    json!({
        "id": m.id,
        "idMal": m.id_mal,
        "title": {
            "romaji": m.title.romaji,
            "english": m.title.english,
            "native": m.title.native,
            "userPreferred": m.title.user_preferred,
        },
        "coverImage": {
            "large": m.cover_image.large,
            "extraLarge": m.cover_image.extra_large,
            "color": m.cover_image.color,
        },
        "format": m.format,
        "status": m.status,
        "averageScore": m.average_score,
        "seasonYear": m.season_year,
        "isAdult": m.is_adult,
    })
}

fn character_json(edge: &CharacterEdge) -> Value {
    let node = edge.node.as_ref().map(|c| {
        json!({
            "id": c.id,
            "name": c.name.as_ref().map(|n| json!({ "full": n.full, "native": n.native })),
            "image": c.image.as_ref().map(|i| json!({ "large": i.large, "medium": i.medium })),
        })
    });
    json!({ "role": edge.role, "node": node, "voiceActors": [] })
}
