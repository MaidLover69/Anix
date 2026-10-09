use crate::commands::player::{resolve_player_bin, sanitized_command, single_line, validate_media_url};
use crate::db;
use crate::models::{CustomPlaylist, CustomPlaylistItem, ExternalPlaylistItem};
use crate::state::AppState;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

#[tauri::command]
pub async fn create_playlist(
    name: String,
    description: Option<String>,
    state: State<'_, AppState>,
) -> Result<CustomPlaylist, String> {
    db::create_custom_playlist(&state.pool, &name, description.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_playlists(
    state: State<'_, AppState>,
) -> Result<Vec<CustomPlaylist>, String> {
    db::list_custom_playlists(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_playlist_items(
    playlist_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<CustomPlaylistItem>, String> {
    db::get_custom_playlist_items(&state.pool, &playlist_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_to_playlist(
    item: CustomPlaylistItem,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::add_item_to_custom_playlist(&state.pool, item)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_multiple_to_playlist(
    items: Vec<CustomPlaylistItem>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::add_items_to_custom_playlist(&state.pool, &items)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_from_playlist(
    item_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::remove_item_from_custom_playlist(&state.pool, &item_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_playlist(
    playlist_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::delete_custom_playlist(&state.pool, &playlist_id)
        .await
        .map_err(|e| e.to_string())
}

/// Renders the playlist file handed to an external player. Every URL must be http(s) and
/// titles are reduced to one line, so an item cannot add entries or player options.
pub fn build_external_playlist(items: &[ExternalPlaylistItem]) -> Result<String, String> {
    let mut out = String::from("#EXTM3U\n");
    for item in items {
        let url = validate_media_url(&item.url)?;
        out.push_str(&format!("#EXTINF:-1,{}\n", single_line(&item.title)));
        if let Some(referer) = item.referer.as_deref().and_then(|r| validate_media_url(r).ok()) {
            out.push_str(&format!("#EXTVLCOPT:http-referrer={}\n", referer));
        }
        out.push_str(&url);
        out.push('\n');
    }
    Ok(out)
}

/// The referer shared by every item, if there is exactly one. mpv ignores `#EXTVLCOPT`,
/// so a common referer is passed on its command line instead.
pub fn common_referer(items: &[ExternalPlaylistItem]) -> Option<String> {
    let mut referers = items
        .iter()
        .map(|i| i.referer.as_deref().and_then(|r| validate_media_url(r).ok()));
    let first = referers.next()??;
    referers.all(|r| r.as_deref() == Some(first.as_str())).then_some(first)
}

const PLAYLIST_FILE_PREFIX: &str = "anilili_playlist_";
const PLAYLIST_FILE_MAX_AGE: Duration = Duration::from_secs(12 * 60 * 60);

/// An absolute directory for playlist files: the player is started from another directory.
fn playlist_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute() && d.is_dir())
        .or_else(|| Some(std::env::temp_dir()).filter(|d| d.is_absolute() && d.is_dir()))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Removes playlist files from earlier launches; a player still reading one is long done.
pub fn remove_stale_playlist_files(dir: &std::path::Path, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let is_ours = name
            .to_str()
            .map(|n| n.starts_with(PLAYLIST_FILE_PREFIX) && (n.ends_with(".m3u") || n.ends_with(".m3u8")))
            .unwrap_or(false);
        let is_old = entry
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .and_then(|m| m.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .map(|age| age >= max_age)
            .unwrap_or(false);
        if is_ours && is_old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn playlist_file_path(playlist_name: &str) -> PathBuf {
    let sanitized_name: String = playlist_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(48)
        .collect();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = playlist_dir();
    remove_stale_playlist_files(&dir, PLAYLIST_FILE_MAX_AGE);
    dir.join(format!(
        "{}{}_{}_{}.m3u",
        PLAYLIST_FILE_PREFIX,
        sanitized_name,
        std::process::id(),
        nonce
    ))
}

#[tauri::command]
pub async fn launch_playlist_external_player(
    player: String,
    playlist_name: String,
    items: Vec<ExternalPlaylistItem>,
) -> Result<bool, String> {
    if items.is_empty() {
        return Err("Playlist has no items to play".to_string());
    }

    let player_bin = resolve_player_bin(Some(&player))?;
    let playlist = build_external_playlist(&items)?;
    let m3u_path = playlist_file_path(&playlist_name);

    // create_new + 0600: never follow or overwrite a file someone else placed at this path.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&m3u_path)
        .map_err(|e| format!("Failed to create playlist file: {}", e))?;
    file.write_all(playlist.as_bytes())
        .map_err(|e| format!("Failed to write playlist file: {}", e))?;

    let mut cmd = sanitized_command(player_bin);
    if player_bin == "mpv" {
        cmd.arg(format!("--playlist={}", m3u_path.display()));
        let title = single_line(&playlist_name);
        if !title.is_empty() {
            cmd.arg(format!("--title={}", title));
        }
        if let Some(referer) = common_referer(&items) {
            cmd.arg(format!("--referrer={}", referer));
        }
    } else {
        cmd.arg(&m3u_path);
    }

    match cmd.spawn() {
        Ok(_) => Ok(true),
        Err(e) => Err(format!("Failed to spawn {}: {}", player_bin, e)),
    }
}
