use crate::db;
use crate::models::{AiringEntry, NotificationPreference};
use crate::state::AppState;
use once_cell::sync::Lazy;
use tauri::State;
use tokio::sync::Notify;

/// Signalled whenever a bell changes; the notifier waits on it.
pub(crate) static BELLS_CHANGED: Lazy<Notify> = Lazy::new(Notify::new);

#[tauri::command]
pub async fn fetch_airing_schedule(
    state: State<'_, AppState>,
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<AiringEntry>, String> {
    if let Ok(Some(cached)) = db::get_cached_schedule(&state.pool, from_ts, to_ts).await {
        return Ok(cached);
    }

    let entries = state
        .anilist_client
        .fetch_airing_schedule(from_ts, to_ts)
        .await
        .map_err(|e| e.to_string())?;

    if let Err(e) = db::cache_schedule(&state.pool, from_ts, to_ts, &entries).await {
        tracing::warn!("Could not cache the airing schedule: {}", e);
    }
    Ok(entries)
}

/// Switches the bell for one anime on or off and has the notifier plan again right away.
#[tauri::command]
pub async fn save_notification_preference(
    state: State<'_, AppState>,
    pref: NotificationPreference,
) -> Result<(), String> {
    if pref.media_id <= 0 {
        return Err("Unknown anime".to_string());
    }
    db::save_notification_preference(&state.pool, &pref)
        .await
        .map_err(|e| e.to_string())?;
    BELLS_CHANGED.notify_one();
    Ok(())
}

/// Every bell that is switched on.
#[tauri::command]
pub async fn list_notification_preferences(
    state: State<'_, AppState>,
) -> Result<Vec<NotificationPreference>, String> {
    db::list_notification_preferences(&state.pool)
        .await
        .map_err(|e| e.to_string())
}
