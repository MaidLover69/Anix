//! Embedded mpv player, the default "Player engine" on Linux.
//!
//! libmpv renders through its OpenGL render API into a GL area placed under the main
//! window's webview, whose background is made transparent; the page draws the controls on
//! top and leaves the video region transparent. The page drives playback only through the
//! fixed set of [`Action`]s below, never with raw mpv commands. Any failure while starting
//! leaves the web player in charge.
//!
//! A switch to this engine is confirmed by the user after the restart (a blank window cannot
//! be detected from the page); see [`policy`] for the rules.

pub mod ffi;
#[cfg(target_os = "linux")]
mod layer;
pub mod policy;
#[cfg(target_os = "linux")]
mod selftest;

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use policy::{Anime4k, Answer, EngineState, HwdecMode, SelfTest, Start};
use std::ffi::{c_int, CStr, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// The event the page listens to for player state.
pub const EVENT: &str = "embedded-mpv";
const SETTING_KEY: &str = "player_engine";
const STATE_FILE: &str = "embedded-mpv.json";
/// The page has this long to show the question and get "Keep" before the backend itself
/// goes back to the web player (the page's own countdown is 15 s).
const CONFIRM_DEADLINE: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioPreset {
    None,
    /// Levels speech up against music and effects.
    Dialogue,
    /// Evens out loud and quiet scenes.
    Night,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoPreset {
    None,
    Deband,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Fit,
    Crop,
    Fill,
}

/// Everything the page may ask the player to do.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Load { url: String, start: Option<f64> },
    /// An external subtitle file; `select` makes it the shown track.
    AddSubtitle { url: String, title: Option<String>, select: bool },
    Stop,
    SetPause { paused: bool },
    Seek { seconds: f64, absolute: bool },
    SetVolume { value: f64 },
    SetMute { muted: bool },
    SetSpeed { value: f64 },
    /// `None` turns the track off.
    SetAudioTrack { id: Option<i64> },
    SetSubTrack { id: Option<i64> },
    /// The second subtitle, drawn at the top.
    SetSecondarySubTrack { id: Option<i64> },
    SetSubDelay { seconds: f64 },
    SetSubScale { value: f64 },
    SetSubPos { value: f64 },
    SetSecondarySubPos { value: f64 },
    SetAudioPreset { preset: AudioPreset },
    SetVideoPreset { preset: VideoPreset },
    SetFit { fit: Fit },
    SetShaders { preset: Anime4k },
    /// Where the transparent video region is, in CSS pixels of the page.
    SetVideoRect { x: f64, y: f64, width: f64, height: f64, viewport_width: f64, viewport_height: f64 },
}

/// One libmpv call an [`Action`] turns into.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Set(&'static str, String),
    Command(Vec<String>),
    /// `glsl-shaders` for a preset; the folder is only known to the running app.
    Shaders(Anime4k),
}

fn finite(value: f64, what: &str) -> Result<f64, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("{} must be a number", what))
    }
}

fn yes_no(on: bool) -> String {
    if on { "yes" } else { "no" }.to_string()
}

fn track(id: Option<i64>) -> Result<String, String> {
    match id {
        None => Ok("no".to_string()),
        Some(id) if id >= 1 => Ok(id.to_string()),
        Some(_) => Err("Track ids start at 1".to_string()),
    }
}

/// `[left, right, top, bottom]` margin ratios that put the video inside the page's rect.
pub fn margins(x: f64, y: f64, width: f64, height: f64, viewport_width: f64, viewport_height: f64) -> Result<[f64; 4], String> {
    for value in [x, y, width, height, viewport_width, viewport_height] {
        finite(value, "The video rect")?;
    }
    if viewport_width < 1.0 || viewport_height < 1.0 {
        return Err("The viewport is empty".to_string());
    }
    let ratio = |v: f64, of: f64| (v / of).clamp(0.0, 1.0);
    let left = ratio(x, viewport_width);
    let top = ratio(y, viewport_height);
    let right = ratio(viewport_width - x - width.max(0.0), viewport_width);
    let bottom = ratio(viewport_height - y - height.max(0.0), viewport_height);
    // mpv needs some area left for the video.
    if left + right >= 0.99 || top + bottom >= 0.99 {
        return Err("The video rect is too small".to_string());
    }
    Ok([left, right, top, bottom])
}

fn from_proxy(url: &str, proxy_base: &str) -> Result<(), String> {
    let allowed = url.starts_with(&format!("{}/", proxy_base))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    if allowed {
        Ok(())
    } else {
        Err("Only streams from the Anix stream proxy can be played".to_string())
    }
}

/// The libmpv calls for `action`. Loads are only allowed from Anix's own stream proxy, which
/// is also how local files are served.
pub fn plan(action: &Action, proxy_base: &str) -> Result<Vec<Op>, String> {
    use Op::{Command, Set};
    Ok(match action {
        Action::Load { url, start } => {
            from_proxy(url, proxy_base)?;
            let start = match start {
                Some(s) if finite(*s, "The start time")? > 0.0 => format!("{:.3}", s),
                _ => "none".to_string(),
            };
            vec![Set("start", start), Command(vec!["loadfile".into(), url.clone(), "replace".into()])]
        }
        Action::AddSubtitle { url, title, select } => {
            from_proxy(url, proxy_base)?;
            let title: String = title.as_deref().unwrap_or("").chars().filter(|c| !c.is_control()).take(80).collect();
            let flag = if *select { "select" } else { "auto" };
            vec![Command(vec!["sub-add".into(), url.clone(), flag.into(), title])]
        }
        Action::Stop => vec![Command(vec!["stop".into()])],
        Action::SetPause { paused } => vec![Set("pause", yes_no(*paused))],
        Action::Seek { seconds, absolute } => {
            let seconds = finite(*seconds, "The seek position")?;
            let mode = if *absolute { "absolute" } else { "relative" };
            vec![Command(vec!["seek".into(), format!("{:.3}", seconds), mode.into()])]
        }
        Action::SetVolume { value } => vec![Set("volume", format!("{:.1}", finite(*value, "Volume")?.clamp(0.0, 130.0)))],
        Action::SetMute { muted } => vec![Set("mute", yes_no(*muted))],
        Action::SetSpeed { value } => vec![Set("speed", format!("{:.3}", finite(*value, "Speed")?.clamp(0.25, 4.0)))],
        Action::SetAudioTrack { id } => vec![Set("aid", track(*id)?)],
        Action::SetSubTrack { id } => vec![Set("sid", track(*id)?)],
        Action::SetSecondarySubTrack { id } => vec![Set("secondary-sid", track(*id)?)],
        Action::SetSubDelay { seconds } => {
            vec![Set("sub-delay", format!("{:.3}", finite(*seconds, "Subtitle delay")?.clamp(-600.0, 600.0)))]
        }
        Action::SetSubScale { value } => {
            vec![Set("sub-scale", format!("{:.2}", finite(*value, "Subtitle size")?.clamp(0.3, 3.0)))]
        }
        Action::SetSubPos { value } => {
            vec![Set("sub-pos", format!("{:.0}", finite(*value, "Subtitle position")?.clamp(0.0, 150.0)))]
        }
        Action::SetSecondarySubPos { value } => {
            let value = finite(*value, "Second subtitle position")?.clamp(0.0, 150.0);
            vec![Set("secondary-sub-pos", format!("{:.0}", value))]
        }
        Action::SetAudioPreset { preset } => vec![Set(
            "af",
            match preset {
                AudioPreset::None => "",
                AudioPreset::Dialogue => "lavfi=[speechnorm=e=6:r=0.0001:l=1]",
                AudioPreset::Night => "lavfi=[dynaudnorm=f=250:g=15]",
            }
            .to_string(),
        )],
        Action::SetVideoPreset { preset } => vec![Set("deband", yes_no(*preset == VideoPreset::Deband))],
        Action::SetFit { fit } => {
            let (keepaspect, panscan) = match fit {
                Fit::Fit => (true, "0"),
                Fit::Crop => (true, "1"),
                Fit::Fill => (false, "0"),
            };
            vec![Set("keepaspect", yes_no(keepaspect)), Set("panscan", panscan.to_string())]
        }
        Action::SetShaders { preset } => vec![Op::Shaders(*preset)],
        Action::SetVideoRect { x, y, width, height, viewport_width, viewport_height } => {
            let [left, right, top, bottom] = margins(*x, *y, *width, *height, *viewport_width, *viewport_height)?;
            vec![
                Set("video-margin-ratio-left", format!("{:.5}", left)),
                Set("video-margin-ratio-right", format!("{:.5}", right)),
                Set("video-margin-ratio-top", format!("{:.5}", top)),
                Set("video-margin-ratio-bottom", format!("{:.5}", bottom)),
            ]
        }
    })
}

/// True for a libavcodec licence string of a GPL or non-free build.
pub fn is_gpl_license(license: &str) -> bool {
    let license = license.to_ascii_lowercase();
    !license.contains("lgpl") && (license.contains("gpl") || license.contains("nonfree"))
}

struct Player {
    lib: &'static ffi::Lib,
    handle: ffi::Handle,
}

// libmpv's client API is thread-safe; the handle lives as long as the app.
unsafe impl Send for Player {}
unsafe impl Sync for Player {}

impl Player {
    fn check(&self, code: c_int, what: &str) -> Result<(), String> {
        if code < 0 {
            Err(format!("{}: {}", what, self.lib.error_text(code)))
        } else {
            Ok(())
        }
    }

    fn set_option(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (CString::new(name).unwrap(), CString::new(value).map_err(|e| e.to_string())?);
        self.check(unsafe { (self.lib.set_option_string)(self.handle, n.as_ptr(), v.as_ptr()) }, name)
    }

    fn get(&self, name: &str) -> Option<String> {
        let n = CString::new(name).ok()?;
        let raw = unsafe { (self.lib.get_property_string)(self.handle, n.as_ptr()) };
        if raw.is_null() {
            return None;
        }
        let value = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
        unsafe { (self.lib.free)(raw.cast()) };
        Some(value)
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (CString::new(name).map_err(|e| e.to_string())?, CString::new(value).map_err(|e| e.to_string())?);
        self.check(unsafe { (self.lib.set_property_string)(self.handle, n.as_ptr(), v.as_ptr()) }, name)
    }

    fn apply(&self, op: &Op) -> Result<(), String> {
        match op {
            Op::Set(name, value) => self.set(name, value),
            Op::Shaders(preset) => {
                let dir = SHADER_DIR.get().ok_or("The Anime4K shaders are not installed")?;
                self.set("glsl-shaders", &policy::shader_list(dir, *preset)?)
            }
            Op::Command(args) => {
                let owned: Vec<CString> = args
                    .iter()
                    .map(|a| CString::new(a.as_str()))
                    .collect::<Result<_, _>>()
                    .map_err(|e| e.to_string())?;
                let mut ptrs: Vec<*const std::ffi::c_char> = owned.iter().map(|a| a.as_ptr()).collect();
                ptrs.push(std::ptr::null());
                self.check(unsafe { (self.lib.command)(self.handle, ptrs.as_mut_ptr()) }, &args[0])
            }
        }
    }

    fn tracks(&self) -> Value {
        let count: usize = self.get("track-list/count").and_then(|c| c.parse().ok()).unwrap_or(0);
        let tracks: Vec<Value> = (0..count.min(64))
            .map(|i| {
                let field = |name: &str| self.get(&format!("track-list/{}/{}", i, name));
                json!({
                    "id": field("id").and_then(|v| v.parse::<i64>().ok()),
                    "type": field("type"),
                    "lang": field("lang"),
                    "title": field("title"),
                    "codec": field("codec"),
                    "selected": field("selected").as_deref() == Some("yes"),
                    "default": field("default").as_deref() == Some("yes"),
                })
            })
            .collect();
        Value::Array(tracks)
    }
}

static PLAYER: OnceCell<Player> = OnceCell::new();
static APP: OnceCell<AppHandle> = OnceCell::new();
static SHADER_DIR: OnceCell<PathBuf> = OnceCell::new();
/// The engine decision of this run, made once before the window exists.
static DECISION: OnceCell<Start> = OnceCell::new();
/// `None` while the web engine is chosen; otherwise whether the embedded player started.
static STATUS: Mutex<Option<Result<(), String>>> = Mutex::new(None);
static STATE_LOCK: Mutex<()> = Mutex::new(());
/// The question "Is the video player showing correctly?" is open for this start.
static ASK: AtomicBool = AtomicBool::new(false);
/// The libmpv/WebKit pair of this run.
static FINGERPRINT: OnceCell<String> = OnceCell::new();
pub(crate) static MPV_VERSION: OnceCell<String> = OnceCell::new();
static HWDEC: Mutex<String> = Mutex::new(String::new());
/// What the render layer needs from the player state, without calling into mpv.
pub(crate) static PLAYING: AtomicBool = AtomicBool::new(false);
pub(crate) static INTEROP: AtomicBool = AtomicBool::new(false);
pub(crate) static TIME_POS: AtomicU64 = AtomicU64::new(0);
/// `[left, right, top, bottom]` margin ratios of the video, as last set by the page.
pub(crate) static MARGINS: Mutex<[f64; 4]> = Mutex::new([0.0; 4]);
/// The self-test should run once nothing plays.
pub(crate) static SELFTEST_DUE: AtomicBool = AtomicBool::new(false);

fn set_status(status: Result<(), String>) {
    if let Err(ref e) = status {
        tracing::warn!("Embedded mpv is not available, the web player is used: {}", e);
    }
    *STATUS.lock().unwrap_or_else(|p| p.into_inner()) = Some(status);
}

fn emit_status_changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, json!({ "kind": "status" }));
    }
}

/// Path to load libmpv from. Debug builds take the system library for local testing; release
/// builds load the LGPL build bundled with the AppImage (`scripts/build-libmpv-lgpl.sh`) and
/// otherwise the system's (RPM/DEB), which must pass the same LGPL check.
fn library_path() -> Result<String, String> {
    if cfg!(debug_assertions) {
        return Ok("libmpv.so.2".to_string());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("The app folder is unknown")?;
    let bundled = dir.join("../lib/anix/libmpv.so.2");
    Ok(if bundled.exists() { bundled.to_string_lossy().into_owned() } else { "libmpv.so.2".to_string() })
}

fn settings_path() -> Option<PathBuf> {
    crate::db::get_config_dir().ok().map(|dir| dir.join("settings.json"))
}

fn read_settings() -> Value {
    settings_path()
        .and_then(|path| crate::commands::settings::read_settings_file(&path).ok())
        .unwrap_or_else(|| json!({}))
}

/// The "Player engine" setting; settings from before it existed mean embedded mpv.
fn engine_setting(settings: &Value) -> String {
    settings.get(SETTING_KEY).and_then(Value::as_str).unwrap_or(default_engine()).to_string()
}

/// Release RPM/DEB builds have no LGPL libmpv of their own (only the AppImage bundles one), so
/// they start on the web player instead of announcing a fallback on every run.
fn default_engine() -> &'static str {
    if cfg!(debug_assertions) || std::env::var_os("APPIMAGE").is_some() {
        "mpv"
    } else {
        "web"
    }
}

fn set_engine_setting(engine: &str) -> Result<(), String> {
    let path = settings_path().ok_or("The settings folder is unknown")?;
    let _guard = crate::accounts::store::settings_lock();
    let mut settings = crate::commands::settings::read_settings_file(&path)?;
    if !settings.is_object() {
        settings = json!({});
    }
    settings[SETTING_KEY] = json!(engine);
    crate::commands::settings::write_settings_file(&path, &settings)
}

fn state_path() -> Option<PathBuf> {
    crate::db::get_config_dir().ok().map(|dir| dir.join(STATE_FILE))
}

fn load_state() -> EngineState {
    state_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Reads, changes and writes the state file under one lock.
fn update_state(change: impl FnOnce(EngineState) -> EngineState) -> EngineState {
    let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let state = change(load_state());
    if let Some(path) = state_path() {
        let written = serde_json::to_vec_pretty(&state)
            .map_err(|e| e.to_string())
            .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
        if let Err(e) = written {
            tracing::warn!("The embedded player state could not be saved: {}", e);
        }
    }
    state
}

/// The cache folder for mpv's on-disk cache (under the app's cache folder, never elsewhere),
/// created when missing.
fn cache_dir() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join(crate::db::APP_DIR_NAME).join("video-cache");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("The video cache folder could not be created: {}", e);
    }
    Some(dir)
}

/// libmpv, loaded once and refused when its FFmpeg is not LGPL (release builds).
fn load_lib() -> Result<&'static ffi::Lib, String> {
    static LIB: OnceCell<&'static ffi::Lib> = OnceCell::new();
    LIB.get_or_try_init(|| {
        let lib: &'static ffi::Lib = Box::leak(Box::new(ffi::Lib::load(&library_path()?)?));
        match lib.license() {
            Some(license) if is_gpl_license(&license) => {
                if !cfg!(debug_assertions) {
                    return Err(format!("The FFmpeg libmpv uses is not an LGPL build ({})", license));
                }
                tracing::warn!("libmpv uses a GPL FFmpeg ({}); fine for local testing only", license);
            }
            Some(license) => tracing::info!("libmpv FFmpeg licence: {}", license),
            None => tracing::warn!("The FFmpeg licence of libmpv could not be read"),
        }
        Ok(lib)
    })
    .copied()
}

fn decision() -> &'static Start {
    DECISION.get_or_init(|| {
        let args: Vec<String> = std::env::args().collect();
        let forced = policy::forced_web(&args, std::env::var("ANIX_PLAYER_ENGINE").ok().as_deref());
        let state = load_state();
        let mut decision = policy::decide(&engine_setting(&read_settings()), forced, &state);
        // Checked before WebKit's DMA-BUF renderer is kept on for mpv: a missing or refused
        // libmpv must not cost the blank-window workaround.
        if decision == Start::Mpv {
            if let Err(e) = load_lib() {
                decision = Start::Web { reason: Some(e) };
            }
        }
        if let Start::Web { reason: Some(reason) } = &decision {
            if state.trial {
                // The previous start never got "Keep": go back to the web engine for good.
                tracing::warn!("{}", reason);
                if let Err(e) = set_engine_setting("web") {
                    tracing::warn!("The player engine could not be set back to web: {}", e);
                }
                let reason = reason.clone();
                update_state(move |mut s| {
                    s.trial = false;
                    s.reason = Some(reason);
                    s
                });
            }
        }
        decision
    })
}

/// This run uses the embedded player (decided before the window exists).
pub fn wants_embedded_player() -> bool {
    *decision() == Start::Mpv
}

/// Called once from `setup` on the main thread. Does nothing unless this run uses mpv.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    if let Ok(dir) = app.path().resource_dir() {
        let _ = SHADER_DIR.set(dir.join("resources/anime4k"));
    }
    match decision() {
        Start::Web { reason: None } => return,
        Start::Web { reason: Some(reason) } => return set_status(Err(reason.clone())),
        Start::Mpv => {}
    }
    if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_ok_and(|v| v != "0") {
        return set_status(Err("WebKit's DMA-BUF renderer is turned off (WEBKIT_DISABLE_DMABUF_RENDERER), \
            so the page could not be drawn over the video"
            .to_string()));
    }
    match start(app) {
        Ok(()) => {
            set_status(Ok(()));
            begin_confirmation();
        }
        Err(e) => give_up(e),
    }
}

/// Embedded mpv failed after this run kept WebKit's DMA-BUF renderer on for it. Later starts use
/// the web player (Settings shows why); when Anix itself kept the renderer on, it restarts now
/// with it off, since on some drivers the window stays blank with it on.
fn give_up(error: String) {
    ASK.store(false, Ordering::SeqCst);
    set_status(Err(error.clone()));
    if let Err(e) = set_engine_setting("web") {
        return tracing::warn!("The player engine could not be set back to web: {}", e);
    }
    let reason = format!("Embedded mpv could not start, so Anix went back to the web player: {}", error);
    update_state(move |mut s| {
        s.trial = false;
        s.reason = Some(reason);
        s
    });
    emit_status_changed();
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        restart();
    }
}

fn main_window_shown() -> bool {
    APP.get()
        .and_then(|app| app.get_webview_window("main"))
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// Asks the page for "Keep" when this libmpv/WebKit pair is not confirmed yet, and goes back to
/// the web player by itself if no answer comes (the window may be blank). A window started to
/// the tray cannot be answered, so the question waits for it and the deadline only counts
/// while it is shown.
fn begin_confirmation() {
    let Some(fingerprint) = FINGERPRINT.get() else { return };
    if !policy::needs_confirmation(&load_state(), fingerprint) {
        return;
    }
    let _ = std::thread::Builder::new().name("embedded-mpv-confirm".into()).spawn(|| {
        let mut asked = false;
        let mut left = CONFIRM_DEADLINE.as_secs();
        while left > 0 {
            std::thread::sleep(Duration::from_secs(1));
            let running = matches!(*STATUS.lock().unwrap_or_else(|p| p.into_inner()), Some(Ok(())));
            if !running || (asked && !ASK.load(Ordering::SeqCst)) {
                return; // mpv failed, or the question was answered
            }
            if !main_window_shown() {
                continue;
            }
            if !asked {
                asked = true;
                update_state(|mut s| {
                    s.trial = true;
                    s
                });
                ASK.store(true, Ordering::SeqCst);
                emit_status_changed();
            }
            left -= 1;
        }
        if ASK.load(Ordering::SeqCst) {
            tracing::warn!("No answer to the player check; going back to the web player");
            if let Err(e) = finish_confirmation(Answer::NoAnswer, None) {
                tracing::warn!("Going back to the web player failed: {}", e);
            }
        }
    });
}

/// Records the answer; anything but Keep sets the engine to web and restarts the app.
fn finish_confirmation(answer: Answer, route: Option<String>) -> Result<(), String> {
    if !ASK.swap(false, Ordering::SeqCst) {
        return Ok(());
    }
    let fingerprint = FINGERPRINT.get().cloned().unwrap_or_default();
    let route = route.filter(|r| policy::valid_route(r));
    if answer == Answer::Keep {
        update_state(|s| policy::answer(s, answer, &fingerprint));
        emit_status_changed();
        return Ok(());
    }
    set_engine_setting("web")?;
    update_state(|s| {
        let mut s = policy::answer(s, answer, &fingerprint);
        s.reopen = route;
        s
    });
    restart();
}

fn restart() -> ! {
    let app = APP.get().expect("the app handle is set in setup");
    app.restart()
}

fn start(app: &AppHandle) -> Result<(), String> {
    let lib = load_lib()?;

    // libmpv refuses to start under a locale whose numbers are not written the C way, and
    // GTK has applied the desktop's locale by now. The page is unaffected: it runs in WebKit's
    // own web process, whose locale this does not change.
    unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) };
    let handle = unsafe { (lib.create)() };
    if handle.is_null() {
        return Err("mpv_create failed".to_string());
    }
    let player = Player { lib, handle };
    let settings = read_settings();
    let mode = HwdecMode::from_setting(settings.get("hwdec_mode").and_then(Value::as_str));
    // The stored self-test only counts for the driver it ran on, which is known once GL is up
    // (`gl_ready`); until then copy-back is used.
    let hwdec = policy::hwdec_value(mode, false);
    *HWDEC.lock().unwrap_or_else(|p| p.into_inner()) = hwdec.to_string();
    for (name, value) in [
        ("vo", "libmpv"),
        ("hwdec", hwdec),
        ("config", "no"),
        ("load-scripts", "no"),
        ("input-default-bindings", "no"),
        ("input-vo-keyboard", "no"),
        ("terminal", "no"),
        ("idle", "yes"),
        ("sub-auto", "no"),
    ] {
        player.set_option(name, value)?;
    }
    for (name, value) in policy::cache_options(&settings, &cache_dir().unwrap_or_default()) {
        player.set_option(name, &value)?;
    }
    // Only builds with Lua have these (the LGPL build has none); off wherever they exist.
    for name in ["ytdl", "osc"] {
        let _ = player.set_option(name, "no");
    }
    player.check(unsafe { (lib.initialize)(handle) }, "mpv_initialize")?;
    for (id, (name, format)) in [
        ("time-pos", ffi::FORMAT_DOUBLE),
        ("duration", ffi::FORMAT_DOUBLE),
        ("pause", ffi::FORMAT_FLAG),
        ("paused-for-cache", ffi::FORMAT_FLAG),
        ("demuxer-cache-time", ffi::FORMAT_DOUBLE),
        ("volume", ffi::FORMAT_DOUBLE),
        ("mute", ffi::FORMAT_FLAG),
        ("speed", ffi::FORMAT_DOUBLE),
        ("sub-delay", ffi::FORMAT_DOUBLE),
        ("track-list", ffi::FORMAT_NONE),
        ("hwdec-current", ffi::FORMAT_STRING),
    ]
    .into_iter()
    .enumerate()
    {
        let n = CString::new(name).unwrap();
        player.check(unsafe { (lib.observe_property)(handle, id as u64, n.as_ptr(), format) }, name)?;
    }
    let mpv_version = player.get("mpv-version").unwrap_or_default();
    let webkit = crate::system::hwaccel::webkitgtk_version().unwrap_or_default();
    let _ = FINGERPRINT.set(policy::fingerprint(&mpv_version, &webkit));
    let _ = MPV_VERSION.set(mpv_version);
    SELFTEST_DUE.store(mode == HwdecMode::Auto, Ordering::SeqCst);

    #[cfg(target_os = "linux")]
    {
        let window = app.get_webview_window("main").ok_or("The main window is missing")?;
        layer::attach(&window, lib, handle)?;
    }
    let _ = PLAYER.set(player);
    let app = app.clone();
    std::thread::Builder::new()
        .name("embedded-mpv-events".into())
        .spawn(move || event_loop(app))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Reported by the render layer when the GL area cannot draw (no OpenGL, context errors).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn render_failed(error: String) {
    give_up(error);
}

/// Sets `hwdec` live, off the render thread (which must not call into mpv).
fn switch_hwdec(value: &'static str) {
    *HWDEC.lock().unwrap_or_else(|p| p.into_inner()) = value.to_string();
    std::thread::spawn(move || {
        if let Some(player) = PLAYER.get() {
            if let Err(e) = player.set("hwdec", value) {
                tracing::warn!("hwdec could not be set to {}: {}", value, e);
            }
        }
        emit_status_changed();
    });
}

/// The render layer saw only black for a while with full interop: copy-back for this session,
/// and the self-test runs again at the next start (its stored result stays until then).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn interop_drew_black() {
    tracing::warn!("Hardware decoding drew a black picture; this session uses copy-back now");
    switch_hwdec("auto-copy-safe");
    update_state(|mut s| {
        s.selftest_recheck = true;
        s
    });
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, json!({ "kind": "hwdec_fallback" }));
    }
}

/// The self-test finished (see `selftest.rs`). The GL driver id is only known there, so the
/// stored result is compared here too.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn selftest_finished(result: SelfTest) {
    tracing::info!("Hardware decoding self-test: {} ({}, {})", if result.ok { "passed" } else { "failed" }, result.driver, result.mpv);
    let ok = result.ok;
    update_state(move |mut s| {
        s.selftest = Some(result);
        s.selftest_recheck = false;
        s
    });
    let mode = HwdecMode::from_setting(read_settings().get("hwdec_mode").and_then(Value::as_str));
    switch_hwdec(policy::hwdec_value(mode, ok));
}

/// GL is up and `driver` is known: full interop when the stored self-test passed with this
/// driver, libmpv and app version (and no recheck is due).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gl_ready(driver: &str) {
    let state = load_state();
    let mode = HwdecMode::from_setting(read_settings().get("hwdec_mode").and_then(Value::as_str));
    let mpv = MPV_VERSION.get().map(String::as_str).unwrap_or_default();
    let ok = !state.selftest_recheck
        && policy::selftest_for(state.selftest.as_ref(), driver, mpv, env!("CARGO_PKG_VERSION")).is_some_and(|t| t.ok);
    let value = policy::hwdec_value(mode, ok);
    if value != HWDEC.lock().unwrap_or_else(|p| p.into_inner()).as_str() {
        switch_hwdec(value);
    }
}

/// Whether the self-test has to run for this driver.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn selftest_needed(driver: &str, mpv: &str) -> bool {
    let mode = HwdecMode::from_setting(read_settings().get("hwdec_mode").and_then(Value::as_str));
    policy::selftest_due(&load_state(), mode, driver, mpv, env!("CARGO_PKG_VERSION"))
}

fn event_loop(app: AppHandle) {
    let Some(player) = PLAYER.get() else { return };
    let mut last_time = Instant::now() - Duration::from_secs(1);
    loop {
        let event = unsafe { &*(player.lib.wait_event)(player.handle, -1.0) };
        let payload = match event.event_id {
            ffi::EVENT_SHUTDOWN => break,
            ffi::EVENT_FILE_LOADED => {
                PLAYING.store(true, Ordering::SeqCst);
                json!({ "kind": "loaded", "tracks": player.tracks() })
            }
            ffi::EVENT_SEEK => json!({ "kind": "seeking" }),
            ffi::EVENT_PLAYBACK_RESTART => json!({ "kind": "seeked" }),
            ffi::EVENT_END_FILE if !event.data.is_null() => {
                PLAYING.store(false, Ordering::SeqCst);
                let end = unsafe { &*(event.data as *const ffi::EventEndFile) };
                match end.reason {
                    ffi::END_FILE_REASON_EOF => json!({ "kind": "end", "reason": "eof" }),
                    ffi::END_FILE_REASON_ERROR => {
                        json!({ "kind": "end", "reason": "error", "error": player.lib.error_text(end.error) })
                    }
                    _ => continue,
                }
            }
            ffi::EVENT_PROPERTY_CHANGE if !event.data.is_null() => {
                let property = unsafe { &*(event.data as *const ffi::EventProperty) };
                let name = unsafe { CStr::from_ptr(property.name) }.to_string_lossy();
                if name == "track-list" {
                    json!({ "kind": "tracks", "tracks": player.tracks() })
                } else if name == "hwdec-current" {
                    let value = if property.format == ffi::FORMAT_STRING && !property.data.is_null() {
                        let text = unsafe { *(property.data as *const *const std::ffi::c_char) };
                        (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned())
                    } else {
                        None
                    };
                    INTEROP.store(value.as_deref().is_some_and(policy::is_interop), Ordering::SeqCst);
                    continue;
                } else {
                    let value = match (property.format, property.data.is_null()) {
                        (_, true) => Value::Null,
                        (ffi::FORMAT_DOUBLE, false) => json!(unsafe { *(property.data as *const f64) }),
                        (ffi::FORMAT_FLAG, false) => json!(unsafe { *(property.data as *const c_int) } != 0),
                        _ => Value::Null,
                    };
                    // The playhead changes every frame; the page needs a few updates a second.
                    if name == "time-pos" {
                        if let Some(t) = value.as_f64() {
                            TIME_POS.store(t.to_bits(), Ordering::Relaxed);
                        }
                        if last_time.elapsed() < Duration::from_millis(200) {
                            continue;
                        }
                        last_time = Instant::now();
                    }
                    json!({ "kind": "property", "name": name, "value": value })
                }
            }
            _ => continue,
        };
        let _ = app.emit(EVENT, payload);
    }
}

#[derive(Debug, Serialize)]
pub struct EngineStatus {
    /// This run was meant to use the embedded player.
    pub requested: bool,
    pub available: bool,
    pub error: Option<String>,
    /// The page asks "Is the video player showing correctly?" now.
    pub ask_confirm: bool,
    /// Why the web player is used although embedded mpv was chosen before.
    pub reason: Option<String>,
    /// mpv's `hwdec` value in use.
    pub hwdec: String,
    pub selftest: Option<SelfTest>,
    /// Engine changes apply after a restart; this is the engine of the running app.
    pub running_engine: &'static str,
}

#[tauri::command]
pub async fn embedded_mpv_status() -> EngineStatus {
    let state = load_state();
    let status = STATUS.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let (requested, available, error) = match status {
        None => (false, false, None),
        Some(Ok(())) => (true, true, None),
        Some(Err(e)) => (true, false, Some(e)),
    };
    EngineStatus {
        requested,
        available,
        error,
        ask_confirm: available && ASK.load(Ordering::SeqCst),
        reason: state.reason,
        hwdec: HWDEC.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        selftest: state.selftest,
        running_engine: if available { "mpv" } else { "web" },
    }
}

/// Async so it runs off the main thread, which renders the video and must not call into mpv.
#[tauri::command]
pub async fn embedded_mpv(action: Action) -> Result<(), String> {
    let player = PLAYER.get().ok_or("The embedded player is not running")?;
    let proxy_base = crate::stream_proxy::start()?;
    let ops = plan(&action, &proxy_base)?;
    match &action {
        // Buffer and cache settings changed in Settings apply from the next file on.
        Action::Load { .. } => {
            for (name, value) in policy::cache_options(&read_settings(), &cache_dir().unwrap_or_default()) {
                if let Err(e) = player.set(name, &value) {
                    tracing::warn!("{}", e);
                }
            }
        }
        Action::SetVideoRect { x, y, width, height, viewport_width, viewport_height } => {
            *MARGINS.lock().unwrap_or_else(|p| p.into_inner()) =
                margins(*x, *y, *width, *height, *viewport_width, *viewport_height)?;
        }
        _ => {}
    }
    for op in ops {
        player.apply(&op)?;
    }
    Ok(())
}

/// The answer to "Is the video player showing correctly?". Anything but Keep restarts the app
/// on the web player and reopens `route`.
#[tauri::command]
pub async fn embedded_mpv_answer(answer: Answer, route: Option<String>) -> Result<(), String> {
    finish_confirmation(answer, route)
}

/// Settings changed the engine: a new switch to mpv is asked about again after the restart.
#[tauri::command]
pub async fn embedded_mpv_engine_changed(engine: String) {
    update_state(|s| policy::engine_changed(s, &engine));
}

/// Restarts Anix and reopens `route` (an in-app path) once it is back.
#[tauri::command]
pub async fn embedded_mpv_restart(route: Option<String>) -> Result<(), String> {
    let route = route.filter(|r| policy::valid_route(r));
    update_state(move |mut s| {
        s.reopen = route;
        s
    });
    restart();
}

/// The route to reopen after a restart Anix asked for, handed out once.
#[tauri::command]
pub async fn embedded_mpv_take_route() -> Option<String> {
    let mut route = None;
    update_state(|mut s| {
        route = s.reopen.take();
        s
    });
    route
}

/// Empties the on-disk video cache folder; returns the bytes freed.
#[tauri::command]
pub async fn embedded_mpv_clear_cache() -> Result<u64, String> {
    let Some(dir) = cache_dir() else { return Ok(0) };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(0) };
    let mut freed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        // Only plain files of mpv's cache; nothing is followed out of the folder.
        if meta.is_file() && std::fs::remove_file(&path).is_ok() {
            freed += meta.len();
        }
    }
    Ok(freed)
}
