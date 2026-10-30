use crate::db;
use crate::downloads::{self, DownloadSource, StorageCheck};
use crate::models::DownloadRecord;
use crate::state::AppState;
use std::collections::HashMap;
use std::path::Path;
use tauri::State;

/// Adds a `QUEUED` row. The queue worker resolves its sources and downloads it.
#[tauri::command]
pub async fn create_download(
    state: State<'_, AppState>,
    mut record: DownloadRecord,
) -> Result<String, String> {
    record.status = db::STATUS_QUEUED.to_string();
    record.progress = 0.0;
    record.file_path = None;
    record.file_size = None;
    record.error_msg = None;

    let id = db::create_download(&state.pool, &record)
        .await
        .map_err(|e| e.to_string())?;
    downloads::notify_queue();
    Ok(id)
}

/// Downloads › Try again: queues a `FAILED` download again. Any other row is left alone, so
/// the page can never start a second job for a running row or mark a row finished.
#[tauri::command]
pub async fn retry_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    downloads::retry_download(&state.pool, &id).await.map(drop)
}

#[tauri::command]
pub async fn get_downloads(
    state: State<'_, AppState>,
    anilist_id: i64,
) -> Result<Vec<DownloadRecord>, String> {
    db::get_downloads(&state.pool, anilist_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_all_downloads(
    state: State<'_, AppState>,
) -> Result<Vec<DownloadRecord>, String> {
    db::get_all_downloads(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

/// Stops the download if it is running (ffmpeg included), removes its partial files and
/// deletes the row. A finished file is left on disk unless `delete_file` is set; then the
/// video and its subtitle side files go first, and the row stays if that fails.
#[tauri::command]
pub async fn delete_download(
    state: State<'_, AppState>,
    id: String,
    delete_file: Option<bool>,
) -> Result<(), String> {
    downloads::cancel_download(&id).await;
    let record = db::get_download_by_id(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?;
    if delete_file.unwrap_or(false) {
        if let Some(path) = record
            .as_ref()
            .filter(|r| r.status.eq_ignore_ascii_case("COMPLETED"))
            .and_then(|r| r.file_path.clone())
        {
            tokio::task::spawn_blocking(move || downloads::delete_finished_files(Path::new(&path)))
                .await
                .map_err(|e| e.to_string())??;
        }
    }
    db::delete_download(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?;
    // The queue worker may have picked the row up between the two steps above.
    downloads::cancel_download(&id).await;
    // What a failed try kept for Try again is not wanted any more. Leftovers in a folder the
    // user has since moved away from are cleaned up at the next start.
    if let Some(record) = record {
        let removed = tokio::task::spawn_blocking(move || {
            downloads::get_download_dir().map(|dir| downloads::remove_resume_files(&dir, &record))
        })
        .await;
        if let Ok(Err(e)) = removed {
            tracing::warn!("Could not remove the kept files of download {}: {}", id, e);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_download_by_id(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<DownloadRecord>, String> {
    db::get_download_by_id(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_storage_for_download(quality: String) -> Result<StorageCheck, String> {
    tokio::task::spawn_blocking(move || downloads::check_storage(&quality))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_download_directory() -> Result<String, String> {
    let dir = tokio::task::spawn_blocking(downloads::get_download_dir)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().to_string())
}

/// Asks for a new download folder, starting in the current one. Returns the folder, which the
/// caller saves as `download_dir`, or `None` when the dialog was cancelled. A folder Anix
/// cannot write to is refused with the reason.
#[tauri::command]
pub async fn pick_download_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let title = crate::i18n::text(crate::i18n::current(), "picker_download_folder");
        let mut dialog = app.dialog().file().set_title(title);
        if let Ok(current) = downloads::get_download_dir() {
            dialog = dialog.set_directory(current);
        }
        dialog.blocking_pick_folder().and_then(|p| p.into_path().ok())
    })
    .await
    .map_err(|e| format!("The folder picker failed: {}", e))?;
    let Some(dir) = picked else { return Ok(None) };
    let checked = dir.clone();
    tokio::task::spawn_blocking(move || downloads::check_writable_dir(&checked))
        .await
        .map_err(|e| e.to_string())??;
    Ok(Some(dir.to_string_lossy().into_owned()))
}

/// Downloads a stream the player has already resolved. `headers` and `stream_type` are the
/// stream's own fields and may be omitted.
#[tauri::command]
pub async fn start_episode_download(
    state: State<'_, AppState>,
    mut record: DownloadRecord,
    stream_url: String,
    referer: Option<String>,
    origin: Option<String>,
    subtitles: Vec<crate::models::SubtitleItem>,
    headers: Option<HashMap<String, String>>,
    stream_type: Option<String>,
) -> Result<String, String> {
    let source = DownloadSource {
        url: downloads::validate_stream_url(&stream_url)?,
        stream_type,
        referer,
        origin,
        headers: headers.unwrap_or_default(),
        subtitles,
        language: None,
    };

    // Inserted as DOWNLOADING so the queue worker does not start it a second time.
    record.status = db::STATUS_DOWNLOADING.to_string();
    record.progress = 0.0;
    record.file_path = None;
    record.file_size = None;
    record.error_msg = None;

    let download_id = db::create_download(&state.pool, &record)
        .await
        .map_err(|e| e.to_string())?;

    downloads::start_download(state.pool.clone(), record, source);

    Ok(download_id)
}
