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

/// The airing schedule between two times, from AniList or, while it is unavailable, built
/// from Shikimori's or MyAnimeList's weekly data (times are then estimates). When no
/// catalogue can be reached, the saved schedules that cover the window are served instead.
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

    let hide_adult = crate::clients::app_settings::current().hide_adult_content;
    let served = match state.catalogue.schedule(from_ts, to_ts, hide_adult).await {
        Ok(served) => served,
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
    offline::mark_served(&app, served.notice());
    let entries = served.value;

    // An estimated schedule is not kept as the window's answer, so AniList's replaces it
    // as soon as AniList is back.
    if served.source == crate::catalogue::Source::AniList {
        if let Err(e) = db::cache_schedule(&state.pool, from_ts, to_ts, &entries).await {
            tracing::warn!("Could not cache the airing schedule: {}", e);
        }
    }
    // An estimate is saved only where nothing is, so it never replaces AniList's.
    let key = format!("{}-{}", from_ts, to_ts);
    let keep = served.source == crate::catalogue::Source::AniList
        || matches!(offline::load(&state.pool, offline::Kind::Schedule, &key).await, Ok(None));
    if keep {
        if let Err(e) = offline::store(&state.pool, offline::Kind::Schedule, &key, &serde_json::json!(entries)).await {
            tracing::warn!("Could not save the airing schedule for offline use: {}", e);
        }
    }
    Ok(entries)
}

/// Switches the bell for one anime on or off and has the notifier plan again right away.
#[tauri::command]
pub async fn save_notification_preference(
    state: State<'_, AppState>,
    pref: NotificationPreference,
) -> Result<(), String> {
    // Negative ids are catalogue stand-ins; their bells use estimated times plus release checks.
    if pref.media_id == 0 {
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

/// Sets how many minutes before an episode airs the bell of `media_id` reminds (0 is off).
#[tauri::command]
pub async fn set_bell_lead_time(state: State<'_, AppState>, media_id: i64, minutes: i64) -> Result<(), String> {
    crate::reminders::set_lead_minutes(&state.pool, media_id, minutes)
        .await
        .map_err(|e| e.to_string())?;
    BELLS_CHANGED.notify_one();
    Ok(())
}

/// The reminder lead time of every bell that has one, as `[media_id, minutes]` pairs.
#[tauri::command]
pub async fn list_bell_lead_times(state: State<'_, AppState>) -> Result<Vec<(i64, i64)>, String> {
    let leads = crate::reminders::lead_minutes(&state.pool).await.map_err(|e| e.to_string())?;
    Ok(leads.into_iter().collect())
}
