use crate::plugins::registry::{PluginConfig, PluginInfo, PluginRepo};
use serde_json::Value;
use std::collections::HashMap;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn list_plugin_repos(state: State<'_, AppState>) -> Result<Vec<PluginRepo>, String> {
    Ok(state.plugins.list_repos())
}

/// Downloads the repository's index and every streaming plugin in it.
#[tauri::command]
pub async fn add_plugin_repo(state: State<'_, AppState>, url: String) -> Result<PluginRepo, String> {
    state.plugins.add_repo(&url).await
}

#[tauri::command]
pub async fn remove_plugin_repo(state: State<'_, AppState>, url: String) -> Result<(), String> {
    state.plugins.remove_repo(&url).await
}

#[tauri::command]
pub async fn refresh_plugin_repos(state: State<'_, AppState>) -> Result<Vec<PluginRepo>, String> {
    Ok(state.plugins.refresh_repos().await)
}

#[tauri::command]
pub async fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    Ok(state.plugins.list_plugins())
}

#[tauri::command]
pub async fn set_plugin_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> Result<(), String> {
    state.plugins.set_enabled(&id, enabled).await
}

/// The plugin's own settings (its manifest's `userConfig`) with the values in effect.
#[tauri::command]
pub async fn get_plugin_config(state: State<'_, AppState>, id: String) -> Result<PluginConfig, String> {
    state.plugins.plugin_config(&id)
}

/// Saves values for the plugin's settings and reloads the plugin with them.
#[tauri::command]
pub async fn set_plugin_config(
    state: State<'_, AppState>,
    id: String,
    values: HashMap<String, Value>,
) -> Result<(), String> {
    state.plugins.set_plugin_config(&id, &values).await
}
