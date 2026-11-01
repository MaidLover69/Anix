use crate::clients::update::{self, fetch_latest_release, UpdateInfo};
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// The site's latest release, with `appimage` set to the file this installation should use.
#[tauri::command]
pub async fn check_for_update(state: State<'_, AppState>) -> Result<Option<UpdateInfo>, String> {
    let info = fetch_latest_release(&state.client).await.map(update::offered_release)?;
    Ok(info.map(for_this_install))
}

fn for_this_install(mut info: UpdateInfo) -> UpdateInfo {
    let (modern, glibc) = (update::running_modern_build(), update::system_glibc());
    info.switch_build = update::offers_build_switch(&info, env!("CARGO_PKG_VERSION"), modern, glibc.as_deref());
    info.appimage = update::pick_appimage(&info, modern, glibc.as_deref()).cloned();
    info.appimage_modern = None;
    info
}

/// Whether the update dialog can offer Install, and why not when it cannot.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInstallSupport {
    pub appimage: bool,
    pub reason: Option<String>,
    /// How Anix was installed: `appimage`, `flatpak` or `package` (DEB, RPM, AUR), so the
    /// dialog can say how to update when Install is not offered.
    pub kind: &'static str,
}

/// How this copy of Anix was installed.
pub fn install_kind() -> &'static str {
    if crate::system::is_flatpak() {
        "flatpak"
    } else if std::env::var_os("APPIMAGE").is_some_and(|p| !p.is_empty()) {
        "appimage"
    } else {
        "package"
    }
}

fn running_appimage() -> Result<std::path::PathBuf, String> {
    update::appimage_target(std::env::var_os("APPIMAGE").as_deref(), unsafe { libc::geteuid() })
}

#[tauri::command]
pub fn get_update_install_support() -> UpdateInstallSupport {
    match running_appimage() {
        Ok(_) => UpdateInstallSupport { appimage: true, reason: None, kind: install_kind() },
        Err(reason) => UpdateInstallSupport { appimage: false, reason: Some(reason), kind: install_kind() },
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
    let latest = fetch_latest_release(&state.client).await?.ok_or("The website lists no release right now.")?;
    let info = update::offered_release(Some(latest.clone()))
        .map(for_this_install)
        .ok_or_else(|| format!("Anix {} has a known problem, so it is not installed.", latest.version))?;
    if !info.switch_build && !update::is_newer_version(&info.version, env!("CARGO_PKG_VERSION")) {
        return Err(format!("Anix {} is already the latest version.", env!("CARGO_PKG_VERSION")));
    }
    let asset = info
        .appimage
        .ok_or("This release has no AppImage for the in-app updater. Use the download page instead.")?;
    let mut last = 0.0;
    let backup = update::download_and_install(&state.client, &asset, &target, |fraction| {
        if fraction - last >= 0.01 || fraction >= 1.0 {
            last = fraction;
            let _ = app.emit(UPDATE_PROGRESS_EVENT, fraction);
        }
    })
    .await?;
    if let Ok(dir) = crate::db::get_app_dir() {
        update::record_update(&dir, env!("CARGO_PKG_VERSION"), &info.version, &backup);
    }
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

/// The version Anix can go back to after an update, when its backup is still there.
#[derive(Debug, Clone, Serialize)]
pub struct RollbackInfo {
    pub version: String,
}

#[tauri::command]
pub fn get_rollback() -> Option<RollbackInfo> {
    let dir = crate::db::get_app_dir().ok()?;
    let target = running_appimage().ok()?;
    update::rollback_available(&dir, env!("CARGO_PKG_VERSION"), &target).map(|r| RollbackInfo { version: r.from })
}

/// Puts the previous AppImage back; the page then calls [`restart_after_update`]. Runs only
/// when the user clicks the button.
#[tauri::command]
pub fn rollback_update() -> Result<String, String> {
    let dir = crate::db::get_app_dir().map_err(|e| e.to_string())?;
    let target = running_appimage()?;
    let record = update::roll_back(&dir, env!("CARGO_PKG_VERSION"), &target)?;
    tracing::warn!("Went back from Anix {} to {}", record.to, record.from);
    Ok(record.from)
}

/// At start: a notice when this version is on the site's list of versions with a known
/// problem, or, in safe mode, when the previous version can be put back.
pub fn check_version_at_start(app: &AppHandle) {
    use tauri::Manager;
    if crate::system::safe_mode::active() && get_rollback().is_some() {
        crate::notices::push_translated(app, crate::notices::SYSTEM, "rollback_offer_title", "rollback_offer_body", Some("/settings?section=system"));
    }
    let Some(state) = app.try_state::<AppState>() else { return };
    let client = state.client.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match fetch_latest_release(&client).await {
            Ok(Some(info)) if update::is_bad_version(&info, env!("CARGO_PKG_VERSION")) => {
                tracing::warn!("Anix {} is listed as a version with a known problem", env!("CARGO_PKG_VERSION"));
                let first = crate::db::get_app_dir().is_ok_and(|dir| update::first_bad_version_warning(&dir, env!("CARGO_PKG_VERSION")));
                if first {
                    // Only point at Settings › System when the Go back row is there.
                    let (body, link) = match get_rollback() {
                        Some(_) => ("bad_version_body", Some("/settings?section=system")),
                        None => ("bad_version_body_no_rollback", None),
                    };
                    crate::notices::push_translated(&app, crate::notices::SYSTEM, "bad_version_title", body, link);
                }
            }
            Ok(_) => {}
            // Offline or the site is down: the regular update check reports it.
            Err(e) => tracing::info!("Version check at start skipped: {}", e),
        }
    });
}
