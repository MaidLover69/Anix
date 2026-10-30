//! Desktop integration: the tray and background mode, the autostart entry, the windowless
//! `--background` release checks, the AppImage's `anix://` handler, restarts, and the
//! hardware-acceleration diagnostics of Settings › System.

pub mod autostart;
pub mod background;
pub mod gnutls;
pub mod hwaccel;
pub mod page_crash;
pub mod tray;
pub mod url_handler;

use serde::Serialize;
use serde_json::Value;
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};

/// Passed by the autostart entry: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";
/// Runs only the release checks, without a window (see [`background`]).
pub const BACKGROUND_ARG: &str = "--background";

/// The launch arguments a restart passes on: the program and harmless flags such as
/// `--web-player`. Dropped: `--minimized` (the restart is meant to show the window),
/// `--background`, and `anix://` / `anilili://` links (the link was already opened; replaying
/// it would compete with the route the restart reopens).
pub fn restart_args(args: &[OsString]) -> Vec<OsString> {
    let dropped = |arg: &OsString| {
        let arg = arg.to_string_lossy().to_ascii_lowercase();
        arg == MINIMIZED_ARG || arg == BACKGROUND_ARG || arg.starts_with("anix:") || arg.starts_with("anilili:")
    };
    args.iter()
        .enumerate()
        .filter(|(index, arg)| *index == 0 || !dropped(arg))
        .map(|(_, arg)| arg.clone())
        .collect()
}

/// Set by [`restart`]; the app's `RunEvent::Exit` handler then calls [`restart_if_asked`].
static RESTART_ASKED: AtomicBool = AtomicBool::new(false);

/// Restarts Anix like `AppHandle::restart`, but with [`restart_args`] instead of the original
/// launch arguments. Off the main thread it asks the event loop to exit, so the exit handlers
/// run first; on the GTK main thread, which must not block, it restarts directly, as Tauri does.
pub fn restart(app: &AppHandle) -> ! {
    if gtk::is_initialized_main_thread() {
        app.cleanup_before_exit();
        tauri::process::restart(&clean_env(app));
    }
    RESTART_ASKED.store(true, Ordering::SeqCst);
    // Tauri's own restart (with the old arguments) is only the fallback when the exit request
    // fails; otherwise `restart_if_asked` restarts first and never returns.
    app.request_restart();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// Called from the app's `RunEvent::Exit` handler: finishes a [`restart`].
pub fn restart_if_asked(app: &AppHandle) {
    if RESTART_ASKED.load(Ordering::SeqCst) {
        app.cleanup_before_exit();
        tauri::process::restart(&clean_env(app));
    }
}

fn clean_env(app: &AppHandle) -> tauri::Env {
    let mut env = app.env();
    env.args_os = restart_args(&env.args_os);
    env
}

/// The `settings.json` keys this module acts on. All default to off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemSettings {
    /// "Keep running in the tray when the window is closed".
    pub minimize_to_tray: bool,
    /// "Start with the desktop session (minimised to tray)".
    pub start_with_desktop: bool,
    /// "Alerts while Anix is closed": `--background` is started at login.
    pub background_alerts: bool,
}

impl SystemSettings {
    pub fn from_json(json: &Value) -> Self {
        let flag = |key: &str| json.get(key).and_then(Value::as_bool).unwrap_or(false);
        Self {
            minimize_to_tray: flag("minimize_to_tray"),
            start_with_desktop: flag("start_with_desktop"),
            background_alerts: flag("background_alerts"),
        }
    }

    pub fn read() -> Self {
        crate::db::get_config_dir()
            .ok()
            .and_then(|dir| std::fs::read_to_string(dir.join("settings.json")).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map(|json| Self::from_json(&json))
            .unwrap_or_default()
    }
}

/// Applies the settings: creates or removes the tray icon and writes or removes the
/// autostart entry. The entry is only written while the tray is on, since it starts hidden.
pub fn apply(app: &tauri::AppHandle, settings: SystemSettings) {
    tray::set_enabled(app, settings.minimize_to_tray);
    let Some(config_home) = dirs::config_dir() else {
        return;
    };
    let exec = if settings.start_with_desktop && settings.minimize_to_tray {
        autostart::current_exec_target()
    } else {
        None
    };
    if let Err(e) = autostart::set_enabled(&config_home, exec.as_deref()) {
        tracing::warn!("The autostart entry could not be updated: {}", e);
    }
    let background = settings.background_alerts.then(autostart::current_exec_target).flatten();
    if let Err(e) = background::install(&config_home, background.as_deref()) {
        tracing::warn!("Alerts while Anix is closed could not be set up: {}", e);
    }
}

/// What Settings › System shows about the tray and autostart.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DesktopStatus {
    /// An AppIndicator library is installed, so a tray icon can be created.
    pub tray_supported: bool,
    /// The session shows tray icons right now.
    pub tray_host: bool,
    pub tray_active: bool,
    pub autostart_installed: bool,
    pub autostart_path: Option<String>,
    /// What the autostart entry starts.
    pub exec_target: Option<String>,
}

pub fn desktop_status(app: &tauri::AppHandle) -> DesktopStatus {
    let config_home = dirs::config_dir();
    DesktopStatus {
        tray_supported: tray::indicator_library().is_some(),
        tray_host: tray::tray_host_present(),
        tray_active: app.tray_by_id(tray::TRAY_ID).is_some(),
        autostart_installed: config_home.as_deref().map_or(false, autostart::is_installed),
        autostart_path: config_home
            .as_deref()
            .map(|dir| autostart::entry_path(dir).to_string_lossy().into_owned()),
        exec_target: autostart::current_exec_target().map(|p| p.to_string_lossy().into_owned()),
    }
}
