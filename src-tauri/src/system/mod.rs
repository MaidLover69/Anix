//! Desktop integration: the tray and background mode, the autostart entry, the windowless
//! `--background` release checks, and the hardware-acceleration diagnostics of Settings › System.

pub mod autostart;
pub mod background;
pub mod gnutls;
pub mod hwaccel;
pub mod page_crash;
pub mod tray;

use serde::Serialize;
use serde_json::Value;

/// Passed by the autostart entry: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";
/// Runs only the release checks, without a window (see [`background`]).
pub const BACKGROUND_ARG: &str = "--background";

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
