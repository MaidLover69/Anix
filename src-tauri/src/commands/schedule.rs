use crate::db;
use crate::offline;
use crate::models::{AiringEntry, NotificationPreference};
use crate::state::AppState;
use once_cell::sync::Lazy;
use tauri::{AppHandle, State};
use tokio::sync::Notify;

/// Signalled whenever a bell changes; the notifier waits on it.
pub(crate) static BELLS_CHANGED: Lazy<Notify> = Lazy::new(Notify::new);
/// The same signal for the tray's "Airing next" list.
pub(crate) static BELLS_CHANGED_FOR_TRAY: Lazy<Notify> = Lazy::new(Notify::new);

/// The airing schedule between two times. When AniList cannot be reached, the saved schedules
/// that cover the window are served instead.
#[tauri::command]
pub async fn fetch_airing_schedule(
    app: AppHandle,
    state: State<'_, AppState>,
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<AiringEntry>, String> {
    if let Ok(Some(cached)) = db::get_cached_schedule(&state.pool, from_ts, to_ts).await {
        return Ok(cached);
    }

    let entries = match state.anilist_client.fetch_airing_schedule(from_ts, to_ts).await {
        Ok(entries) => entries,
        Err(e) if offline::is_offline_error(&e) => {
            let saved = offline::load_all(&state.pool, offline::Kind::Schedule).await.unwrap_or_default();
            let (entries, saved_at) = offline::saved_schedule(saved, from_ts, to_ts);
            offline::mark_offline(&app, saved_at);
            if saved_at.is_none() {
                return Err(e.to_string());
            }
            return Ok(entries);
        }
        Err(e) => return Err(e.to_string()),
    };
    offline::mark_online(&app);

    if let Err(e) = db::cache_schedule(&state.pool, from_ts, to_ts, &entries).await {
        tracing::warn!("Could not cache the airing schedule: {}", e);
    }
    let key = format!("{}-{}", from_ts, to_ts);
    if let Err(e) = offline::store(&state.pool, offline::Kind::Schedule, &key, &serde_json::json!(entries)).await {
        tracing::warn!("Could not save the airing schedule for offline use: {}", e);
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
    BELLS_CHANGED_FOR_TRAY.notify_one();
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
