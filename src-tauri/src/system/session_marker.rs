//! Tells the next start that the last session ended without a clean exit (a crash, a kill, a
//! power cut). A small `running.marker` file is written at start and removed on every clean
//! exit or restart; finding it at start means the last session never got that far.

use std::path::Path;

pub const FILE_NAME: &str = "running.marker";

/// Writes the marker for this session. Returns true when the last session left its marker,
/// that is, did not exit cleanly.
pub fn begin(dir: &Path) -> bool {
    let path = dir.join(FILE_NAME);
    let unclean = path.exists();
    if let Err(e) = std::fs::write(&path, std::process::id().to_string()) {
        tracing::warn!("The session marker could not be written: {}", e);
    }
    unclean
}

/// Removes the marker: this session is ending cleanly.
pub fn end(dir: &Path) {
    match std::fs::remove_file(dir.join(FILE_NAME)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("The session marker could not be removed: {}", e),
    }
}

/// [`end`] for the app's own data folder.
pub fn end_current() {
    if let Ok(dir) = crate::db::get_app_dir() {
        end(&dir);
    }
}
