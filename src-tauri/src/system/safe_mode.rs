//! Safe mode: after two starts in a row that did not exit cleanly, Anix starts with the options
//! most likely to get it running again instead of crashing in a loop. This start uses the web
//! player, turns WebKit's DMA-BUF renderer off (see `hwaccel::apply_dmabuf_workaround`), skips
//! the extension-list refresh and does not reopen the screen a restart asked for. From the third
//! unclean exit in a row WebKit's compositing goes off too. One clean exit turns it all off again.
//!
//! Decided in `main` before WebKit starts (both switches are environment variables WebKit reads
//! once), from the session marker, which it only reads.

use std::path::Path;
use std::sync::OnceLock;

/// Unclean exits in a row that start safe mode.
pub const UNCLEAN_STARTS: u32 = 2;
/// Unclean exits in a row that also turn compositing off (the heavier step).
pub const COMPOSITING_STARTS: u32 = 3;
const COMPOSITING_VAR: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";
/// Set next to the variable above when safe mode set it, so a later normal start (a restart
/// inherits the environment) can take it away again without touching the user's own setting.
const MARK_VAR: &str = "ANIX_SAFE_MODE_SET";

static ACTIVE: OnceLock<bool> = OnceLock::new();

/// Whether `unclean` exits in a row call for safe mode.
pub fn needed(unclean: u32) -> bool {
    unclean >= UNCLEAN_STARTS
}

/// Whether `unclean` exits in a row also call for compositing off.
pub fn compositing_needed(unclean: u32) -> bool {
    unclean >= COMPOSITING_STARTS
}

/// What `apply` does with the compositing variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    Set,
    /// Safe mode set it on an earlier run (a restart inherits the environment) and this run does not want it.
    Clear,
    Untouched,
    /// The user's own value: never changed.
    User,
}

/// The rule for the compositing variable, from its value, safe mode's mark and whether it is wanted.
pub fn switch_env(value: Option<&str>, mark: Option<&str>, wanted: bool) -> Switch {
    let ours = match value {
        None => true,
        Some(v) => v == "1" && mark == Some("1"),
    };
    match (ours, wanted, value.is_some()) {
        (false, _, _) => Switch::User,
        (true, true, false) => Switch::Set,
        (true, false, true) => Switch::Clear,
        _ => Switch::Untouched,
    }
}

/// What `apply` does with the compositing variable after `unclean` unclean exits in a row.
pub fn compositing_switch(unclean: u32, value: Option<&str>, mark: Option<&str>) -> Switch {
    switch_env(value, mark, compositing_needed(unclean))
}

/// Called once from `main`, before WebKit starts.
pub fn apply(app_dir: Option<&Path>) {
    let unclean = app_dir.map_or(0, super::session_marker::peek);
    let _ = ACTIVE.set(needed(unclean));
    let value = std::env::var(COMPOSITING_VAR).ok();
    let mark = std::env::var(MARK_VAR).ok();
    match compositing_switch(unclean, value.as_deref(), mark.as_deref()) {
        Switch::Set => {
            std::env::set_var(COMPOSITING_VAR, "1");
            std::env::set_var(MARK_VAR, "1");
        }
        Switch::Clear => {
            std::env::remove_var(COMPOSITING_VAR);
            std::env::remove_var(MARK_VAR);
        }
        Switch::Untouched | Switch::User => {}
    }
}

/// Whether WebKit's compositing is off this run: safe mode or the user's own variable. WebKitGTK
/// aborts the page when `document.startViewTransition` runs then, so the page drops that method.
pub fn compositing_off() -> bool {
    compositing_off_value(std::env::var_os(COMPOSITING_VAR).as_deref())
}

/// WebKitGTK's rule for the variable: any value but "0" turns compositing off.
pub fn compositing_off_value(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| v != "0")
}

/// True when this run started in safe mode. False before [`apply`] (tests, the background checker).
pub fn active() -> bool {
    ACTIVE.get().copied().unwrap_or(false)
}
