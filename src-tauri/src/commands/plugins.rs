use crate::plugins::health::{self, DirectoryList, PluginHealth};
use crate::plugins::registry::{PluginConfig, PluginInfo, PluginRepo, PluginUpdate};
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

/// Runs a quick test search through one plugin; nothing is played.
#[tauri::command]
pub async fn test_plugin(state: State<'_, AppState>, id: String) -> Result<PluginHealth, String> {
    Ok(health::test_plugin(&state.plugins, &id).await)
}

/// The test search through every enabled plugin at once.
#[tauri::command]
pub async fn test_all_plugins(state: State<'_, AppState>) -> Result<Vec<PluginHealth>, String> {
    let ids: Vec<String> = state
        .plugins
        .list_plugins()
        .into_iter()
        .filter(|p| p.enabled)
        .map(|p| p.id)
        .collect();
    let mut tasks = tokio::task::JoinSet::new();
    for (order, id) in ids.into_iter().enumerate() {
        let registry = state.plugins.clone();
        tasks.spawn(async move { (order, health::test_plugin(&registry, &id).await) });
    }
    let mut results = Vec::new();
    while let Some(done) = tasks.join_next().await {
        if let Ok(result) = done {
            results.push(result);
        }
    }
    results.sort_by_key(|(order, _)| *order);
    Ok(results.into_iter().map(|(_, health)| health).collect())
}

/// Installed plugins whose repository lists another version; nothing is installed.
#[tauri::command]
pub async fn check_plugin_updates(state: State<'_, AppState>) -> Result<Vec<PluginUpdate>, String> {
    Ok(state.plugins.available_updates().await)
}

/// The extension lists the project site recommends; empty when it publishes none.
#[tauri::command]
pub async fn get_plugin_directory(state: State<'_, AppState>) -> Result<Vec<DirectoryList>, String> {
    Ok(health::fetch_directory(&state.client, crate::clients::update::SITE_URL).await)
}
