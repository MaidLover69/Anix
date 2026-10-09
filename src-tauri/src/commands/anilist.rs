use crate::accounts;
use crate::db;
use crate::models::{Media, MediaListEntry, Viewer};
use crate::state::AppState;
use accounts::oauth::Service;
use accounts::session;
use accounts::sync::{self, SavedEntry};
use serde_json::json;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn fetch_trending(state: State<'_, AppState>) -> Result<Vec<Media>, String> {
    state
        .anilist_client
        .fetch_media_list("TRENDING_DESC", 20)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_popular(state: State<'_, AppState>) -> Result<Vec<Media>, String> {
    state
        .anilist_client
        .fetch_media_list("POPULARITY_DESC", 20)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_top_rated(state: State<'_, AppState>) -> Result<Vec<Media>, String> {
    state
        .anilist_client
        .fetch_media_list("SCORE_DESC", 20)
        .await
        .map_err(|e| e.to_string())
}

/// Home screen data. `continue_watching` comes from the local database and is always present.
/// When AniList cannot be reached its lists are empty and `error` says why; if there is no local
/// history to show either, the command fails with that reason instead.
#[tauri::command]
pub async fn fetch_home_data(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let trending_fut = state.anilist_client.fetch_media_list("TRENDING_DESC", 10);
    let popular_fut = state.anilist_client.fetch_media_list("POPULARITY_DESC", 10);
    let top_rated_fut = state.anilist_client.fetch_media_list("SCORE_DESC", 10);
    let newest_fut = state.anilist_client.fetch_media_list("START_DATE_DESC", 10);
    let history_fut = db::get_continue_watching(&state.pool);

    let (trending, popular, top_rated, newest, history) = tokio::join!(
        trending_fut,
        popular_fut,
        top_rated_fut,
        newest_fut,
        history_fut
    );

    let history = history.unwrap_or_else(|e| {
        tracing::warn!("continue-watching list unavailable: {}", e);
        Default::default()
    });

    let mut error: Option<String> = None;
    let mut list_or_empty = |name: &str, result: Result<Vec<Media>, crate::error::AppError>| match result {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!("AniList {} list failed: {}", name, e);
            error.get_or_insert_with(|| e.to_string());
            Vec::new()
        }
    };
    let trending = list_or_empty("trending", trending);
    let popular = list_or_empty("popular", popular);
    let top_rated = list_or_empty("top rated", top_rated);
    let newest = list_or_empty("newest", newest);

    let nothing_from_anilist =
        trending.is_empty() && popular.is_empty() && top_rated.is_empty() && newest.is_empty();
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
    }))
}

#[tauri::command]
pub async fn search_anime(
    state: State<'_, AppState>,
    query: Option<String>,
    genres: Option<Vec<String>>,
    format: Option<String>,
    status: Option<String>,
    sort: Option<String>,
    page: Option<i32>,
    per_page: Option<i32>,
) -> Result<Vec<Media>, String> {
    state
        .anilist_client
        .search(
            query,
            genres.unwrap_or_default(),
            format,
            status,
            sort,
            page.unwrap_or(1),
            per_page.unwrap_or(20),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_anime_details(
    state: State<'_, AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    state
        .anilist_client
        .fetch_details(id)
        .await
        .map_err(|e| e.to_string())
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
