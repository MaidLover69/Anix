//! Commands of "Your files": linking a folder or files to a title, and handing the built-in
//! player a source for one of its episodes.

use crate::commands::local_scanner::{
    parse_episode_and_title, resolve_local_media_file, scan_local_anime_directory, LocalAnimeFile,
};
use crate::local_media::{
    self, find_sidecars, is_video_file, register_playable, sidecar_label, LocalMediaLink,
    PlayableKind, LOCAL_PROVIDER,
};
use crate::models::{SourcesResult, StreamItem, SubtitleItem};
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

/// What the player needs to play a local episode.
#[derive(Debug, Clone, Serialize)]
pub struct LocalEpisodeSource {
    pub sources: SourcesResult,
    /// The file itself, for opening it in an external player.
    pub file_path: String,
}

#[tauri::command]
pub async fn get_local_media(
    state: State<'_, AppState>,
    anilist_id: i32,
) -> Result<LocalMediaLink, String> {
    local_media::get_link(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

/// Opens the system's folder picker (`files: false`) or file picker. Returns the chosen
/// paths; empty when the user cancelled.
#[tauri::command]
pub async fn pick_local_media(app: AppHandle, files: bool) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file();
        let picked = if files {
            dialog
                .set_title("Choose episode files")
                .add_filter("Video", &local_media::VIDEO_EXTENSIONS)
                .blocking_pick_files()
                .unwrap_or_default()
        } else {
            dialog
                .set_title("Choose the folder with the episodes")
                .blocking_pick_folder()
                .into_iter()
                .collect()
        };
        picked
            .into_iter()
            .filter_map(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    })
    .await
    .map_err(|e| format!("The file picker failed: {}", e))
}

/// Links `folder_path` to the title (replacing an earlier folder) and scans it.
#[tauri::command]
pub async fn link_local_folder(
    state: State<'_, AppState>,
    anilist_id: i32,
    folder_path: String,
) -> Result<LocalMediaLink, String> {
    let absolute = std::path::absolute(folder_path.trim())
        .map_err(|_| format!("Folder does not exist: {}", folder_path))?;
    let folder = absolute.to_string_lossy().to_string();
    let scanned = scan_local_anime_directory(folder.clone()).await?;
    local_media::save_scan(&state.pool, anilist_id, Some(&folder), &scanned)
        .await
        .map_err(|e| e.to_string())?;
    local_media::get_link(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

/// Scans the linked folder again: new files are added, removed ones dropped, numbers the
/// user set are kept.
#[tauri::command]
pub async fn rescan_local_media(
    state: State<'_, AppState>,
    anilist_id: i32,
) -> Result<LocalMediaLink, String> {
    let folder = local_media::get_folder(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())?
        .flatten();
    if let Some(folder) = folder {
        let scanned = scan_local_anime_directory(folder.clone()).await?;
        local_media::save_scan(&state.pool, anilist_id, Some(&folder), &scanned)
            .await
            .map_err(|e| e.to_string())?;
    }
    local_media::get_link(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

/// Links single video files to the title.
#[tauri::command]
pub async fn add_local_files(
    state: State<'_, AppState>,
    anilist_id: i32,
    file_paths: Vec<String>,
) -> Result<LocalMediaLink, String> {
    let mut files: Vec<LocalAnimeFile> = Vec::new();
    for raw in &file_paths {
        let path = resolve_local_media_file(raw)?;
        if !is_video_file(&path) {
            return Err(format!("Not a video file: {}", raw));
        }
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let (parsed_title, parsed_episode) = parse_episode_and_title(&file_name);
        files.push(LocalAnimeFile {
            file_size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            file_path: path.to_string_lossy().to_string(),
            file_name,
            parsed_title,
            parsed_episode,
        });
    }
    local_media::save_scan(&state.pool, anilist_id, None, &files)
        .await
        .map_err(|e| e.to_string())?;
    local_media::get_link(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

/// Sets the episode a linked file plays as; `None` leaves the file unused.
#[tauri::command]
pub async fn set_local_file_episode(
    state: State<'_, AppState>,
    anilist_id: i32,
    file_id: i64,
    episode_number: Option<f64>,
) -> Result<LocalMediaLink, String> {
    if !local_media::set_file_episode(&state.pool, anilist_id, file_id, episode_number)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err("That file is not linked to this title".to_string());
    }
    local_media::get_link(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_local_file(
    state: State<'_, AppState>,
    anilist_id: i32,
    file_id: i64,
) -> Result<LocalMediaLink, String> {
    local_media::remove_file(&state.pool, anilist_id, file_id)
        .await
        .map_err(|e| e.to_string())?;
    local_media::get_link(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unlink_local_media(
    state: State<'_, AppState>,
    anilist_id: i32,
) -> Result<LocalMediaLink, String> {
    local_media::unlink(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())?;
    local_media::get_link(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

/// The file of one episode as player sources: the video and its sidecar subtitles, served by
/// the stream proxy. `None` when the title has no file for that episode.
#[tauri::command]
pub async fn get_local_episode_source(
    state: State<'_, AppState>,
    anilist_id: i32,
    episode_number: f64,
) -> Result<Option<LocalEpisodeSource>, String> {
    if !episode_number.is_finite() {
        return Err("Invalid episode number".to_string());
    }
    let Some(path) = local_media::episode_file(&state.pool, anilist_id, episode_number)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let base = crate::stream_proxy::start()?;
    Ok(Some(local_episode_source(&base, &path)))
}

/// Registers `video` and its sidecars with the proxy and describes them as sources.
pub fn local_episode_source(proxy_base: &str, video: &std::path::Path) -> LocalEpisodeSource {
    let key = register_playable(PlayableKind::Video, video);
    let stream_type = video
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "mp4".to_string());
    let stream = StreamItem::new(
        format!("{}/local/{}", proxy_base, key),
        stream_type,
        Some("Original".to_string()),
        None,
        None,
        true,
    );

    let subtitles = find_sidecars(video)
        .into_iter()
        .map(|subtitle| SubtitleItem {
            url: format!(
                "{}/localsub/{}",
                proxy_base,
                register_playable(PlayableKind::Subtitle, &subtitle)
            ),
            language: sidecar_label(video, &subtitle),
            format: subtitle
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase(),
            is_default: None,
        })
        .collect();

    let mut sources = SourcesResult::new(vec![stream]);
    sources.subtitles = subtitles;
    sources.provider = Some(LOCAL_PROVIDER.to_string());
    LocalEpisodeSource {
        sources,
        file_path: video.to_string_lossy().to_string(),
    }
}
