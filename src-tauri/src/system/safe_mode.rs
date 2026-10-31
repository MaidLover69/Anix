//! Safe mode: after two starts in a row that did not exit cleanly, Anix starts with the options
//! most likely to get it running again instead of crashing in a loop. This start uses the web
//! player, turns WebKit's compositing off, skips the extension-list refresh and does not reopen
//! the screen a restart asked for. One clean exit turns it off again.
//!
//! Decided in `main` before WebKit starts (the compositing switch is an environment variable
//! WebKit reads once), from the session marker, which it only reads.

use std::path::Path;
use std::sync::OnceLock;

/// Unclean exits in a row that start safe mode.
pub const UNCLEAN_STARTS: u32 = 2;
const COMPOSITING_VAR: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";
/// Set next to the variable above when safe mode set it, so a later normal start (a restart
/// inherits the environment) can take it away again without touching the user's own setting.
const MARK_VAR: &str = "ANIX_SAFE_MODE_SET";

static ACTIVE: OnceLock<bool> = OnceLock::new();

/// Whether `unclean` exits in a row call for safe mode.
pub fn needed(unclean: u32) -> bool {
    unclean >= UNCLEAN_STARTS
}

/// Called once from `main`, before WebKit starts.
pub fn apply(app_dir: Option<&Path>) {
    let active = app_dir.is_some_and(|dir| needed(super::session_marker::peek(dir)));
    let _ = ACTIVE.set(active);
    let ours = std::env::var_os(MARK_VAR).is_some();
    if active && std::env::var_os(COMPOSITING_VAR).is_none() {
        std::env::set_var(COMPOSITING_VAR, "1");
        std::env::set_var(MARK_VAR, "1");
    } else if !active && ours {
        std::env::remove_var(COMPOSITING_VAR);
        std::env::remove_var(MARK_VAR);
    }
}

/// True when this run started in safe mode. False before [`apply`] (tests, the background checker).
pub fn active() -> bool {
    ACTIVE.get().copied().unwrap_or(false)
}
