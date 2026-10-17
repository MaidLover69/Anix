use crate::commands::player::{local_file_player_args, resolve_player_bin, sanitized_command};
use crate::release_parser::{file_stem, parse_file_name, ReleaseKind};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalAnimeFile {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub parsed_title: String,
    pub parsed_episode: Option<f64>,
}

/// The series title and episode number a file name suggests, read by the release parser.
/// Openings, endings and other extras have no episode; a name with no title keeps its stem.
pub fn parse_episode_and_title(file_name: &str) -> (String, Option<f64>) {
    let parsed = parse_file_name(file_name);
    let episode = if parsed.kind == ReleaseKind::Extra { None } else { parsed.episode };
    let title = if parsed.title.is_empty() { file_stem(file_name) } else { parsed.title };
    (title, episode)
}

fn scan_dir_recursive(path: &Path, depth: usize, max_depth: usize, results: &mut Vec<LocalAnimeFile>) {
    if depth > max_depth {
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan_dir_recursive(&p, depth + 1, max_depth, results);
        } else if p.is_file() {
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if matches!(ext_lower.as_str(), "mkv" | "mp4" | "webm" | "avi") {
                    let file_name = p.file_name().and_then(|f| f.to_str()).unwrap_or("").to_string();
                    let metadata = fs::metadata(&p).ok();
                    let file_size = metadata.map(|m| m.len()).unwrap_or(0);
                    let (parsed_title, parsed_episode) = parse_episode_and_title(&file_name);

                    results.push(LocalAnimeFile {
                        file_path: p.to_string_lossy().to_string(),
                        file_name,
                        file_size,
                        parsed_title,
                        parsed_episode,
                    });
                }
            }
        }
    }
}

#[tauri::command]
pub async fn scan_local_anime_directory(dir_path: String) -> Result<Vec<LocalAnimeFile>, String> {
    // Absolute so every returned file path is absolute, whatever the user typed. Symlinks are
    // kept as they are: players look for subtitles next to the name the library shows.
    let path = std::path::absolute(&dir_path)
        .map_err(|_| format!("Directory does not exist: {}", dir_path))?;
    if !path.is_dir() {
        return Err(format!("Directory does not exist: {}", dir_path));
    }

    tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        scan_dir_recursive(&path, 0, 4, &mut files);
        files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        files
    })
    .await
    .map_err(|e| format!("Directory scan failed: {}", e))
}

/// Resolves a path from the frontend to an absolute path of a regular file, so it reaches the
/// player as a file operand (it starts with `/`) and can never be read as a command-line
/// option. Symlink names are kept; the target must be a regular file.
pub fn resolve_local_media_file(file_path: &str) -> Result<PathBuf, String> {
    if file_path.is_empty() {
        return Err("No file was given".to_string());
    }
    let absolute = std::path::absolute(file_path)
        .map_err(|_| format!("File does not exist: {}", file_path))?;
    if !absolute.exists() {
        return Err(format!("File does not exist: {}", file_path));
    }
    if !absolute.is_file() {
        return Err(format!("Not a regular file: {}", file_path));
    }
    Ok(absolute)
}

#[tauri::command]
pub async fn open_file_in_player(file_path: String, player: Option<String>) -> Result<(), String> {
    let player_bin = resolve_player_bin(player.as_deref())?;
    let file = resolve_local_media_file(&file_path)?;

    let mut cmd = sanitized_command(player_bin);
    cmd.args(local_file_player_args(player_bin, &file));

    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to launch {}: {}", player_bin, e)),
    }
}
