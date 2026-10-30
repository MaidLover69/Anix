use crate::state::AppState;
use crate::system::{self, hwaccel, DesktopStatus};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

/// Tray and autostart state for Settings › System.
#[tauri::command]
pub async fn get_desktop_status(app: AppHandle) -> Result<DesktopStatus, String> {
    tokio::task::spawn_blocking(move || system::desktop_status(&app))
        .await
        .map_err(|e| e.to_string())
}

/// The hardware-acceleration diagnostics. Read-only; runs `vainfo` when it is installed.
#[tauri::command]
pub async fn get_hardware_report() -> Result<hwaccel::HardwareReport, String> {
    Ok(hwaccel::report().await)
}

/// Asks where to save the diagnostics report and writes it there. `None` when the dialog
/// was cancelled.
#[tauri::command]
pub async fn save_diagnostics_report(app: AppHandle, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let report = crate::diagnostics::build_report(&state.pool).await;
    let name = format!("anix-report-{}.txt", crate::db::current_unix_timestamp());
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(crate::i18n::text(crate::i18n::current(), "picker_save_report"))
            .set_file_name(&name)
            .add_filter(crate::i18n::text(crate::i18n::current(), "picker_text_files"), &["txt"])
            .blocking_save_file()
            .and_then(|p| p.into_path().ok())
    })
    .await
    .map_err(|e| format!("The file picker failed: {}", e))?;
    let Some(path) = picked else { return Ok(None) };
    std::fs::write(&path, report).map_err(|e| format!("Could not write {}: {}", path.display(), e))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Opens the folder that holds the log file (the app's data folder) in the file manager.
#[tauri::command]
pub async fn open_log_folder() -> Result<(), String> {
    let dir = crate::db::get_app_dir().map_err(|e| e.to_string())?;
    crate::commands::player::spawn_reaped(
        crate::commands::player::sanitized_command("xdg-open")
            .arg(&dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()),
    )
    .map(drop)
    .map_err(|e| format!("Could not open {}: {}", dir.display(), e))
}
