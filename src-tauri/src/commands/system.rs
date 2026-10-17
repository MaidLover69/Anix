use crate::system::{self, hwaccel, DesktopStatus};
use tauri::AppHandle;

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
