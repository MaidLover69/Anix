use crate::clients::update::{fetch_latest_release, UpdateInfo, SITE_URL};
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn check_for_update(state: State<'_, AppState>) -> Result<Option<UpdateInfo>, String> {
    fetch_latest_release(&state.client).await
}

/// The project's website, where the downloads and the extension list link are published.
#[tauri::command]
pub fn get_site_url() -> String {
    SITE_URL.trim_end_matches('/').to_string()
}
