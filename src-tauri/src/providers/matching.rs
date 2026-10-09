//! Deciding whether a search hit on a provider site is the anime that was asked for.

use crate::models::AnimeQuery;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::BTreeSet;

/// A hit is accepted only at or above this score.
pub const MIN_MATCH_SCORE: f64 = 0.8;

static ORDINAL_SEASON_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(\d+)(?:st|nd|rd|th) season\b").expect("ordinal season regex"));
static SEASON_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(?:season|s) ?(\d+)\b").expect("season regex"));
static PART_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(?:part|cour) ?(\d+)\b").expect("part regex"));

const NOISE_WORDS: [&str; 8] = ["tv", "sub", "dub", "subbed", "dubbed", "the", "uncensored", "bd"];

/// Lower-cases, drops punctuation and writes season markers one way ("2nd Season" and
/// "Season 2" both become "season 2").
pub fn normalize_title(title: &str) -> String {
    let lowered: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let collapsed = lowered.split_whitespace().collect::<Vec<_>>().join(" ");
    let seasons = ORDINAL_SEASON_RE.replace_all(&collapsed, "season $1");
    let seasons = SEASON_RE.replace_all(&seasons, "season $1");
    seasons
        .split_whitespace()
        .filter(|word| !NOISE_WORDS.contains(word))
        .collect::<Vec<_>>()
        .join(" ")
}

fn roman_value(word: &str) -> Option<u32> {
    match word {
        "ii" => Some(2),
        "iii" => Some(3),
        "iv" => Some(4),
        "v" => Some(5),
        "vi" => Some(6),
        _ => None,
    }
}

/// Season number a normalised title announces, if any: "season 2", a trailing "2" or "ii".
/// A title without a marker is treated as the first season.
pub fn season_number(normalized: &str) -> u32 {
    if let Some(cap) = SEASON_RE.captures(normalized) {
        if let Ok(n) = cap[1].parse::<u32>() {
            return n.max(1);
        }
    }
    let words: Vec<&str> = normalized.split_whitespace().collect();
    if words.len() >= 2 {
        if let Some(last) = words.last() {
            if let Some(n) = roman_value(last) {
                return n;
            }
            if let Ok(n) = last.parse::<u32>() {
                if (2..=9).contains(&n) {
                    return n;
                }
            }
        }
    }
    1
}

fn part_number(normalized: &str) -> u32 {
    PART_RE
        .captures(normalized)
        .and_then(|cap| cap[1].parse::<u32>().ok())
        .unwrap_or(1)
}

fn is_movie(normalized: &str) -> bool {
    normalized
        .split_whitespace()
        .any(|w| matches!(w, "movie" | "film" | "gekijouban"))
}

/// Similarity of two titles in 0.0..=1.0. Identical after normalisation is 1.0; otherwise the
/// shared-word ratio, cut down when the titles name different seasons, parts or a movie.
pub fn title_similarity(a: &str, b: &str) -> f64 {
    let (na, nb) = (normalize_title(a), normalize_title(b));
    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }
    if na == nb {
        return 1.0;
    }

    let wa: BTreeSet<&str> = na.split_whitespace().collect();
    let wb: BTreeSet<&str> = nb.split_whitespace().collect();
    let shared = wa.intersection(&wb).count() as f64;
    let mut score = 2.0 * shared / (wa.len() + wb.len()) as f64;

    if season_number(&na) != season_number(&nb) || part_number(&na) != part_number(&nb) {
        score *= 0.6;
    }
    if is_movie(&na) != is_movie(&nb) {
        score *= 0.6;
    }
    score.min(0.99)
}

/// Score of one candidate against the query: the best title similarity, adjusted by the year and
/// episode count when both sides know them.
pub fn match_score(
    query: &AnimeQuery,
    candidate_titles: &[String],
    candidate_year: Option<i32>,
    candidate_episodes: Option<i32>,
) -> f64 {
    let mut score: f64 = 0.0;
    for wanted in &query.titles {
        for title in candidate_titles {
            score = score.max(title_similarity(wanted, title));
        }
    }
    if score == 0.0 {
        return 0.0;
    }

    if let (Some(wanted), Some(found)) = (query.year, candidate_year) {
        if (wanted - found).abs() > 1 {
            score *= 0.7;
        }
    }
    if let (Some(wanted), Some(found)) = (query.episodes, candidate_episodes) {
        if wanted > 0 && found > 0 {
            let tolerance = ((wanted as f64) * 0.2).ceil().max(2.0);
            if ((wanted - found).abs() as f64) > tolerance {
                score *= 0.75;
            }
        }
    }
    score
}

/// One search hit with whatever the site says about it.
#[derive(Debug, Clone)]
pub struct Candidate<T> {
    pub item: T,
    pub titles: Vec<String>,
    pub year: Option<i32>,
    pub episodes: Option<i32>,
}

impl<T> Candidate<T> {
    pub fn new(item: T, titles: Vec<String>) -> Self {
        Self {
            item,
            titles,
            year: None,
            episodes: None,
        }
    }
}

/// The best-scoring candidate, or `None` when nothing plausibly matches. Earlier candidates win
/// ties, so the site's own ranking still counts between equals.
pub fn pick_best<T>(query: &AnimeQuery, candidates: Vec<Candidate<T>>) -> Option<T> {
    let mut best: Option<(f64, T)> = None;
    for candidate in candidates {
        let score = match_score(query, &candidate.titles, candidate.year, candidate.episodes);
        if score < MIN_MATCH_SCORE {
            continue;
        }
        if best.as_ref().map_or(true, |(top, _)| score > *top) {
            best = Some((score, candidate.item));
        }
    }
    best.map(|(_, item)| item)
}
