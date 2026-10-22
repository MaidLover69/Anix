use crate::clients::update::{self, fetch_latest_release, UpdateInfo, SITE_URL};
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub async fn check_for_update(state: State<'_, AppState>) -> Result<Option<UpdateInfo>, String> {
    fetch_latest_release(&state.client).await
}

/// The project's website, where the downloads and the extension list link are published.
#[tauri::command]
pub fn get_site_url() -> String {
    SITE_URL.trim_end_matches('/').to_string()
}

/// Whether the update dialog can offer Install, and why not when it cannot.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInstallSupport {
    pub appimage: bool,
    pub reason: Option<String>,
}

fn running_appimage() -> Result<std::path::PathBuf, String> {
    update::appimage_target(std::env::var_os("APPIMAGE").as_deref(), unsafe { libc::geteuid() })
}

#[tauri::command]
pub fn get_update_install_support() -> UpdateInstallSupport {
    match running_appimage() {
        Ok(_) => UpdateInstallSupport { appimage: true, reason: None },
        Err(reason) => UpdateInstallSupport { appimage: false, reason: Some(reason) },
    }
}

/// Sent while the update downloads: the fraction done.
pub const UPDATE_PROGRESS_EVENT: &str = "update-progress";

/// Downloads the AppImage the website lists for the latest release, checks its SHA-256 and
/// replaces the running AppImage, keeping the old one as `<name>.bak`. Runs only when the user
/// clicks Install. Returns the path of the AppImage to restart.
#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let target = running_appimage()?;
    // The address and checksum are read again here; the page cannot supply them.
    let info = fetch_latest_release(&state.client)
        .await?
        .ok_or("The website lists no release right now.")?;
    if !update::is_newer_version(&info.version, env!("CARGO_PKG_VERSION")) {
        return Err(format!("Anix {} is already the latest version.", env!("CARGO_PKG_VERSION")));
    }
    let asset = info
        .appimage
        .ok_or("This release has no AppImage for the in-app updater. Use the download page instead.")?;
    let mut last = 0.0;
    update::download_and_install(&state.client, &asset, &target, |fraction| {
        if fraction - last >= 0.01 || fraction >= 1.0 {
            last = fraction;
            let _ = app.emit(UPDATE_PROGRESS_EVENT, fraction);
        }
    })
    .await?;
    tracing::info!("Installed Anix {} into {}", info.version, target.display());
    Ok(target.to_string_lossy().into_owned())
}

/// Quits and starts the updated AppImage. The new process waits until this one is gone, so
/// the single-instance check does not hand it back to the old window.
#[tauri::command]
pub fn restart_after_update(app: AppHandle) -> Result<(), String> {
    let target = running_appimage()?;
    crate::commands::player::sanitized_command("sh")
        .args(["-c", "sleep 2; exec \"$0\""])
        .arg(&target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start the new version: {}", e))?;
    app.exit(0);
    Ok(())
}
