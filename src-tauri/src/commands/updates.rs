use crate::clients::github::{fetch_latest_release, UpdateInfo};
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn check_for_update(state: State<'_, AppState>) -> Result<Option<UpdateInfo>, String> {
    fetch_latest_release(&state.client).await
}
