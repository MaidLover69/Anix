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

const APPIMAGE_SINGLE_VALUES: [&str; 13] = [
    "PYTHONHOME",
    "GI_TYPELIB_PATH",
    "GST_PTP_HELPER_1_0",
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

/// The web address to hand to the browser: only absolute `http`/`https` addresses, in the
/// form the `url` crate writes them, so nothing else ever reaches `xdg-open`.
pub fn external_url(url: &str) -> Result<String, String> {
    match url::Url::parse(url.trim()) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some() => {
            Ok(parsed.to_string())
        }
        _ => Err("Only web addresses (http or https) can be opened in the browser.".to_string()),
    }
}

/// Opens a web address in the system browser with `xdg-open`. Started through
/// [`sanitized_command`]: a browser launched from the AppImage would otherwise get the bundle's
/// GTK, GIO and GStreamer paths and may fail to start or never hand a sign-in back.
pub fn open_in_browser(url: &str) -> Result<(), String> {
    let url = external_url(url)?;
    let mut child = sanitized_command("xdg-open")
        .arg(&url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not open the browser: {}", e))?;
    // Reap it; xdg-open hands the address to the browser and exits.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// The UI's "open in browser" links (see `useOpenExternal.ts`).
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    open_in_browser(&url)
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

/// A subtitle of the stream, handed to the external player as a file.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExternalSubtitle {
    pub url: String,
    #[serde(default)]
    pub language: Option<String>,
}

const SUBTITLE_FILE_PREFIX: &str = "anix_sub_";
const SUBTITLE_FILE_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);
const MAX_EXTERNAL_SUBTITLES: usize = 8;

/// Player options that load `files` as subtitle tracks. VLC takes one file; mpv takes
/// them all and, with `selected` false, starts with subtitles off.
pub fn subtitle_player_args(player_bin: &str, files: &[std::path::PathBuf], selected: bool) -> Vec<std::ffi::OsString> {
    let mut args = Vec::new();
    if player_bin == "vlc" {
        if let Some(first) = files.first().filter(|_| selected) {
            let mut arg = std::ffi::OsString::from("--sub-file=");
            arg.push(first.as_os_str());
            args.push(arg);
        }
    } else {
        for file in files {
            let mut arg = std::ffi::OsString::from("--sub-file=");
            arg.push(file.as_os_str());
            args.push(arg);
        }
        if !files.is_empty() {
            args.push(if selected { "--sid=1" } else { "--sid=no" }.into());
        }
    }
    args
}

/// A file name for one downloaded subtitle; mpv shows the language part as the track name.
pub fn subtitle_file_name(nonce: u128, index: usize, language: Option<&str>, extension: &str) -> String {
    let language: String = language
        .unwrap_or("")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(32)
        .collect();
    let language = if language.is_empty() { "subtitle".to_string() } else { language };
    format!("{}{}_{}_{}.{}", SUBTITLE_FILE_PREFIX, nonce, index + 1, language, extension)
}

fn remove_stale_subtitle_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let ours = entry.file_name().to_str().is_some_and(|n| n.starts_with(SUBTITLE_FILE_PREFIX));
        let old = entry
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age >= SUBTITLE_FILE_MAX_AGE);
        if ours && old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Downloads the stream's subtitles to temporary files, in the given order. A subtitle
/// that cannot be fetched or is not a subtitle file is left out.
async fn fetch_subtitle_files(subtitles: &[ExternalSubtitle], referer: Option<&str>) -> Vec<std::path::PathBuf> {
    let dir = crate::commands::playlists::playlist_dir();
    remove_stale_subtitle_files(&dir);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let client = crate::clients::http::download_client();

    let mut tasks = tokio::task::JoinSet::new();
    for (index, sub) in subtitles.iter().take(MAX_EXTERNAL_SUBTITLES).enumerate() {
        let (client, dir, sub, referer) = (client.clone(), dir.clone(), sub.clone(), referer.map(str::to_string));
        tasks.spawn(async move {
            let url = validate_media_url(&sub.url).ok()?;
            let mut request = client.get(&url).timeout(std::time::Duration::from_secs(20));
            if let Some(r) = referer {
                request = request.header("Referer", r);
            }
            let body = request.send().await.ok()?.error_for_status().ok()?.bytes().await.ok()?;
            let format = crate::downloads::detect_subtitle_format(&body)?;
            let path = dir.join(subtitle_file_name(nonce, index, sub.language.as_deref(), format.extension()));
            tokio::fs::write(&path, &body).await.ok()?;
            Some((index, path))
        });
    }
    let mut files: Vec<(usize, std::path::PathBuf)> = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Ok(Some(file)) = result {
            files.push(file);
        }
    }
    files.sort_by_key(|(index, _)| *index);
    files.into_iter().map(|(_, path)| path).collect()
}

/// Seconds mpv starts at: a resume position, never a negative or non-finite one.
pub fn start_arg(start_seconds: Option<f64>) -> Option<String> {
    start_seconds
        .filter(|s| s.is_finite() && *s >= 1.0)
        .map(|s| format!("--start={:.3}", s))
}

/// Plays a stream in mpv or VLC. With `episode`, mpv's position is synced back into the
/// watch history (see `mpv_ipc`).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn launch_external_player(
    app: tauri::AppHandle,
    player: String,
    url: String,
    title: Option<String>,
    referer: Option<String>,
    subtitles: Option<Vec<ExternalSubtitle>>,
    subtitles_on: Option<bool>,
    episode: Option<crate::mpv_ipc::EpisodeTarget>,
    start_seconds: Option<f64>,
) -> Result<bool, String> {
    let sync = episode.map(|episode| (app, episode));
    launch_stream_player(player, url, title, referer, subtitles, subtitles_on, sync, start_seconds).await
}

#[allow(clippy::too_many_arguments)]
pub async fn launch_stream_player(
    player: String,
    url: String,
    title: Option<String>,
    referer: Option<String>,
    subtitles: Option<Vec<ExternalSubtitle>>,
    subtitles_on: Option<bool>,
    sync: Option<(tauri::AppHandle, crate::mpv_ipc::EpisodeTarget)>,
    start_seconds: Option<f64>,
) -> Result<bool, String> {
    let player_bin = resolve_player_bin(Some(&player))?;
    let args = stream_player_args(player_bin, &url, title.as_deref(), referer.as_deref())?;
    let referer = referer.as_deref().and_then(|r| validate_media_url(r).ok());
    let files = fetch_subtitle_files(subtitles.as_deref().unwrap_or_default(), referer.as_deref()).await;
    if subtitles.as_ref().is_some_and(|s| !s.is_empty()) && files.is_empty() {
        tracing::warn!("None of the stream's subtitles could be fetched for the external player");
    }

    let mut cmd = sanitized_command(player_bin);
    cmd.args(subtitle_player_args(player_bin, &files, subtitles_on.unwrap_or(true)));

    // VLC is started and left alone; mpv reports its position back over its IPC socket.
    let synced = player_bin == "mpv";
    let socket = synced.then(|| {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        crate::mpv_ipc::socket_path(nonce)
    });
    if let Some(socket) = &socket {
        let mut arg = std::ffi::OsString::from("--input-ipc-server=");
        arg.push(socket.as_os_str());
        cmd.arg(arg);
        if let Some(start) = start_arg(start_seconds) {
            cmd.arg(start);
        }
    }
    cmd.args(args);

    let (Some(socket), Some((app, episode))) = (socket, sync) else {
        return match cmd.spawn() {
            Ok(_) => Ok(true),
            Err(e) => Err(format!("Failed to spawn {}: {}", player_bin, e)),
        };
    };
    let child = tokio::process::Command::from(cmd)
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", player_bin, e))?;
    tauri::async_runtime::spawn(crate::mpv_ipc::follow(app, child, socket, episode));
    Ok(true)
}

/// Subtitle files the player accepts when one is dropped on it.
const DROPPED_SUBTITLE_EXTENSIONS: [&str; 4] = ["srt", "ass", "ssa", "vtt"];
const DROPPED_SUBTITLE_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Reads a subtitle file dropped on the player and returns it as WebVTT.
#[tauri::command]
pub async fn read_subtitle_file(path: String) -> Result<String, String> {
    let path = std::path::PathBuf::from(path);
    if !path.is_absolute() {
        return Err("Not a file path".to_string());
    }
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !DROPPED_SUBTITLE_EXTENSIONS.contains(&extension.as_str()) {
        return Err("Only .srt, .ass, .ssa and .vtt files can be used as subtitles".to_string());
    }
    let metadata = tokio::fs::metadata(&path).await.map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > DROPPED_SUBTITLE_MAX_BYTES {
        return Err("That file is not a subtitle file".to_string());
    }
    let bytes = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => encoding_rs::WINDOWS_1252.decode(e.as_bytes()).0.into_owned(),
    };
    let vtt = crate::stream_proxy::subtitle_to_vtt(&text);
    if !vtt.contains("-->") {
        return Err("No subtitle lines were found in that file".to_string());
    }
    Ok(vtt)
}
