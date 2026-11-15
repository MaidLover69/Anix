//! Settings › System › Fast drawing, for NVIDIA's driver. There, WebKit's DMA-BUF renderer can
//! leave the window blank, so the web player draws through a slow copy path (see `hwaccel`).
//! Users whose driver works turn the fast path on: the next start asks Keep / Switch back with a
//! countdown, and a start that never got an answer (blank window, app closed) turns it off
//! again. `--safe-graphics` turns it off from the command line.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::AppHandle;

use super::hwaccel;

const STATE_FILE: &str = "fast-drawing.json";
/// Starts without fast drawing (it stays off) and on the web player, without forcing the renderer.
pub const SAFE_ARG: &str = "--safe-graphics";
/// Why Anix turned it off; codes the page translates.
pub const NOT_CONFIRMED: &str = "not_confirmed";
pub const DECLINED: &str = "declined";
pub const SAFE_STARTED: &str = "safe_graphics";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FastDrawingState {
    pub on: bool,
    /// Set while this start's question waits; still set at the next start means nobody answered.
    pub trial: bool,
    /// The WebKitGTK version the user said draws correctly.
    pub confirmed: Option<String>,
    pub reason: Option<String>,
}

/// The state to keep and whether this start uses fast drawing.
pub fn at_start(state: FastDrawingState, safe_graphics: bool, safe_mode: bool) -> (FastDrawingState, bool) {
    if state.trial {
        let next = FastDrawingState { on: false, trial: false, reason: Some(NOT_CONFIRMED.into()), ..state };
        return (next, false);
    }
    if safe_graphics && state.on {
        return (FastDrawingState { on: false, reason: Some(SAFE_STARTED.into()), ..state }, false);
    }
    let use_fast = state.on && !safe_graphics && !safe_mode;
    (state, use_fast)
}

/// Whether this start asks "Does Anix look right?". Embedded mpv asks its own question about the
/// same window, so this one waits for a web player start.
pub fn needs_question(state: &FastDrawingState, using_fast: bool, wants_mpv: bool, webkit: &str) -> bool {
    using_fast && !wants_mpv && state.confirmed.as_deref() != Some(webkit)
}

pub fn answer(state: FastDrawingState, keep: bool, webkit: &str) -> FastDrawingState {
    if keep {
        FastDrawingState { trial: false, confirmed: Some(webkit.to_string()), reason: None, ..state }
    } else {
        FastDrawingState { on: false, trial: false, confirmed: None, reason: Some(DECLINED.into()) }
    }
}

static USING: AtomicBool = AtomicBool::new(false);
static WANTS_MPV: AtomicBool = AtomicBool::new(false);
static ANSWERED: AtomicBool = AtomicBool::new(false);
static LOCK: Mutex<()> = Mutex::new(());

fn state_path() -> Option<PathBuf> {
    crate::db::get_config_dir().ok().map(|dir| dir.join(STATE_FILE))
}

fn load() -> FastDrawingState {
    state_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(state: &FastDrawingState) -> Result<(), String> {
    let path = state_path().ok_or("The settings folder is unknown")?;
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("The fast drawing choice could not be saved: {e}"))
}

/// Called once from `main` before WebKit starts: whether this run draws the fast way on NVIDIA.
pub fn decide_at_start(wants_mpv: bool) -> bool {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let safe_graphics = std::env::args().any(|arg| arg == SAFE_ARG);
    let state = load();
    let (next, use_fast) = at_start(state.clone(), safe_graphics, super::safe_mode::active());
    if next != state {
        if let Err(e) = save(&next) {
            eprintln!("{e}"); // logging starts later
        }
    }
    let use_fast = use_fast && hwaccel::nvidia_driver_loaded();
    USING.store(use_fast, Ordering::SeqCst);
    WANTS_MPV.store(wants_mpv, Ordering::SeqCst);
    use_fast
}

#[derive(Debug, Clone, Serialize)]
pub struct FastDrawingStatus {
    /// NVIDIA's own driver is loaded: the only place the setting does anything.
    pub nvidia: bool,
    pub on: bool,
    /// This run draws the fast way (on NVIDIA, through the setting).
    pub active: bool,
    /// The page shows "Does Anix look right?" now.
    pub ask: bool,
    pub reason: Option<String>,
}

#[tauri::command]
pub async fn fast_drawing_status() -> FastDrawingStatus {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut state = load();
    let active = USING.load(Ordering::SeqCst);
    let webkit = hwaccel::webkitgtk_version().unwrap_or_default();
    let ask = !ANSWERED.load(Ordering::SeqCst) && needs_question(&state, active, WANTS_MPV.load(Ordering::SeqCst), &webkit);
    if ask && !state.trial {
        // From here a start without an answer means the window may have been blank.
        state.trial = true;
        if let Err(e) = save(&state) {
            tracing::warn!("{}", e);
        }
    }
    FastDrawingStatus { nvidia: hwaccel::nvidia_driver_loaded(), on: state.on, active, ask, reason: state.reason }
}

/// Turns fast drawing on or off and restarts, since WebKit reads the switch once at start.
#[tauri::command]
pub fn fast_drawing_set(app: AppHandle, on: bool) -> Result<(), String> {
    {
        let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let state = load();
        save(&FastDrawingState { on, trial: false, reason: None, ..state })?;
    }
    super::restart(&app)
}

/// The answer to "Does Anix look right?": `keep`, `switch_back` or `no_answer`. Anything but keep
/// turns fast drawing off and restarts.
#[tauri::command]
pub fn fast_drawing_answer(app: AppHandle, answer: String) -> Result<(), String> {
    let keep = answer == "keep";
    let webkit = hwaccel::webkitgtk_version().unwrap_or_default();
    {
        let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = self::answer(load(), keep, &webkit);
        if answer == "no_answer" {
            next.reason = Some(NOT_CONFIRMED.into());
        }
        save(&next)?;
    }
    ANSWERED.store(true, Ordering::SeqCst);
    if keep {
        return Ok(());
    }
    tracing::warn!("Fast drawing was not kept ({}); restarting without it", answer);
    super::restart(&app)
}
