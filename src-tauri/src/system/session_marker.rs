//! Tells the next start that the last session ended without a clean exit (a crash, a kill, a
//! power cut), and how many starts in a row did. A small `running.marker` file is written at
//! start and removed on every clean exit or restart; finding it at start means the last
//! session never got that far. It holds the pid, the count of unclean starts so far, and the
//! boot and login session it was written in.

use std::path::Path;

pub const FILE_NAME: &str = "running.marker";

/// Unclean exits in a row before this start: 0 after a clean exit, 1 after one crash, and so
/// on. Reads only; a second launch that hands over to the running window may call it.
pub fn peek(dir: &Path) -> u32 {
    match std::fs::read_to_string(dir.join(FILE_NAME)) {
        Ok(text) => {
            let mut parts = text.split_whitespace();
            let _pid = parts.next();
            // A marker from before the count was kept (pid only) counts as one.
            let count = parts.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
            let written_in = parts.collect::<Vec<_>>().join(" ");
            // Logging out or shutting down can end Anix before its exit steps (GTK quits when
            // the display goes): a marker from another boot or login session is no crash.
            if !written_in.is_empty() && written_in != origin() {
                return 0;
            }
            count.saturating_add(1)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => 1,
    }
}

/// Writes the marker for this session. Returns [`peek`]: how many sessions in a row did not
/// exit cleanly before this one. A second launch, which hands over to the running Anix and
/// exits, leaves that Anix's marker alone.
pub fn begin(dir: &Path) -> u32 {
    let unclean = peek(dir);
    if owner_running(dir) {
        return unclean;
    }
    write(dir, std::process::id(), unclean);
    unclean
}

/// For a start that gives up before it runs (the database did not open): leaves the marker as
/// this start found it, so its clean exit does not wipe out an earlier session's crash.
pub fn abandon(dir: &Path, unclean: u32) {
    if unclean > 0 {
        // pid 0 is never this process, so `end` keeps it.
        write(dir, 0, unclean - 1);
    }
}

/// Removes the marker if this process wrote it: this session is ending cleanly.
pub fn end(dir: &Path) {
    let path = dir.join(FILE_NAME);
    let own = std::process::id().to_string();
    if !std::fs::read_to_string(&path).is_ok_and(|text| text.split_whitespace().next() == Some(own.as_str())) {
        return;
    }
    if let Err(e) = std::fs::remove_file(path) {
        tracing::warn!("The session marker could not be removed: {}", e);
    }
}

/// [`end`] for the app's own data folder.
pub fn end_current() {
    if let Ok(dir) = crate::db::get_app_dir() {
        end(&dir);
    }
}

fn write(dir: &Path, pid: u32, unclean: u32) {
    if let Err(e) = std::fs::write(dir.join(FILE_NAME), format!("{} {} {}", pid, unclean, origin())) {
        tracing::warn!("The session marker could not be written: {}", e);
    }
}

/// Whether the marker's pid is another Anix that still runs.
fn owner_running(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(FILE_NAME)) else { return false };
    let pid = text.split_whitespace().next().unwrap_or("");
    !crate::embedded_mpv::policy::marker_owner_gone(pid, std::process::id(), |pid| {
        std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|name| name.trim_end() == "anix")
    })
}

/// The boot and login session this process runs in, "-" for what is unknown.
fn origin() -> String {
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap_or_default();
    let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    let known = |s: &str| if s.trim().is_empty() { "-".to_owned() } else { s.split_whitespace().collect::<Vec<_>>().join("_") };
    format!("{} {}", known(&boot), known(&session))
}
