//! Library › Local files: scanned video files grouped into series, each matched to an AniList
//! title. Matches are scored here and cached in `local_library_matches`, so a folder is only
//! searched for once; a match the user confirmed or picked is kept until they change it.

use crate::commands::local_scanner::LocalAnimeFile;
use crate::db::current_unix_millis;
use crate::error::AppError;
use crate::plugins::scanner_utils::{
    extract_part_number, extract_season_number, normalize_title, weighted_token_match_ratio,
};
use crate::release_parser::{parse_relative_path, ParsedRelease, ReleaseKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A match is offered without asking from this score on.
pub const AUTO_MATCH_SCORE: f64 = 0.6;
/// An unconfirmed match is searched for again after this long.
pub const AUTO_MATCH_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// A series nothing was found for is searched for again after this long.
pub const NO_MATCH_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Results asked for per search.
pub const CANDIDATES_PER_SEARCH: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryFile {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub parsed: ParsedRelease,
    /// The title this file is linked to under "Your files", if any.
    pub linked_anilist_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalSeries {
    /// Stable id of the series: normalized title, season, part and kind.
    pub key: String,
    pub title: String,
    pub season: Option<u32>,
    pub part: Option<u32>,
    pub year: Option<u32>,
    pub kind: ReleaseKind,
    /// Episode files first (by number), then extras.
    pub files: Vec<LibraryFile>,
    /// Distinct episode numbers, ascending.
    pub episodes: Vec<f64>,
    pub extras: usize,
    pub release_groups: Vec<String>,
    pub resolutions: Vec<String>,
    pub total_size: u64,
    /// The deepest folder holding every file of the series.
    pub folder: Option<String>,
    /// `folder` holds no video of another series, so it can be linked as a whole.
    pub folder_exclusive: bool,
    /// Titles some of these files are already linked to.
    pub linked_anilist_ids: Vec<i32>,
    /// False for files whose name gave no title.
    pub matchable: bool,
    pub series_match: Option<SeriesMatch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalLibraryScan {
    pub root: String,
    pub file_count: usize,
    pub series: Vec<LocalSeries>,
}

/// What a search for one series is scored against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchRequest {
    pub key: String,
    pub title: String,
    pub season: Option<u32>,
    pub part: Option<u32>,
    pub year: Option<u32>,
    pub kind: ReleaseKind,
    pub max_episode: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CandidateTitle {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
    pub user_preferred: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MatchCandidate {
    pub id: i32,
    pub id_mal: Option<i32>,
    pub title: CandidateTitle,
    pub synonyms: Vec<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub episodes: Option<i32>,
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub start_year: Option<i32>,
    pub cover_image: Option<String>,
    pub color: Option<String>,
    pub banner_image: Option<String>,
    pub average_score: Option<i32>,
    pub genres: Vec<String>,
    pub is_adult: bool,
    /// How well the candidate fits the series, 0 to 1.
    pub score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchStatus {
    /// Found by the search and good enough to offer; not confirmed yet.
    Auto,
    /// Confirmed or picked by the user.
    Confirmed,
    /// Searched, nothing fit well enough.
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesMatch {
    pub key: String,
    pub status: MatchStatus,
    pub media: Option<MatchCandidate>,
    /// The search results, best first.
    pub candidates: Vec<MatchCandidate>,
    pub updated_at: i64,
}

impl SeriesMatch {
    /// True while the cached answer is still trusted.
    pub fn is_fresh(&self, now: i64) -> bool {
        match self.status {
            MatchStatus::Confirmed => true,
            MatchStatus::Auto => now - self.updated_at < AUTO_MATCH_TTL_MS,
            MatchStatus::None => now - self.updated_at < NO_MATCH_TTL_MS,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Grouping

fn key_text(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn kind_key(kind: ReleaseKind) -> &'static str {
    match kind {
        ReleaseKind::Episode | ReleaseKind::Extra => "tv",
        ReleaseKind::Movie => "movie",
        ReleaseKind::Ova => "ova",
        ReleaseKind::Ona => "ona",
        ReleaseKind::Special => "special",
    }
}

/// The series a parsed file belongs to. Extras join the regular episodes of their title, and
/// every movie is its own series. Empty for a file with no title.
pub fn series_key(parsed: &ParsedRelease) -> String {
    let title = key_text(&parsed.title);
    if title.is_empty() {
        return String::new();
    }
    let mut key = format!(
        "{}|s{}|p{}|{}",
        title,
        parsed.season.unwrap_or(1),
        parsed.part.unwrap_or(1),
        kind_key(parsed.kind)
    );
    if parsed.kind == ReleaseKind::Movie {
        if let Some(n) = parsed.episode {
            key.push_str(&format!("|{}", n));
        }
    }
    key
}

fn most_common<T: Clone + Ord>(values: impl IntoIterator<Item = T>) -> Option<T> {
    let mut counts: BTreeMap<T, usize> = BTreeMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    let best = counts.values().copied().max()?;
    counts.into_iter().find(|(_, c)| *c == best).map(|(v, _)| v)
}

fn distinct_sorted(values: impl IntoIterator<Item = f64>) -> Vec<f64> {
    let mut list: Vec<f64> = values.into_iter().filter(|n| n.is_finite()).collect();
    list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    list.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    list
}

fn common_dir(paths: &[&Path]) -> Option<PathBuf> {
    let mut common: PathBuf = paths.first()?.parent()?.to_path_buf();
    for path in &paths[1..] {
        let parent = path.parent()?;
        while !parent.starts_with(&common) {
            if !common.pop() {
                return None;
            }
        }
    }
    Some(common)
}

/// Parses every file by its path below `root` (folder names fill in what a file name lacks)
/// and groups the files into series, sorted by title.
pub fn build_library(root: &Path, files: Vec<LocalAnimeFile>, linked: &HashMap<String, i32>) -> Vec<LocalSeries> {
    let library_files: Vec<LibraryFile> = files
        .into_iter()
        .map(|file| {
            let path = Path::new(&file.file_path);
            let relative = path
                .strip_prefix(root)
                .unwrap_or_else(|_| Path::new(&file.file_name))
                .to_string_lossy()
                .to_string();
            LibraryFile {
                linked_anilist_id: linked.get(&file.file_path).copied(),
                parsed: parse_relative_path(&relative),
                file_path: file.file_path,
                file_name: file.file_name,
                file_size: file.file_size,
            }
        })
        .collect();
    group_series(library_files)
}

/// Groups parsed files into series.
pub fn group_series(files: Vec<LibraryFile>) -> Vec<LocalSeries> {
    let mut groups: BTreeMap<String, Vec<LibraryFile>> = BTreeMap::new();
    for file in files {
        groups.entry(series_key(&file.parsed)).or_default().push(file);
    }

    let all_paths: Vec<(String, PathBuf)> = groups
        .iter()
        .flat_map(|(key, files)| files.iter().map(move |f| (key.clone(), PathBuf::from(&f.file_path))))
        .collect();

    let mut series: Vec<LocalSeries> = groups
        .into_iter()
        .map(|(key, mut files)| {
            files.sort_by(|a, b| {
                let extra = |f: &LibraryFile| f.parsed.kind == ReleaseKind::Extra;
                extra(a)
                    .cmp(&extra(b))
                    .then(
                        a.parsed
                            .episode
                            .unwrap_or(f64::MAX)
                            .partial_cmp(&b.parsed.episode.unwrap_or(f64::MAX))
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
                    .then(a.file_name.cmp(&b.file_name))
            });
            let main: Vec<&LibraryFile> = files.iter().filter(|f| f.parsed.kind != ReleaseKind::Extra).collect();
            let described = if main.is_empty() { files.iter().collect() } else { main.clone() };

            let title = most_common(described.iter().map(|f| f.parsed.title.clone())).unwrap_or_default();
            let kind = most_common(described.iter().map(|f| kind_key(f.parsed.kind)))
                .map(|k| match k {
                    "movie" => ReleaseKind::Movie,
                    "ova" => ReleaseKind::Ova,
                    "ona" => ReleaseKind::Ona,
                    "special" => ReleaseKind::Special,
                    _ => ReleaseKind::Episode,
                })
                .unwrap_or(ReleaseKind::Episode);
            let season = most_common(described.iter().filter_map(|f| f.parsed.season));
            let part = most_common(described.iter().filter_map(|f| f.parsed.part));
            let year = most_common(described.iter().filter_map(|f| f.parsed.year));
            let episodes = distinct_sorted(main.iter().filter_map(|f| f.parsed.episode).chain(
                main.iter().filter_map(|f| f.parsed.episode_end),
            ));
            let mut release_groups: Vec<String> = Vec::new();
            for g in files.iter().filter_map(|f| f.parsed.release_group.clone()) {
                if !release_groups.contains(&g) {
                    release_groups.push(g);
                }
            }
            let mut resolutions: Vec<String> = Vec::new();
            for r in files.iter().filter_map(|f| f.parsed.resolution.clone()) {
                if !resolutions.contains(&r) {
                    resolutions.push(r);
                }
            }
            let mut linked_anilist_ids: Vec<i32> = Vec::new();
            for id in files.iter().filter_map(|f| f.linked_anilist_id) {
                if !linked_anilist_ids.contains(&id) {
                    linked_anilist_ids.push(id);
                }
            }

            let paths: Vec<&Path> = files.iter().map(|f| Path::new(&f.file_path)).collect();
            let folder = common_dir(&paths);
            let folder_exclusive = folder.as_ref().is_some_and(|dir| {
                !all_paths.iter().any(|(other, path)| *other != key && path.starts_with(dir))
            });

            LocalSeries {
                matchable: !key.is_empty(),
                title: if key.is_empty() { String::new() } else { title },
                season,
                part,
                year,
                kind,
                extras: files.len() - main.len(),
                episodes,
                release_groups,
                resolutions,
                total_size: files.iter().map(|f| f.file_size).sum(),
                folder: folder.map(|d| d.to_string_lossy().to_string()),
                folder_exclusive,
                linked_anilist_ids,
                series_match: None,
                key,
                files,
            }
        })
        .collect();

    series.sort_by(|a, b| {
        (!a.matchable)
            .cmp(&!b.matchable)
            .then(a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            .then(a.key.cmp(&b.key))
    });
    series
}

impl LocalSeries {
    pub fn match_request(&self) -> MatchRequest {
        MatchRequest {
            key: self.key.clone(),
            title: self.title.clone(),
            season: self.season,
            part: self.part,
            year: self.year,
            kind: self.kind,
            max_episode: self.episodes.last().copied(),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Scoring

fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Sørensen–Dice coefficient of the character bigrams of two strings (spaces ignored).
pub fn bigram_similarity(a: &str, b: &str) -> f64 {
    let bigrams = |s: &str| -> Vec<(char, char)> {
        let chars: Vec<char> = compact(s).chars().collect();
        chars.windows(2).map(|w| (w[0], w[1])).collect()
    };
    let (a, b) = (bigrams(a), bigrams(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut pool = b.clone();
    let mut shared = 0usize;
    for pair in &a {
        if let Some(pos) = pool.iter().position(|p| p == pair) {
            pool.swap_remove(pos);
            shared += 1;
        }
    }
    (2 * shared) as f64 / (a.len() + b.len()) as f64
}

/// How alike a file title and one AniList title are, 0 to 1. Season and part words are left
/// out on both sides; they are scored separately.
pub fn title_similarity(file_title: &str, candidate: &str) -> f64 {
    let q = normalize_title(file_title);
    let c = normalize_title(candidate);
    if q.normalized.is_empty() || c.normalized.is_empty() {
        return 0.0;
    }
    if q.normalized == c.normalized {
        return 1.0;
    }
    if compact(&q.normalized) == compact(&c.normalized) {
        return 0.98;
    }
    let forward = weighted_token_match_ratio(&q.tokens, &c.tokens);
    let backward = weighted_token_match_ratio(&c.tokens, &q.tokens);
    let dice = bigram_similarity(&q.normalized, &c.normalized);
    0.45 * forward + 0.25 * backward + 0.30 * dice
}

impl MatchCandidate {
    /// Every title the candidate is known by.
    pub fn all_titles(&self) -> Vec<&str> {
        let mut titles: Vec<&str> = [
            &self.title.romaji,
            &self.title.english,
            &self.title.user_preferred,
            &self.title.native,
        ]
        .into_iter()
        .filter_map(|t| t.as_deref())
        .collect();
        titles.extend(self.synonyms.iter().map(String::as_str));
        titles.retain(|t| !t.trim().is_empty());
        titles
    }

    /// The season the romaji or English title names ("2nd Season", "II", "S3"), also in the
    /// part before a colon ("Title 2: Subtitle"); `None` when neither does.
    pub fn titled_season(&self) -> Option<u32> {
        [&self.title.romaji, &self.title.english]
            .into_iter()
            .filter_map(|t| t.as_deref())
            .flat_map(|t| [t, t.split(':').next().unwrap_or(t)])
            .map(extract_season_number)
            .find(|s| *s > 0)
            .map(|s| s as u32)
    }

    /// The season the candidate's titles or synonyms name; 1 when none does.
    pub fn season_number(&self) -> u32 {
        self.titled_season()
            .or_else(|| {
                self.synonyms
                    .iter()
                    .map(|t| extract_season_number(t))
                    .find(|s| *s > 0)
                    .map(|s| s as u32)
            })
            .unwrap_or(1)
    }

    pub fn part_number(&self) -> u32 {
        [&self.title.romaji, &self.title.english]
            .into_iter()
            .filter_map(|t| t.as_deref())
            .map(extract_part_number)
            .find(|p| *p > 0)
            .map(|p| p as u32)
            .unwrap_or(1)
    }

    pub fn display_title(&self) -> String {
        self.title
            .user_preferred
            .clone()
            .or_else(|| self.title.romaji.clone())
            .or_else(|| self.title.english.clone())
            .unwrap_or_default()
    }
}

/// The best title similarity between the file title and any of the candidate's titles. The
/// part before a colon or " - " counts too, slightly less than a full title.
pub fn best_title_similarity(file_title: &str, candidate: &MatchCandidate) -> f64 {
    let mut best: f64 = 0.0;
    for title in candidate.all_titles() {
        best = best.max(title_similarity(file_title, title));
        let head = title.split([':', '：']).next().unwrap_or(title);
        let head = head.split(" - ").next().unwrap_or(head).trim();
        if head.len() >= 4 && head.len() < title.len() {
            best = best.max(0.97 * title_similarity(file_title, head));
        }
    }
    best
}

/// The season the files name, if they name one.
fn file_season(request: &MatchRequest) -> Option<u32> {
    request.season.or_else(|| {
        let s = extract_season_number(&request.title);
        (s > 0).then_some(s as u32)
    })
}

fn file_part(request: &MatchRequest) -> u32 {
    request
        .part
        .or_else(|| {
            let p = extract_part_number(&request.title);
            (p > 0).then_some(p as u32)
        })
        .unwrap_or(1)
}

/// How well `candidate` (at `rank` in AniList's results) fits the series, 0 to 1: title
/// similarity (up to 0.8), adjusted for season, part, format, episode count and year.
pub fn score_candidate(request: &MatchRequest, candidate: &MatchCandidate, rank: usize) -> f64 {
    let similarity = best_title_similarity(&request.title, candidate);
    if similarity <= 0.0 {
        return 0.0;
    }
    let mut score = 0.8 * similarity;

    // Files that name no season ("Kaguya-sama wa Kokurasetai - Ultra Romantic") are only held
    // against a candidate whose own title names a later season, not one whose synonyms do.
    score += match file_season(request) {
        Some(season) if season == candidate.season_number() => 0.08,
        Some(_) => -0.25,
        None => match candidate.titled_season() {
            Some(season) if season > 1 => -0.15,
            _ => 0.08,
        },
    };
    let (fp, cp) = (file_part(request), candidate.part_number());
    if fp == cp {
        if fp > 1 {
            score += 0.04;
        }
    } else {
        score -= 0.12;
    }

    let format = candidate.format.as_deref().unwrap_or("");
    score += match (request.kind, format) {
        (_, "MUSIC") => -0.3,
        (ReleaseKind::Episode | ReleaseKind::Extra, "TV" | "TV_SHORT" | "ONA") => 0.04,
        (ReleaseKind::Episode | ReleaseKind::Extra, "MOVIE") => -0.15,
        (ReleaseKind::Episode | ReleaseKind::Extra, "OVA" | "SPECIAL") => -0.06,
        (ReleaseKind::Movie, "MOVIE") => 0.12,
        (ReleaseKind::Movie, _) => -0.1,
        (ReleaseKind::Ova | ReleaseKind::Special, "OVA" | "SPECIAL") => 0.12,
        (ReleaseKind::Ova | ReleaseKind::Special, "TV") => -0.05,
        (ReleaseKind::Ona, "ONA") => 0.1,
        _ => 0.0,
    };

    if let (Some(max), Some(count)) = (request.max_episode, candidate.episodes) {
        if count > 0 {
            if max > count as f64 + 0.5 {
                if candidate.status.as_deref() == Some("FINISHED") {
                    score -= 0.15;
                }
            } else {
                score += 0.03;
            }
        }
    }

    if let Some(year) = request.year {
        if let Some(cy) = candidate.season_year.or(candidate.start_year) {
            let diff = (year as i32 - cy).abs();
            score += match diff {
                0 => 0.08,
                1 => 0.03,
                _ => -0.12,
            };
        }
    }

    score += (CANDIDATES_PER_SEARCH.saturating_sub(rank)) as f64 * 0.005;
    score.clamp(0.0, 1.0)
}

/// Scores and sorts candidates (best first), dropping repeats of the same id.
pub fn rank_candidates(request: &MatchRequest, candidates: Vec<MatchCandidate>) -> Vec<MatchCandidate> {
    let mut seen = HashSet::new();
    let mut scored: Vec<MatchCandidate> = candidates
        .into_iter()
        .enumerate()
        .filter(|(_, c)| seen.insert(c.id))
        .map(|(rank, mut c)| {
            c.score = (score_candidate(request, &c, rank) * 1000.0).round() / 1000.0;
            c
        })
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// The match a search gives: the best candidate when it scores at least
/// [`AUTO_MATCH_SCORE`], otherwise none (the candidates are kept to pick from).
pub fn match_from_candidates(request: &MatchRequest, candidates: Vec<MatchCandidate>, now: i64) -> SeriesMatch {
    let ranked = rank_candidates(request, candidates);
    let best = ranked.first().filter(|c| c.score >= AUTO_MATCH_SCORE).cloned();
    SeriesMatch {
        key: request.key.clone(),
        status: if best.is_some() { MatchStatus::Auto } else { MatchStatus::None },
        media: best,
        candidates: ranked,
        updated_at: now,
    }
}

// ---------------------------------------------------------------------------------------
// AniList search

pub const MEDIA_FIELDS: &str = r#"
id
idMal
title { romaji english native userPreferred }
synonyms
format
status
episodes
season
seasonYear
startDate { year }
coverImage { large extraLarge color }
bannerImage
averageScore
genres
isAdult
"#;

/// The searches made for a series: its title, and the title with the season or type when
/// there is one, since AniList's search ranks "Title Season 2" better that way.
pub fn search_terms(request: &MatchRequest) -> Vec<String> {
    let title = request.title.trim().to_string();
    if title.is_empty() {
        return Vec::new();
    }
    let mut terms = vec![title.clone()];
    let season = request.season.unwrap_or(1);
    if season > 1 {
        terms.push(format!("{} Season {}", title, season));
    } else {
        match request.kind {
            ReleaseKind::Movie => terms.push(format!("{} Movie", title)),
            ReleaseKind::Ova => terms.push(format!("{} OVA", title)),
            ReleaseKind::Special => terms.push(format!("{} Special", title)),
            _ => {}
        }
    }
    terms
}

/// One GraphQL request running every search in `terms` (aliases `s0`, `s1`, ...).
pub fn batched_search_body(terms: &[String], hide_adult: bool) -> Value {
    let mut params = Vec::new();
    let mut fields = Vec::new();
    let mut variables = serde_json::Map::new();
    let adult = if hide_adult { ", isAdult: false" } else { "" };
    for (i, term) in terms.iter().enumerate() {
        params.push(format!("$q{}: String", i));
        fields.push(format!(
            "s{i}: Page(perPage: {per}) {{ media(search: $q{i}, type: ANIME, sort: SEARCH_MATCH{adult}) {{ ...m }} }}",
            i = i,
            per = CANDIDATES_PER_SEARCH,
            adult = adult
        ));
        variables.insert(format!("q{}", i), json!(term));
    }
    let query = format!(
        "query ({}) {{ {} }} fragment m on Media {{ {} }}",
        params.join(", "),
        fields.join(" "),
        MEDIA_FIELDS
    );
    json!({ "query": query, "variables": variables })
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string).filter(|s| !s.is_empty())
}

pub fn parse_candidate(media: &Value) -> Option<MatchCandidate> {
    let id = media["id"].as_i64()? as i32;
    let cover = &media["coverImage"];
    Some(MatchCandidate {
        id,
        id_mal: media["idMal"].as_i64().map(|v| v as i32),
        title: CandidateTitle {
            romaji: opt_str(&media["title"]["romaji"]),
            english: opt_str(&media["title"]["english"]),
            native: opt_str(&media["title"]["native"]),
            user_preferred: opt_str(&media["title"]["userPreferred"]),
        },
        synonyms: media["synonyms"]
            .as_array()
            .map(|a| a.iter().filter_map(opt_str).collect())
            .unwrap_or_default(),
        format: opt_str(&media["format"]),
        status: opt_str(&media["status"]),
        episodes: media["episodes"].as_i64().map(|v| v as i32),
        season: opt_str(&media["season"]),
        season_year: media["seasonYear"].as_i64().map(|v| v as i32),
        start_year: media["startDate"]["year"].as_i64().map(|v| v as i32),
        cover_image: opt_str(&cover["extraLarge"]).or_else(|| opt_str(&cover["large"])),
        color: opt_str(&cover["color"]),
        banner_image: opt_str(&media["bannerImage"]),
        average_score: media["averageScore"].as_i64().map(|v| v as i32),
        genres: media["genres"]
            .as_array()
            .map(|a| a.iter().filter_map(opt_str).collect())
            .unwrap_or_default(),
        is_adult: media["isAdult"].as_bool().unwrap_or(false),
        score: 0.0,
    })
}

/// The results of alias `s{index}` of a batched search.
pub fn candidates_of_alias(data: &Value, index: usize) -> Vec<MatchCandidate> {
    data[format!("s{}", index)]["media"]
        .as_array()
        .map(|list| list.iter().filter_map(parse_candidate).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------
// Database

const MATCHES_DDL: &str = r#"
    CREATE TABLE IF NOT EXISTS local_library_matches (
        series_key      TEXT PRIMARY KEY,
        status          TEXT NOT NULL,
        anilist_id      INTEGER,
        media_json      TEXT,
        candidates_json TEXT NOT NULL DEFAULT '[]',
        updated_at      INTEGER NOT NULL
    );
"#;

pub async fn create_tables(conn: &mut SqliteConnection) -> Result<(), AppError> {
    sqlx::query(MATCHES_DDL).execute(&mut *conn).await?;
    Ok(())
}

fn status_text(status: MatchStatus) -> &'static str {
    match status {
        MatchStatus::Auto => "auto",
        MatchStatus::Confirmed => "confirmed",
        MatchStatus::None => "none",
    }
}

fn status_from(text: &str) -> MatchStatus {
    match text {
        "confirmed" => MatchStatus::Confirmed,
        "auto" => MatchStatus::Auto,
        _ => MatchStatus::None,
    }
}

/// Cached matches of the given series keys.
pub async fn load_matches(pool: &SqlitePool, keys: &[String]) -> Result<HashMap<String, SeriesMatch>, AppError> {
    let mut found = HashMap::new();
    for chunk in keys.chunks(400) {
        let placeholders = vec!["?"; chunk.len()].join(", ");
        let sql = format!(
            "SELECT series_key, status, media_json, candidates_json, updated_at
             FROM local_library_matches WHERE series_key IN ({})",
            placeholders
        );
        let mut query = sqlx::query(&sql);
        for key in chunk {
            query = query.bind(key);
        }
        for row in query.fetch_all(pool).await? {
            let key: String = row.get("series_key");
            let media: Option<String> = row.get("media_json");
            let candidates: String = row.get("candidates_json");
            found.insert(
                key.clone(),
                SeriesMatch {
                    key,
                    status: status_from(&row.get::<String, _>("status")),
                    media: media.and_then(|m| serde_json::from_str(&m).ok()),
                    candidates: serde_json::from_str(&candidates).unwrap_or_default(),
                    updated_at: row.get("updated_at"),
                },
            );
        }
    }
    Ok(found)
}

pub async fn save_match(pool: &SqlitePool, m: &SeriesMatch) -> Result<(), AppError> {
    let media = m.media.as_ref().map(|c| serde_json::to_string(c)).transpose()?;
    sqlx::query(
        r#"
        INSERT INTO local_library_matches
            (series_key, status, anilist_id, media_json, candidates_json, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(series_key) DO UPDATE SET
            status = excluded.status,
            anilist_id = excluded.anilist_id,
            media_json = excluded.media_json,
            candidates_json = excluded.candidates_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&m.key)
    .bind(status_text(m.status))
    .bind(m.media.as_ref().map(|c| c.id))
    .bind(media)
    .bind(serde_json::to_string(&m.candidates)?)
    .bind(m.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Stores `candidate` as the user's choice for the series, keeping cached search results.
pub async fn confirm_match(pool: &SqlitePool, key: &str, candidate: MatchCandidate) -> Result<SeriesMatch, AppError> {
    let mut candidates = load_matches(pool, &[key.to_string()])
        .await?
        .remove(key)
        .map(|m| m.candidates)
        .unwrap_or_default();
    if !candidates.iter().any(|c| c.id == candidate.id) {
        candidates.insert(0, candidate.clone());
    }
    let m = SeriesMatch {
        key: key.to_string(),
        status: MatchStatus::Confirmed,
        media: Some(candidate),
        candidates,
        updated_at: current_unix_millis(),
    };
    save_match(pool, &m).await?;
    Ok(m)
}

pub async fn clear_match(pool: &SqlitePool, key: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM local_library_matches WHERE series_key = ?1")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

/// Every file linked under "Your files", by path, with its title.
pub async fn linked_files(pool: &SqlitePool) -> Result<HashMap<String, i32>, AppError> {
    // A stand-in merged into its AniList id has the same files under both; the AniList id wins.
    let rows = sqlx::query(
        "SELECT file_path, anilist_id FROM local_media_files \
         WHERE anilist_id NOT IN (SELECT stand_in FROM catalogue_merged_ids)",
    )
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get::<String, _>("file_path"), r.get::<i32, _>("anilist_id")))
        .collect())
}
