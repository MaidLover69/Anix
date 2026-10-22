use crate::backup;
use crate::state::AppState;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

fn settings_path() -> Result<PathBuf, String> {
    Ok(crate::db::get_config_dir().map_err(|e| e.to_string())?.join("settings.json"))
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupResult {
    pub path: String,
    pub summary: backup::Summary,
}

/// Asks where to save, then writes the backup there (gzip when the name ends in `.gz`).
/// `None` when the save dialog was cancelled. Logins go in only when `passphrase` is given.
#[tauri::command]
pub async fn export_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    search_history: Value,
    passphrase: Option<String>,
) -> Result<Option<BackupResult>, String> {
    let name = format!("anix-backup-{}.json.gz", today_utc());
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Save backup")
            .set_file_name(&name)
            .add_filter("Anix backup", &["gz", "json"])
            .blocking_save_file()
            .and_then(|p| p.into_path().ok())
    })
    .await
    .map_err(|e| format!("The file picker failed: {}", e))?;
    let Some(path) = picked else { return Ok(None) };

    let made = backup::create(&state.pool, &settings_path()?, search_history, passphrase.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let gzip = path.extension().is_some_and(|ext| ext == "gz");
    let bytes = backup::to_bytes(&made, gzip).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("Could not write {}: {}", path.display(), e))?;
    Ok(Some(BackupResult {
        path: path.to_string_lossy().to_string(),
        summary: backup::summary(&made),
    }))
}

/// Asks for a backup file and restores it. `None` when the dialog was cancelled.
#[tauri::command]
pub async fn import_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: Option<String>,
) -> Result<Option<BackupResult>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Restore a backup")
            .add_filter("Anix backup", &["gz", "json"])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok())
    })
    .await
    .map_err(|e| format!("The file picker failed: {}", e))?;
    let Some(path) = picked else { return Ok(None) };

    let bytes = std::fs::read(&path).map_err(|e| format!("Could not read {}: {}", path.display(), e))?;
    let parsed = backup::from_bytes(&bytes).map_err(|e| backup_message(&e))?;
    let summary = backup::restore(&state.pool, &settings_path()?, &parsed, passphrase.as_deref())
        .await
        .map_err(|e| backup_message(&e))?;
    Ok(Some(BackupResult {
        path: path.to_string_lossy().to_string(),
        summary,
    }))
}

/// The message itself, without the error kind in front.
fn backup_message(error: &crate::error::AppError) -> String {
    match error {
        crate::error::AppError::Other(message) | crate::error::AppError::Parse(message) => message.clone(),
        other => other.to_string(),
    }
}

/// Today's date as YYYY-MM-DD (UTC), for the suggested file name.
fn today_utc() -> String {
    let days = crate::db::current_unix_timestamp().div_euclid(86_400);
    // Civil date from a count of days since 1970-01-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{:04}-{:02}-{:02}", y, m, d)
}
