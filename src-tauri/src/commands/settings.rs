use crate::db;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

fn get_settings_path() -> Result<PathBuf, String> {
    let config_dir = db::get_config_dir().map_err(|e| e.to_string())?;
    Ok(config_dir.join("settings.json"))
}

/// Reads the settings object from `path`. A file that is not a JSON object is moved aside
/// (it may hold account tokens) instead of being silently replaced by the next save.
pub fn read_settings_file(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    restrict_to_owner(path);
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    match serde_json::from_str::<Value>(&content) {
        Ok(val) if val.is_object() => Ok(val),
        _ => {
            let backup = path.with_extension("json.corrupt");
            tracing::warn!(
                "settings file {} is not a JSON object; keeping a copy at {}",
                path.display(),
                backup.display()
            );
            let _ = fs::rename(path, &backup);
            Ok(serde_json::json!({}))
        }
    }
}

/// Drops group/other access from an existing settings file left by an older version.
fn restrict_to_owner(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.permissions().mode() & 0o077 != 0 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
    }
}

/// Writes settings atomically and readable by the owner only: the file holds the AniList token.
pub fn write_settings_file(path: &Path, settings: &Value) -> Result<(), String> {
    if !settings.is_object() {
        return Err("Settings must be a JSON object".to_string());
    }
    // A settings file symlinked into a dotfiles directory keeps being written through the link.
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = resolved.as_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;

    let tmp_path = path.with_extension(format!("json.tmp{}", std::process::id()));
    let _ = fs::remove_file(&tmp_path);
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp_path)?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp_path, path)
    })();

    if let Err(e) = result {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.to_string());
    }
    Ok(())
}

/// Settings for the frontend. Account tokens stay in the backend and are never returned.
#[tauri::command]
pub async fn get_settings() -> Result<Value, String> {
    let mut settings = {
        let _guard = crate::accounts::store::settings_lock();
        read_settings_file(&get_settings_path()?)?
    };
    crate::accounts::store::redact_secrets(&mut settings);
    Ok(settings)
}

#[tauri::command]
pub async fn save_settings(mut settings: Value) -> Result<(), String> {
    let path = get_settings_path()?;
    {
        // The frontend never sees the account keys, so carry the stored ones over.
        let _guard = crate::accounts::store::settings_lock();
        let stored = read_settings_file(&path)?;
        crate::accounts::store::keep_stored_account_keys(&stored, &mut settings);
        write_settings_file(&path, &settings)?;
    }
    // The release-notifications switch lives in settings; apply it right away.
    crate::notifier::replan();
    Ok(())
}
