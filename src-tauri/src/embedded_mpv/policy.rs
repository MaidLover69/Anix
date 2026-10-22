//! The embedded player's decisions that need no libmpv, GL or window: which engine starts,
//! the confirm-or-revert state after a switch, the hardware decoding mode, the self-test
//! verdict, the black-picture watch, cache options and the Anime4K shader lists.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

// ---- engine choice and the confirm-or-revert state ------------------------------------------

/// What the backend remembers about the embedded engine (`embedded-mpv.json`, next to the
/// settings; the page saves the whole settings object, so backend-written values live here).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineState {
    /// The libmpv/WebKit pair the user confirmed shows the page over the video.
    pub confirmed: Option<String>,
    /// Set while a start with an unconfirmed pair waits for "Keep"; still set at the next
    /// start means the app was closed or killed during the question.
    pub trial: bool,
    /// Why the web player is used although embedded mpv was chosen (shown in Settings).
    pub reason: Option<String>,
    /// The route to open after a restart the app asked for.
    pub reopen: Option<String>,
    pub selftest: Option<SelfTest>,
    /// Playback saw a black picture with full interop: run the self-test again.
    pub selftest_recheck: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    Web { reason: Option<String> },
    Mpv,
}

pub const UNCONFIRMED_REASON: &str =
    "The last start with embedded mpv was not confirmed, so Anix went back to the web player.";
pub const DECLINED_REASON: &str = "You chose to switch back to the web player.";
pub const NO_ANSWER_REASON: &str =
    "Nobody confirmed that the video player was showing correctly, so Anix went back to the web player.";

/// Why this run must use the web engine whatever the setting says, if it must.
pub fn forced_web(args: &[String], env_engine: Option<&str>) -> Option<String> {
    if args.iter().any(|a| a == "--web-player") {
        Some("Anix was started with --web-player.".to_string())
    } else if env_engine.is_some_and(|v| v.trim().eq_ignore_ascii_case("web")) {
        Some("ANIX_PLAYER_ENGINE=web is set.".to_string())
    } else {
        None
    }
}

/// The engine for this start. An unanswered trial from the previous start means the window
/// may have been blank, so that start ends the trial on the web engine.
pub fn decide(engine_setting: &str, forced: Option<String>, state: &EngineState) -> Start {
    if engine_setting != "mpv" {
        return Start::Web { reason: None };
    }
    if let Some(reason) = forced {
        return Start::Web { reason: Some(reason) };
    }
    if state.trial {
        return Start::Web { reason: Some(UNCONFIRMED_REASON.to_string()) };
    }
    Start::Mpv
}

/// The libmpv and WebKit versions a confirmation holds for.
pub fn fingerprint(mpv_version: &str, webkit_version: &str) -> String {
    format!("mpv {} / WebKitGTK {}", mpv_version.trim(), webkit_version.trim())
}

/// Whether this start has to ask "Is the video player showing correctly?".
pub fn needs_confirmation(state: &EngineState, fingerprint: &str) -> bool {
    state.confirmed.as_deref() != Some(fingerprint)
}

/// The user's answer (or the lack of one) to the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Keep,
    SwitchBack,
    NoAnswer,
}

/// The state after `answer`; for anything but Keep the caller also sets the engine to web.
pub fn answer(mut state: EngineState, answer: Answer, fingerprint: &str) -> EngineState {
    state.trial = false;
    match answer {
        Answer::Keep => {
            state.confirmed = Some(fingerprint.to_string());
            state.reason = None;
        }
        Answer::SwitchBack => state.reason = Some(DECLINED_REASON.to_string()),
        Answer::NoAnswer => state.reason = Some(NO_ANSWER_REASON.to_string()),
    }
    state
}

/// The user picked an engine in Settings: a switch to mpv is asked about again.
pub fn engine_changed(mut state: EngineState, engine: &str) -> EngineState {
    state.reason = None;
    if engine == "mpv" {
        state.confirmed = None;
    }
    state
}

/// Only in-app routes are reopened after a restart.
pub fn valid_route(route: &str) -> bool {
    route.starts_with('/')
        && !route.starts_with("//")
        && route.len() <= 512
        && !route.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\')
}

// ---- hardware decoding -------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwdecMode {
    /// Copy-back, or full interop once the self-test passed on this machine.
    Auto,
    /// Always decode on the GPU and copy the frames back.
    Copy,
    /// Software decoding.
    Off,
}

impl HwdecMode {
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("copy") => Self::Copy,
            Some("off") => Self::Off,
            _ => Self::Auto,
        }
    }
}

/// mpv's `hwdec` value. Copy-back needs no GL interop, so it can never draw black.
pub fn hwdec_value(mode: HwdecMode, selftest_passed: bool) -> &'static str {
    match mode {
        HwdecMode::Auto if selftest_passed => "auto-safe",
        HwdecMode::Auto | HwdecMode::Copy => "auto-copy-safe",
        HwdecMode::Off => "no",
    }
}

/// True when `hwdec-current` names a decoder that hands frames to GL directly.
pub fn is_interop(hwdec_current: &str) -> bool {
    let value = hwdec_current.trim();
    !value.is_empty() && value != "no" && !value.ends_with("-copy")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfTest {
    pub ok: bool,
    /// GL vendor, renderer and version of the context the video is drawn with.
    pub driver: String,
    pub mpv: String,
    pub app: String,
    /// Unix seconds.
    pub at: u64,
}

/// The stored result when it was made with this driver, libmpv and app version.
pub fn selftest_for<'a>(stored: Option<&'a SelfTest>, driver: &str, mpv: &str, app: &str) -> Option<&'a SelfTest> {
    stored.filter(|t| t.driver == driver && t.mpv == mpv && t.app == app)
}

/// Whether the self-test must run (again) on this machine.
pub fn selftest_due(state: &EngineState, mode: HwdecMode, driver: &str, mpv: &str, app: &str) -> bool {
    mode == HwdecMode::Auto && (state.selftest_recheck || selftest_for(state.selftest.as_ref(), driver, mpv, app).is_none())
}

/// The self-test clip (`resources/hwdec-selftest.mp4`, 256x144): four bands of these colours.
pub const SELFTEST_SIZE: (i32, i32) = (256, 144);
pub const SELFTEST_POINTS: [(i32, i32); 4] = [(32, 72), (96, 72), (160, 72), (224, 72)];
pub const SELFTEST_COLOURS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];

/// Every sample is within `tolerance` of its expected colour on every channel.
pub fn colours_match(samples: &[[u8; 3]], expected: &[[u8; 3]], tolerance: u8) -> bool {
    samples.len() == expected.len()
        && samples
            .iter()
            .zip(expected)
            .all(|(s, e)| s.iter().zip(e).all(|(a, b)| a.abs_diff(*b) <= tolerance))
}

/// No sample is brighter than the darkest grey a picture can show.
pub fn all_black(samples: &[[u8; 3]]) -> bool {
    !samples.is_empty() && samples.iter().all(|s| s.iter().all(|c| *c <= 16))
}

/// Watches the drawn picture while full interop is used: black for `BLACK_SECONDS` while the
/// playhead moves on means interop draws nothing, and the session switches to copy-back.
/// shortcut: a black title card of 3 s+ looks the same and costs a copy-back session plus a
/// self-test re-run (which keeps the stored pass); upgrade to a copy-back re-render check if
/// logs show it firing on real content.
#[derive(Debug, Default)]
pub struct BlackWatch {
    since: Option<(f64, f64)>,
    fired: bool,
}

pub const BLACK_SECONDS: f64 = 3.0;

impl BlackWatch {
    /// `now` in seconds of any clock, `time_pos` the playhead. True once, when to switch.
    pub fn observe(&mut self, now: f64, black: bool, time_pos: f64) -> bool {
        if self.fired {
            return false;
        }
        if !black {
            self.since = None;
            return false;
        }
        let (start, start_pos) = *self.since.get_or_insert((now, time_pos));
        // A seek backwards restarts the count.
        if time_pos < start_pos {
            self.since = Some((now, time_pos));
            return false;
        }
        if now - start >= BLACK_SECONDS && time_pos - start_pos >= BLACK_SECONDS * 0.6 {
            self.fired = true;
            return true;
        }
        false
    }
}

// ---- buffer and disk cache -----------------------------------------------------------------

pub const DEFAULT_BUFFER_MB: u64 = 150;
pub const DEFAULT_READAHEAD_SECS: u64 = 30;
pub const DEFAULT_DISK_CACHE_MB: u64 = 1024;

/// mpv options for the buffer and the on-disk cache, from the settings file. The disk cache
/// lives in `cache_dir` (under the app's cache folder) and its cap is mpv's cache size.
pub fn cache_options(settings: &Value, cache_dir: &Path) -> Vec<(&'static str, String)> {
    let number = |key: &str, default: u64, min: u64, max: u64| {
        settings.get(key).and_then(Value::as_u64).unwrap_or(default).clamp(min, max)
    };
    let buffer = number("player_buffer_mb", DEFAULT_BUFFER_MB, 16, 2048);
    let readahead = number("player_readahead_secs", DEFAULT_READAHEAD_SECS, 5, 600);
    let disk = settings.get("player_disk_cache").and_then(Value::as_bool).unwrap_or(false);
    let disk_cap = number("player_disk_cache_mb", DEFAULT_DISK_CACHE_MB, 256, 16384);
    let mut options = vec![
        ("cache", "yes".to_string()),
        ("demuxer-readahead-secs", readahead.to_string()),
        ("demuxer-max-bytes", format!("{}MiB", if disk { disk_cap.max(buffer) } else { buffer })),
        ("cache-on-disk", if disk { "yes" } else { "no" }.to_string()),
    ];
    if disk {
        options.push(("cache-dir", cache_dir.to_string_lossy().into_owned()));
    }
    options
}

// ---- Anime4K -------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anime4k {
    Off,
    AFast,
    BFast,
    CFast,
    AHq,
    BHq,
    CHq,
}

/// The shader chain of a preset, in order, as the Anime4K project documents its modes
/// (fast: for most GPUs; HQ: for strong GPUs). Files are in `resources/anime4k`.
pub fn anime4k_files(preset: Anime4k) -> Vec<&'static str> {
    let (restore, upscale, last) = match preset {
        Anime4k::Off => return Vec::new(),
        Anime4k::AFast => (Some("Restore_CNN_M"), "Upscale_CNN_x2_M", "Upscale_CNN_x2_S"),
        Anime4k::BFast => (Some("Restore_CNN_Soft_M"), "Upscale_CNN_x2_M", "Upscale_CNN_x2_S"),
        Anime4k::CFast => (None, "Upscale_Denoise_CNN_x2_M", "Upscale_CNN_x2_S"),
        Anime4k::AHq => (Some("Restore_CNN_VL"), "Upscale_CNN_x2_VL", "Upscale_CNN_x2_M"),
        Anime4k::BHq => (Some("Restore_CNN_Soft_VL"), "Upscale_CNN_x2_VL", "Upscale_CNN_x2_M"),
        Anime4k::CHq => (None, "Upscale_Denoise_CNN_x2_VL", "Upscale_CNN_x2_M"),
    };
    let mut files = vec!["Clamp_Highlights"];
    files.extend(restore);
    files.extend([upscale, "AutoDownscalePre_x2", "AutoDownscalePre_x4", last]);
    files
}

/// The `glsl-shaders` value (a `:`-separated path list) for `preset` from `dir`.
pub fn shader_list(dir: &Path, preset: Anime4k) -> Result<String, String> {
    let dir = dir.to_string_lossy();
    if dir.contains(':') {
        return Err("The shader folder path contains a colon".to_string());
    }
    Ok(anime4k_files(preset)
        .iter()
        .map(|name| format!("{}/Anime4K_{}.glsl", dir.trim_end_matches('/'), name))
        .collect::<Vec<_>>()
        .join(":"))
}
