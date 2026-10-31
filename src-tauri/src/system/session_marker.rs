//! Tells the next start that the last session ended without a clean exit (a crash, a kill, a
//! power cut), and how many starts in a row did. A small `running.marker` file is written at
//! start and removed on every clean exit or restart; finding it at start means the last
//! session never got that far. It holds the pid and the count of unclean starts so far.

use std::path::Path;

pub const FILE_NAME: &str = "running.marker";

/// Unclean exits in a row before this start: 0 after a clean exit, 1 after one crash, and so
/// on. Reads only; a second launch that hands over to the running window may call it.
pub fn peek(dir: &Path) -> u32 {
    match std::fs::read_to_string(dir.join(FILE_NAME)) {
        // A marker from before the count was kept (pid only) counts as one.
        Ok(text) => text.split_whitespace().nth(1).and_then(|n| n.parse::<u32>().ok()).unwrap_or(0).saturating_add(1),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => 1,
    }
}

/// Writes the marker for this session. Returns [`peek`]: how many sessions in a row did not
/// exit cleanly before this one.
pub fn begin(dir: &Path) -> u32 {
    let unclean = peek(dir);
    if let Err(e) = std::fs::write(dir.join(FILE_NAME), format!("{} {}", std::process::id(), unclean)) {
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
