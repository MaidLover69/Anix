//! Commands of Library › Local files: scanning a folder into series and matching each series
//! to an AniList title. Linking a match uses the "Your files" commands (`link_local_folder`,
//! `add_local_files`).
//!
//! AniList allows about 30 requests a minute when it is under load, so every search made here
//! goes through one gate: requests run one at a time, at least [`MIN_REQUEST_GAP`] apart,
//! several series share one request, and after a 429 nothing is sent until it has expired.

use crate::catalogue::breaker;
use crate::catalogue::model::anilist_json;
use crate::catalogue::{self, Catalogue, Found, SearchQuery, Source};
use crate::clients::anilist::post_graphql_to;
use crate::clients::app_settings;
use crate::commands::local_scanner::scan_local_anime_directory;
use crate::db::current_unix_millis;
use crate::error::AppError;
use crate::local_library::{
    self, batched_search_body, candidates_of_alias, CANDIDATES_PER_SEARCH, match_from_candidates, rank_candidates,
    search_terms, LocalLibraryScan, MatchCandidate, MatchRequest, SeriesMatch,
};
use crate::state::AppState;
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};
use tauri::State;

const ANILIST: &str = "AniList";

pub const MIN_REQUEST_GAP: Duration = Duration::from_millis(2100);
/// Series searched per request; each takes one or two of its aliases.
pub const SERIES_PER_REQUEST: usize = 3;

#[derive(Default)]
struct Gate {
    last: Option<Instant>,
    blocked_until: Option<Instant>,
}

static GATE: Lazy<tokio::sync::Mutex<Gate>> = Lazy::new(|| tokio::sync::Mutex::new(Gate::default()));

async fn anilist_search(client: &reqwest::Client, url: &str, terms: &[String]) -> Result<Value, AppError> {
    let mut gate = GATE.lock().await;
    if let Some(until) = gate.blocked_until {
        let now = Instant::now();
        if until > now {
            return Err(AppError::RateLimited {
                retry_after_s: (until - now).as_secs().max(1),
            });
        }
        gate.blocked_until = None;
    }
    if let Some(last) = gate.last {
        let next = last + MIN_REQUEST_GAP;
        if next > Instant::now() {
            tokio::time::sleep_until(next.into()).await;
        }
    }
    let body = batched_search_body(terms, app_settings::current().hide_adult_content);
    let result = post_graphql_to(client, url, &body, None).await;
    gate.last = Some(Instant::now());
    if let Err(AppError::RateLimited { retry_after_s }) = &result {
        gate.blocked_until = Some(Instant::now() + Duration::from_secs((*retry_after_s).max(1)));
    }
    result
}

/// Runs the searches on AniList, or, while AniList is unavailable, on the fallback catalogues
/// (one request per term), in the same answer shape.
async fn gated_search(state: &AppState, terms: &[String]) -> Result<Value, AppError> {
    search_with_fallback(&state.catalogue, &state.client, terms).await
}

/// [`gated_search`] without the app state. AniList is skipped while the catalogue has it paused
/// or the debug switch `ANIX_CATALOGUE_DOWN` names it, as for every other catalogue request.
pub async fn search_with_fallback(catalogue: &Catalogue, client: &reqwest::Client, terms: &[String]) -> Result<Value, AppError> {
    let breaker = catalogue.breaker();
    if !breaker.is_open(ANILIST) {
        let result = if catalogue::simulated_down(Source::AniList) {
            Err(AppError::Network(format!("{} HTTP 503: switched off by ANIX_CATALOGUE_DOWN", ANILIST)))
        } else {
            anilist_search(client, catalogue.anilist_url(), terms).await
        };
        // A rate limit is waited out by the caller, not answered elsewhere.
        if matches!(result, Err(AppError::RateLimited { .. })) || !result.as_ref().is_err_and(breaker::trips) {
            return result;
        }
        breaker.record(ANILIST, &result);
    }
    let hide_adult = app_settings::current().hide_adult_content;
    let mut data = serde_json::Map::new();
    for (i, term) in terms.iter().enumerate() {
        let query = SearchQuery {
            query: Some(term.clone()),
            page: 1,
            per_page: CANDIDATES_PER_SEARCH as i32,
            hide_adult,
            ..Default::default()
        };
        let served = catalogue.search(&query).await?;
        let media: Vec<Value> = served
            .value
            .into_iter()
            .map(|media| anilist_json(&Found { media, ..Default::default() }))
            .collect();
        data.insert(format!("s{}", i), serde_json::json!({ "media": media }));
    }
    Ok(Value::Object(data))
}

/// Scans `dir_path` and groups its video files into series, with cached matches attached.
#[tauri::command]
pub async fn scan_local_library(
    state: State<'_, AppState>,
    dir_path: String,
) -> Result<LocalLibraryScan, String> {
    let files = scan_local_anime_directory(dir_path.clone()).await?;
    let root = std::path::absolute(&dir_path).map_err(|e| e.to_string())?;
    let linked = local_library::linked_files(&state.pool)
        .await
        .map_err(|e| e.to_string())?;
    let file_count = files.len();
    let root_for_parse = root.clone();
    let mut series = tokio::task::spawn_blocking(move || {
        local_library::build_library(&root_for_parse, files, &linked)
    })
    .await
    .map_err(|e| format!("Grouping the files failed: {}", e))?;

    let keys: Vec<String> = series.iter().filter(|s| s.matchable).map(|s| s.key.clone()).collect();
    let mut cached = local_library::load_matches(&state.pool, &keys)
        .await
        .map_err(|e| e.to_string())?;
    for s in &mut series {
        s.series_match = cached.remove(&s.key);
    }
    Ok(LocalLibraryScan {
        root: root.to_string_lossy().to_string(),
        file_count,
        series,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchBatch {
    /// Matches found or read from the cache in this call.
    pub matches: Vec<SeriesMatch>,
    /// Keys still to search; call again with them.
    pub pending: Vec<String>,
    /// Set when AniList is rate limiting: wait this long before calling again.
    pub retry_after_s: Option<u64>,
    /// Set when the search failed for another reason.
    pub error: Option<String>,
}

/// Matches series to AniList titles. Fresh cached matches are returned without a request
/// (unless `force`); of the rest, up to [`SERIES_PER_REQUEST`] are searched in one request
/// and the others are returned as `pending`.
#[tauri::command]
pub async fn match_local_series(
    state: State<'_, AppState>,
    requests: Vec<MatchRequest>,
    force: Option<bool>,
) -> Result<MatchBatch, String> {
    let force = force.unwrap_or(false);
    let now = current_unix_millis();
    let keys: Vec<String> = requests.iter().map(|r| r.key.clone()).collect();
    let mut cached = local_library::load_matches(&state.pool, &keys)
        .await
        .map_err(|e| e.to_string())?;

    let mut batch = MatchBatch { matches: Vec::new(), pending: Vec::new(), retry_after_s: None, error: None };
    let mut to_search: Vec<(MatchRequest, Vec<String>)> = Vec::new();
    for request in requests {
        if request.key.is_empty() {
            continue;
        }
        if !force {
            if let Some(m) = cached.remove(&request.key).filter(|m| m.is_fresh(now)) {
                batch.matches.push(m);
                continue;
            }
        }
        let terms = search_terms(&request);
        if terms.is_empty() {
            continue;
        }
        if to_search.len() < SERIES_PER_REQUEST {
            to_search.push((request, terms));
        } else {
            batch.pending.push(request.key);
        }
    }
    if to_search.is_empty() {
        return Ok(batch);
    }

    let all_terms: Vec<String> = to_search.iter().flat_map(|(_, t)| t.iter().cloned()).collect();
    match gated_search(&state, &all_terms).await {
        Ok(data) => {
            let mut alias = 0;
            for (request, terms) in &to_search {
                // The more specific search (with season or type) first: it ranks better.
                let mut lists: Vec<Vec<MatchCandidate>> =
                    (0..terms.len()).map(|i| candidates_of_alias(&data, alias + i)).collect();
                alias += terms.len();
                lists.reverse();
                let candidates: Vec<MatchCandidate> = lists.into_iter().flatten().collect();
                let m = match_from_candidates(request, candidates, current_unix_millis());
                local_library::save_match(&state.pool, &m)
                    .await
                    .map_err(|e| e.to_string())?;
                batch.matches.push(m);
            }
        }
        Err(e) => {
            if let AppError::RateLimited { retry_after_s } = e {
                batch.retry_after_s = Some(retry_after_s);
            } else {
                batch.error = Some(e.to_string());
            }
            let later = std::mem::take(&mut batch.pending);
            batch.pending = to_search.into_iter().map(|(r, _)| r.key).chain(later).collect();
        }
    }
    Ok(batch)
}

/// Searches AniList for `query`; with `request`, results are scored against that series and
/// sorted best first.
#[tauri::command]
pub async fn search_local_match(
    state: State<'_, AppState>,
    query: String,
    request: Option<MatchRequest>,
) -> Result<Vec<MatchCandidate>, String> {
    let query = query.trim().to_string();
    if query.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let data = gated_search(&state, std::slice::from_ref(&query))
        .await
        .map_err(|e| e.to_string())?;
    let candidates = candidates_of_alias(&data, 0);
    Ok(match request {
        Some(request) => rank_candidates(&request, candidates),
        None => candidates,
    })
}

/// Stores the user's choice of title for a series.
#[tauri::command]
pub async fn set_local_series_match(
    state: State<'_, AppState>,
    series_key: String,
    candidate: MatchCandidate,
) -> Result<SeriesMatch, String> {
    if series_key.is_empty() {
        return Err("This group of files has no title to match".to_string());
    }
    local_library::confirm_match(&state.pool, &series_key, candidate)
        .await
        .map_err(|e| e.to_string())
}

/// Forgets the match of a series, so it is searched for again.
#[tauri::command]
pub async fn clear_local_series_match(state: State<'_, AppState>, series_key: String) -> Result<(), String> {
    local_library::clear_match(&state.pool, &series_key)
        .await
        .map_err(|e| e.to_string())
}
