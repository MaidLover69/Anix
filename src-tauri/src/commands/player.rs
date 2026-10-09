use std::path::Path;
use std::process::Command;

/// Environment variables the AppImage launcher and its GTK hook point at the bundle.
/// A system mpv/vlc/ffmpeg started with them loads the bundle's libraries and plugins.
const APPIMAGE_PATH_LISTS: [&str; 10] = [
    "PATH",
    "LD_LIBRARY_PATH",
    "XDG_DATA_DIRS",
    "PYTHONPATH",
    "PERLLIB",
    "QT_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GSETTINGS_SCHEMA_DIR",
    "GTK_PATH",
];

const APPIMAGE_SINGLE_VALUES: [&str; 11] = [
    "PYTHONHOME",
    "GIO_EXTRA_MODULES",
    "GTK_EXE_PREFIX",
    "GTK_DATA_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
    "GDK_BACKEND",
    "GTK_THEME",
    "GST_PLUGIN_PATH_1_0",
    "GST_PLUGIN_SCANNER_1_0",
    "GST_REGISTRY_REUSE_PLUGIN_SCANNER",
];

/// Drops the entries of a `:`-separated list that live inside `appdir`.
pub fn strip_appdir_entries(value: &str, appdir: &str) -> String {
    let appdir = appdir.trim_end_matches('/');
    value
        .split(':')
        .filter(|entry| {
            !entry.is_empty() && *entry != appdir && !entry.starts_with(&format!("{}/", appdir))
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// A `Command` for a host program that does not inherit the AppImage's bundle environment.
pub fn sanitized_command(program: &str) -> Command {
    let mut cmd = Command::new(program);

    if let Ok(appdir) = std::env::var("APPDIR") {
        if !appdir.is_empty() {
            for name in APPIMAGE_PATH_LISTS {
                if let Ok(value) = std::env::var(name) {
                    let cleaned = strip_appdir_entries(&value, &appdir);
                    if cleaned.is_empty() {
                        cmd.env_remove(name);
                    } else {
                        cmd.env(name, cleaned);
                    }
                }
            }
            for name in APPIMAGE_SINGLE_VALUES {
                cmd.env_remove(name);
            }
        }
    }

    // The AppImage launcher changes into the read-only bundle; start children from home.
    if let Some(home) = dirs::home_dir() {
        if home.is_dir() {
            cmd.current_dir(home);
        }
    }

    cmd
}

/// Maps the player setting to a fixed binary name. The caller's string never becomes the
/// program that is executed; `builtin` has no external binary, so it falls back to mpv.
pub fn resolve_player_bin(player: Option<&str>) -> Result<&'static str, String> {
    match player.map(|p| p.to_lowercase()).as_deref() {
        None | Some("") | Some("mpv") | Some("builtin") => Ok("mpv"),
        Some("vlc") => Ok("vlc"),
        Some(other) => Err(format!("Unsupported player: {}", other)),
    }
}

/// Accepts only absolute http(s) URLs, so a value can never be read as a player option,
/// a local file, or another protocol handler.
pub fn validate_media_url(raw: &str) -> Result<String, String> {
    let parsed = url::Url::parse(raw.trim()).map_err(|e| format!("Invalid media URL: {}", e))?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(parsed.to_string()),
        scheme => Err(format!("Unsupported media URL scheme: {}", scheme)),
    }
}

/// Removes control characters so a title cannot inject extra lines or arguments.
pub fn single_line(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Arguments for playing one remote stream in `player_bin`.
pub fn stream_player_args(
    player_bin: &str,
    url: &str,
    title: Option<&str>,
    referer: Option<&str>,
) -> Result<Vec<String>, String> {
    let url = validate_media_url(url)?;
    let referer = referer.and_then(|r| validate_media_url(r).ok());
    let title = title.map(single_line).filter(|t| !t.is_empty());

    let mut args = Vec::new();
    if player_bin == "vlc" {
        if let Some(t) = title {
            args.push(format!("--meta-title={}", t));
        }
        if let Some(r) = referer {
            args.push(format!("--http-referrer={}", r));
        }
        args.push(url);
    } else {
        if let Some(t) = title {
            args.push(format!("--title={}", t));
        }
        if let Some(r) = referer {
            args.push(format!("--referrer={}", r));
        }
        args.push("--".to_string());
        args.push(url);
    }
    Ok(args)
}

/// Arguments for playing one local file in `player_bin`. `file` must be an absolute path.
pub fn local_file_player_args(player_bin: &str, file: &Path) -> Vec<std::ffi::OsString> {
    let mut args = Vec::new();
    if player_bin != "vlc" {
        args.push("--".into());
    }
    args.push(file.as_os_str().to_os_string());
    args
}

#[tauri::command]
pub async fn launch_external_player(
    player: String,
    url: String,
    title: Option<String>,
    referer: Option<String>,
) -> Result<bool, String> {
    let player_bin = resolve_player_bin(Some(&player))?;
    let args = stream_player_args(player_bin, &url, title.as_deref(), referer.as_deref())?;

    let mut cmd = sanitized_command(player_bin);
    cmd.args(args);

    match cmd.spawn() {
        Ok(_) => Ok(true),
        Err(e) => Err(format!("Failed to spawn {}: {}", player_bin, e)),
    }
}
