use crate::notices::{self, Notice};
use crate::state::AppState;
use tauri::State;

/// The notification centre, newest first; `kind` limits it to one kind.
#[tauri::command]
pub async fn list_notices(state: State<'_, AppState>, kind: Option<String>) -> Result<Vec<Notice>, String> {
    notices::list(&state.pool, kind.as_deref()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_unread_notice_count(state: State<'_, AppState>) -> Result<i64, String> {
    notices::unread_count(&state.pool).await.map_err(|e| e.to_string())
}

/// Marks `ids` read, or every notice when `ids` is not given.
#[tauri::command]
pub async fn mark_notices_read(state: State<'_, AppState>, ids: Option<Vec<i64>>) -> Result<u64, String> {
    notices::mark_read(&state.pool, ids.as_deref()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_notices(state: State<'_, AppState>) -> Result<(), String> {
    notices::clear(&state.pool).await.map_err(|e| e.to_string())
}

/// Adds a sample notice, to see how notices look.
#[tauri::command]
pub async fn push_test_notice(state: State<'_, AppState>) -> Result<i64, String> {
    notices::push(
        &state.pool,
        notices::TEST,
        "Test notification",
        "Notifications from Anix show up here.",
        Some("/notifications"),
    )
    .await
    .map_err(|e| e.to_string())
}
