//! Covers and banners kept on disk (see `image_cache`).

use crate::image_cache;
use base64::Engine;

fn settings() -> serde_json::Value {
    crate::db::get_config_dir()
        .ok()
        .and_then(|dir| crate::commands::settings::read_settings_file(&dir.join("settings.json")).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn is_remote_image(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some())
}

/// Downloads the picture at an https address into the image folder; a failure only means it is
/// not saved.
#[tauri::command]
pub async fn save_image(url: String) -> Result<(), String> {
    let (Some(dir), true) = (image_cache::dir(), is_remote_image(&url)) else {
        return Ok(());
    };
    if image_cache::find(&dir, &url).is_some() {
        return Ok(());
    }
    let Some(bytes) = image_cache::fetch_public(&url, image_cache::MAX_IMAGE_BYTES).await? else {
        return Ok(());
    };
    image_cache::save(&dir, &url, &bytes, image_cache::limit_bytes(&settings())).map_err(|e| e.to_string())?;
    Ok(())
}

/// The saved picture at `url` as a data address the page can show, or none.
#[tauri::command]
pub async fn saved_image(url: String) -> Result<Option<String>, String> {
    let Some(path) = image_cache::dir().and_then(|dir| image_cache::find(&dir, &url)) else {
        return Ok(None);
    };
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(None);
    };
    let mime = match bytes.as_slice() {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', ..] => "image/webp",
        _ => "image/jpeg",
    };
    Ok(Some(format!("data:{};base64,{}", mime, base64::engine::general_purpose::STANDARD.encode(bytes))))
}

/// Bytes the saved pictures use, for Settings.
#[tauri::command]
pub async fn image_cache_size() -> Result<u64, String> {
    Ok(image_cache::dir().map_or(0, |dir| image_cache::used_bytes(&dir)))
}

/// Applies the limit now (the setting was lowered).
#[tauri::command]
pub async fn trim_image_cache() -> Result<(), String> {
    if let Some(dir) = image_cache::dir() {
        image_cache::prune(&dir, image_cache::limit_bytes(&settings())).map_err(|e| e.to_string())?;
    }
    Ok(())
}
