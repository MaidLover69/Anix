use crate::accounts;
use crate::catalogue::{SearchQuery, Served, Source};
use crate::db;
use crate::offline;
use crate::models::{Media, MediaListEntry, Viewer};
use crate::state::AppState;
use accounts::oauth::Service;
use accounts::session;
use accounts::sync::{self, SavedEntry};
use serde_json::json;
use tauri::{AppHandle, State};

/// Home screen data. `continue_watching` comes from the local database and is always present.
/// The lists come from the first catalogue that answers (AniList, else its fallbacks). When
/// none can be reached they come from the last saved copy (`stale` is then true and
/// `saved_at` says when it was saved) or are empty, and `error` says why; if there is nothing
/// to show at all, the command fails with that reason instead.
#[tauri::command]
pub async fn fetch_home_data(app: AppHandle, state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let hide_adult = crate::clients::app_settings::current().hide_adult_content;
    let queries = ["TRENDING_DESC", "POPULARITY_DESC", "SCORE_DESC", "START_DATE_DESC"]
        .map(|sort| SearchQuery::list(sort, 10, hide_adult));
    let (trending, popular, top_rated, newest, history) = tokio::join!(
        state.catalogue.search(&queries[0]),
        state.catalogue.search(&queries[1]),
        state.catalogue.search(&queries[2]),
        state.catalogue.search(&queries[3]),
        db::get_continue_watching(&state.pool)
    );

    let history = history.unwrap_or_else(|e| {
        tracing::warn!("continue-watching list unavailable: {}", e);
        Default::default()
    });

    let mut error: Option<crate::error::AppError> = None;
    let mut notice: Option<&'static str> = None;
    let mut list_or_empty = |name: &str, result: Result<Served<Vec<Media>>, crate::error::AppError>| match result {
        Ok(served) => {
            notice = notice.or(served.notice());
            Some(served.value)
        }
        Err(e) => {
            tracing::warn!("The {} list failed: {}", name, e);
            error.get_or_insert(e);
            None
        }
    };
    let mut lists = [
        list_or_empty("trending", trending),
        list_or_empty("popular", popular),
        list_or_empty("top rated", top_rated),
        list_or_empty("newest", newest),
    ];
    const NAMES: [&str; 4] = ["trending", "popular", "top_rated", "newest"];
    let home_key = home_cache_key();

    let mut stale = false;
    let mut saved_at: Option<i64> = None;
    match &error {
        None => {
            let saved: serde_json::Map<String, serde_json::Value> = NAMES
                .iter()
                .zip(lists.iter())
                .map(|(name, list)| (name.to_string(), json!(list.as_deref().unwrap_or_default())))
                .collect();
            if let Err(e) = offline::store(&state.pool, offline::Kind::Home, &home_key, &saved.into()).await {
                tracing::warn!("Could not save the home lists for offline use: {}", e);
            }
            offline::mark_served(&app, notice);
        }
        Some(e) if offline::is_offline_error(e) => {
            if let Ok(Some((saved, at))) = offline::load(&state.pool, offline::Kind::Home, &home_key).await {
                for (name, list) in NAMES.iter().zip(lists.iter_mut()) {
                    if list.is_none() {
                        *list = serde_json::from_value::<Vec<Media>>(saved[*name].clone()).ok();
                    }
                }
                stale = true;
                saved_at = Some(at);
            }
            offline::mark_offline(&app, saved_at);
        }
        Some(_) => {}
    }

    let [trending, popular, top_rated, newest] = lists.map(Option::unwrap_or_default);
    let nothing_from_anilist =
        trending.is_empty() && popular.is_empty() && top_rated.is_empty() && newest.is_empty();
    let error = error.map(|e| e.to_string());
    if let Some(reason) = &error {
        if nothing_from_anilist && history.is_empty() {
            return Err(reason.clone());
        }
    }

    Ok(json!({
        "trending": trending,
        "popular": popular,
        "top_rated": top_rated,
        "newest": newest,
        "continue_watching": history,
        "error": error,
        "stale": stale,
        "saved_at": saved_at,
    }))
}

/// The home lists depend on the adult-content setting, so each setting has its own copy.
fn home_cache_key() -> String {
    if crate::clients::app_settings::current().hide_adult_content {
        "home".to_string()
    } else {
        "home+adult".to_string()
    }
}

/// Anime search, from the first catalogue that answers. Results are saved for offline use;
/// when no catalogue can be reached, the saved results of the same search are returned.
#[tauri::command]
pub async fn search_anime(
    app: AppHandle,
    state: State<'_, AppState>,
    query: Option<String>,
    genres: Option<Vec<String>>,
    format: Option<String>,
    status: Option<String>,
    sort: Option<String>,
    page: Option<i32>,
    per_page: Option<i32>,
    exclude_genres: Option<Vec<String>>,
    min_score: Option<i32>,
    year: Option<i32>,
    season: Option<String>,
) -> Result<Vec<Media>, String> {
    const SEASONS: [&str; 4] = ["WINTER", "SPRING", "SUMMER", "FALL"];
    let request = SearchQuery {
        query,
        genres: genres.unwrap_or_default(),
        format: format.filter(|f| !f.trim().is_empty()),
        status: status.filter(|s| !s.trim().is_empty()),
        sort,
        page: page.unwrap_or(1),
        per_page: per_page.unwrap_or(20),
        hide_adult: crate::clients::app_settings::current().hide_adult_content,
        exclude_genres: exclude_genres.unwrap_or_default(),
        min_score: min_score.filter(|s| (1..=100).contains(s)),
        year: year.filter(|y| (1900..=3000).contains(y)),
        season: season.filter(|s| SEASONS.contains(&s.as_str())),
    };
    let mut key = offline::search_key(
        request.query.as_deref(),
        &request.genres,
        request.format.as_deref(),
        request.status.as_deref(),
        request.sort.as_deref(),
        request.page,
        request.per_page,
        request.hide_adult,
    );
    // Only searches with the newer filters get a longer key, so saved results of plain ones still match.
    if !request.exclude_genres.is_empty() || request.min_score.is_some() || request.year.is_some() || request.season.is_some() {
        key.push_str(&json!([request.exclude_genres, request.min_score, request.year, request.season]).to_string());
    }
    match state.catalogue.search(&request).await {
        Ok(served) => {
            if let Err(e) = offline::store(&state.pool, offline::Kind::Search, &key, &json!(served.value)).await {
                tracing::warn!("Could not save search results for offline use: {}", e);
            }
            offline::mark_served(&app, served.notice());
            Ok(served.value)
        }
        Err(e) if offline::is_offline_error(&e) => {
            let saved = offline::load(&state.pool, offline::Kind::Search, &key).await.ok().flatten();
            let parsed = saved.and_then(|(value, at)| {
                serde_json::from_value::<Vec<Media>>(value).ok().map(|list| (list, at))
            });
            match parsed {
                Some((list, at)) => {
                    offline::mark_offline(&app, Some(at));
                    Ok(list)
                }
                None => {
                    offline::mark_offline(&app, None);
                    Err(e.to_string())
                }
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

/// One anime's details (`id` is an AniList id or a synthetic one, see `catalogue::ids`), in
/// AniList's shape whichever catalogue answered. Saved for offline use; a fallback
/// catalogue's copy does not replace a saved AniList copy, which has more in it.
#[tauri::command]
pub async fn fetch_anime_details(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    let key = id.to_string();
    match state.catalogue.details(id).await {
        Ok(served) => {
            let keep = served.source == Source::AniList
                || matches!(offline::load(&state.pool, offline::Kind::Details, &key).await, Ok(None));
            if keep {
                if let Err(e) = offline::store(&state.pool, offline::Kind::Details, &key, &served.value).await {
                    tracing::warn!("Could not save anime {} for offline use: {}", id, e);
                }
            }
            offline::mark_served(&app, served.notice());
            Ok(served.value)
        }
        Err(e) if offline::is_offline_error(&e) => {
            match offline::load(&state.pool, offline::Kind::Details, &key).await.ok().flatten() {
                Some((details, at)) => {
                    offline::mark_offline(&app, Some(at));
                    Ok(details)
                }
                None => {
                    offline::mark_offline(&app, None);
                    Err(e.to_string())
                }
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Whether AniList data is currently served from the saved copy.
#[tauri::command]
pub async fn get_offline_status() -> Result<offline::OfflineStatus, String> {
    Ok(offline::status())
}

/// The connected AniList account. Fails with a sentence for the user when no account is
/// connected or AniList no longer accepts the saved login.
#[tauri::command]
pub async fn get_anilist_viewer(app: AppHandle, state: State<'_, AppState>) -> Result<Viewer, String> {
    let (store, notifier) = accounts::parts(&app)?;
    let ctx = accounts::context(&state, &store, &notifier);
    let connected = store.is_connected(Service::AniList);
    session::refresh_anilist_profile(&ctx)
        .await
        .map_err(|e| session::error_message(Service::AniList, connected, &e))
}

/// The connected account's anime list, optionally one status only.
#[tauri::command]
pub async fn fetch_anilist_library(
    app: AppHandle,
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<MediaListEntry>, String> {
    let (store, notifier) = accounts::parts(&app)?;
    let ctx = accounts::context(&state, &store, &notifier);
    let connected = store.is_connected(Service::AniList);
    let entries = async {
        let user_id = session::anilist_viewer_id(&ctx).await?;
        session::with_anilist(&ctx, |token| async move {
            ctx.anilist.get_user_library(&token, user_id, status).await
        })
        .await
    }
    .await;
    entries.map_err(|e| session::error_message(Service::AniList, connected, &e))
}

/// Saves the status and progress chosen in the AniList tab to AniList and, when it is
/// connected and knows the anime, to MyAnimeList.
#[tauri::command]
pub async fn save_anilist_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    media_id: i32,
    status: Option<String>,
    progress: Option<i32>,
) -> Result<SavedEntry, String> {
    let (store, notifier) = accounts::parts(&app)?;
    let ctx = accounts::context(&state, &store, &notifier);
    let connected = store.is_connected(Service::AniList);
    sync::save_entry(&ctx, media_id, status, progress)
        .await
        .map_err(|e| session::error_message(Service::AniList, connected, &e))
}
