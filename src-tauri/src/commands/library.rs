use crate::accounts;
use crate::db;
use crate::models::{HistoryEntry, MediaListEntry, WatchlistEntry};
use crate::state::AppState;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn get_watched_episodes(
    state: State<'_, AppState>,
    anilist_id: i32,
) -> Result<Vec<f64>, String> {
    db::get_watched_episodes(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_episode_progress(
    state: State<'_, AppState>,
    anilist_id: i32,
    episode_number: f64,
) -> Result<Option<(i64, i64)>, String> {
    db::get_episode_progress(&state.pool, anilist_id, episode_number)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_continue_watching(
    state: State<'_, AppState>,
) -> Result<Vec<HistoryEntry>, String> {
    db::get_continue_watching(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

/// Saves the position of one episode. The save that takes the episode past the watched
/// threshold also sends it to the connected AniList and MyAnimeList accounts, in the
/// background; a failure there never fails the save.
#[tauri::command]
pub async fn update_watch_progress(
    app: AppHandle,
    state: State<'_, AppState>,
    anilist_id: i32,
    title: String,
    cover: Option<String>,
    episode_number: f64,
    episode_title: Option<String>,
    provider: String,
    category: String,
    position_ms: i64,
    duration_ms: i64,
) -> Result<(), String> {
    if !episode_number.is_finite() {
        return Err("Invalid episode number".to_string());
    }

    // A duration of a second or less is the player's placeholder for "not known yet";
    // storing it would make the first second of playback count as the whole episode.
    let duration_ms = if duration_ms > 1000 { duration_ms } else { 0 };

    let entry = HistoryEntry {
        anilist_id,
        title,
        cover,
        episode_number,
        episode_title,
        provider,
        category,
        position_ms,
        duration_ms,
        updated_at: db::current_unix_millis(),
        from_remote: false,
    };

    let just_watched = accounts::sync::save_watch_progress(&state.pool, &entry)
        .await
        .map_err(|e| e.to_string())?;
    if just_watched {
        accounts::episode_watched(&app, &state, anilist_id, episode_number).await;
    }
    Ok(())
}

/// Adds an anime to the in-app watchlist and, in the background, to the connected lists as
/// planning when it is not on them yet.
#[tauri::command]
pub async fn add_to_watchlist(
    app: AppHandle,
    state: State<'_, AppState>,
    anilist_id: i32,
    title: String,
    cover: Option<String>,
    format: Option<String>,
    average_score: Option<i32>,
) -> Result<(), String> {
    db::add_to_watchlist(
        &state.pool,
        anilist_id,
        &title,
        cover.as_deref(),
        format.as_deref(),
        average_score,
    )
    .await
    .map_err(|e| e.to_string())?;
    accounts::watchlist_added(&app, &state, anilist_id).await;
    Ok(())
}

/// Removes an anime from the in-app watchlist only.
#[tauri::command]
pub async fn remove_from_watchlist(
    state: State<'_, AppState>,
    anilist_id: i32,
) -> Result<(), String> {
    db::remove_from_watchlist(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())?;
    // Only an addition that was never pushed is taken back; the remote lists keep their entry.
    if let Err(e) = accounts::sync::cancel_watchlist_addition(&state.pool, anilist_id).await {
        tracing::warn!("could not clear the queued watchlist addition of anime {}: {}", anilist_id, e);
    }
    Ok(())
}

#[tauri::command]
pub async fn is_in_watchlist(
    state: State<'_, AppState>,
    anilist_id: i32,
) -> Result<bool, String> {
    db::is_in_watchlist(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_watchlist(
    state: State<'_, AppState>,
) -> Result<Vec<WatchlistEntry>, String> {
    db::get_watchlist(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_library_entry(
    state: State<'_, AppState>,
    entry: MediaListEntry,
) -> Result<(), String> {
    db::upsert_library_entry(&state.pool, &entry)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_library_entries(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<MediaListEntry>, String> {
    db::get_library_entries(&state.pool, status)
        .await
        .map_err(|e| e.to_string())
}
