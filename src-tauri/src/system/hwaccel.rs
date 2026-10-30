//! Read-only hardware-acceleration diagnostics for Settings › System: VA-API (via `vainfo`),
//! the GStreamer plugins the webview can load, the WebKitGTK version, the display server and
//! the DMA-BUF renderer workaround. Nothing here changes the system.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

const VAINFO_TIMEOUT: Duration = Duration::from_secs(6);

/// 0 = not recorded, 1 = the app set `WEBKIT_DISABLE_DMABUF_RENDERER`, 2 = it was already set.
static DMABUF_SET_BY: AtomicU8 = AtomicU8::new(0);

/// Called from `main` before the webview starts: whether the app set the variable itself.
pub fn note_dmabuf_workaround(set_by_app: bool) {
    DMABUF_SET_BY.store(if set_by_app { 1 } else { 2 }, Ordering::SeqCst);
}

const DMABUF_VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
/// Set next to `WEBKIT_DISABLE_DMABUF_RENDERER=1` when Anix set that itself. A restart inherits
/// the environment, and without the mark the new run took Anix's value for the user's and so
/// refused embedded mpv after "Restart now".
const DMABUF_MARK: &str = "ANIX_DMABUF_SET_BY_APP";

/// What `main` does with `WEBKIT_DISABLE_DMABUF_RENDERER` before WebKit starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmabufEnv {
    /// Turn the renderer off (it leaves the window blank on some drivers) and mark it as Anix's.
    Disable,
    /// An earlier run of Anix turned it off and this run uses mpv, which needs it: remove both.
    Clear,
    /// Unset, and embedded mpv needs the renderer on.
    Untouched,
    /// The user's own value: never changed.
    User,
}

/// The rule for [`apply_dmabuf_workaround`], from the variable, its mark and the engine.
pub fn dmabuf_env(value: Option<&str>, mark: Option<&str>, wants_mpv: bool) -> DmabufEnv {
    let set_by_app = match value {
        None => true,
        Some(v) => v == "1" && mark == Some("1"),
    };
    match (set_by_app, wants_mpv) {
        (false, _) => DmabufEnv::User,
        (true, false) => DmabufEnv::Disable,
        (true, true) if value.is_some() => DmabufEnv::Clear,
        (true, true) => DmabufEnv::Untouched,
    }
}

/// The user turned WebKit's DMA-BUF renderer off themselves, which embedded mpv cannot run with.
pub fn dmabuf_turned_off_by_user() -> bool {
    let value = std::env::var(DMABUF_VAR).ok();
    let mark = std::env::var(DMABUF_MARK).ok();
    dmabuf_off_by_user(value.as_deref(), mark.as_deref())
}

/// The rule for [`dmabuf_turned_off_by_user`]: a user value other than "0", which keeps the
/// renderer on (the same check `embedded_mpv::install` makes).
pub fn dmabuf_off_by_user(value: Option<&str>, mark: Option<&str>) -> bool {
    dmabuf_env(value, mark, true) == DmabufEnv::User && value != Some("0")
}

/// Called once from `main`, before any thread or the webview starts.
pub fn apply_dmabuf_workaround(wants_mpv: bool) {
    let value = std::env::var(DMABUF_VAR).ok();
    let mark = std::env::var(DMABUF_MARK).ok();
    match dmabuf_env(value.as_deref(), mark.as_deref(), wants_mpv) {
        DmabufEnv::Disable => {
            std::env::set_var(DMABUF_VAR, "1");
            std::env::set_var(DMABUF_MARK, "1");
            note_dmabuf_workaround(true);
        }
        DmabufEnv::Clear => {
            std::env::remove_var(DMABUF_VAR);
            std::env::remove_var(DMABUF_MARK);
        }
        DmabufEnv::Untouched => {}
        DmabufEnv::User => note_dmabuf_workaround(false),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct VaInfo {
    pub vaapi_version: Option<String>,
    pub driver: Option<String>,
    /// Codecs with a decode entrypoint (`VAEntrypointVLD`), in a fixed order: H.264, HEVC,
    /// VP8, VP9, AV1, MPEG-2, VC-1, JPEG.
    pub decode: Vec<String>,
    /// Every profile/entrypoint line, as `VAProfileH264Main: VAEntrypointVLD`.
    pub profiles: Vec<String>,
}

const CODECS: [(&str, &str); 8] = [
    ("VAProfileH264", "H.264"),
    ("VAProfileHEVC", "HEVC"),
    ("VAProfileVP8", "VP8"),
    ("VAProfileVP9", "VP9"),
    ("VAProfileAV1", "AV1"),
    ("VAProfileMPEG2", "MPEG-2"),
    ("VAProfileVC1", "VC-1"),
    ("VAProfileJPEG", "JPEG"),
];

/// Reads `vainfo` output (stdout and stderr together). `None` when no driver was initialised.
pub fn parse_vainfo(output: &str) -> Option<VaInfo> {
    let mut info = VaInfo::default();
    for line in output.lines() {
        let text = line.trim();
        let body = text.strip_prefix("vainfo:").map(str::trim).unwrap_or(text);
        if let Some(rest) = body.strip_prefix("VA-API version:") {
            info.vaapi_version = Some(rest.trim().to_string());
        } else if let Some(rest) = body.strip_prefix("Driver version:") {
            info.driver = Some(rest.trim().to_string());
        } else if body.starts_with("VAProfile") {
            let mut parts = body.splitn(2, ':');
            let profile = parts.next().unwrap_or_default().trim();
            let entry = parts.next().unwrap_or_default().trim();
            if profile.is_empty() || !entry.starts_with("VAEntrypoint") {
                continue;
            }
            info.profiles.push(format!("{}: {}", profile, entry));
            if entry == "VAEntrypointVLD" {
                if let Some((_, codec)) = CODECS.iter().find(|(prefix, _)| profile.starts_with(prefix)) {
                    if !info.decode.iter().any(|known| known == codec) {
                        info.decode.push(codec.to_string());
                    }
                }
            }
        }
    }
    let order = |codec: &String| CODECS.iter().position(|(_, name)| name == codec).unwrap_or(usize::MAX);
    info.decode.sort_by_key(order);
    if info.driver.is_none() && info.profiles.is_empty() {
        None
    } else {
        Some(info)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct VaapiCheck {
    pub vainfo_installed: bool,
    pub ok: bool,
    pub info: Option<VaInfo>,
    /// The last lines of `vainfo` when it found no driver.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct GstPlugins {
    /// The folders searched, in order.
    pub dirs: Vec<String>,
    /// The new `va` plugin (vah264dec and friends).
    pub va: Option<String>,
    /// The older gstreamer-vaapi plugin.
    pub vaapi: Option<String>,
    /// Software decoders (FFmpeg).
    pub libav: Option<String>,
    /// The plugins come from the AppImage rather than the system.
    pub bundled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct HardwareReport {
    /// `wayland`, `x11` or `unknown`.
    pub session: String,
    pub wayland_display: Option<String>,
    pub x11_display: Option<String>,
    pub xdg_session_type: Option<String>,
    pub desktop: Option<String>,
    pub webkitgtk_version: Option<String>,
    pub dmabuf_disabled: bool,
    pub dmabuf_value: Option<String>,
    /// The app set the workaround itself (rather than the environment it was started from).
    pub dmabuf_set_by_app: bool,
    /// `intel`, `amd`, `nvidia`, another PCI vendor id, or none when no render node exists.
    pub gpu_vendor: Option<String>,
    pub render_node: Option<String>,
    pub libva_driver_name: Option<String>,
    pub running_from_appimage: bool,
    pub vaapi: VaapiCheck,
    pub gstreamer: GstPlugins,
    pub tips: Vec<String>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `wayland`, `x11` or `unknown` from the session variables.
pub fn session_kind(
    gdk_backend: Option<&str>,
    wayland_display: Option<&str>,
    session_type: Option<&str>,
    display: Option<&str>,
) -> &'static str {
    match gdk_backend {
        Some(b) if b.starts_with("x11") && display.is_some() => return "x11",
        Some(b) if b.starts_with("wayland") && wayland_display.is_some() => return "wayland",
        _ => {}
    }
    if wayland_display.is_some() {
        "wayland"
    } else if session_type == Some("wayland") {
        "wayland"
    } else if display.is_some() || session_type == Some("x11") {
        "x11"
    } else {
        "unknown"
    }
}

/// WebKitGTK's own version, read from the library loaded into this process.
pub(crate) fn webkitgtk_version() -> Option<String> {
    type VersionFn = unsafe extern "C" fn() -> libc::c_uint;
    let lookup = |name: &str| -> Option<VersionFn> {
        let c_name = std::ffi::CString::new(name).ok()?;
        // SAFETY: looks a symbol up in the libraries already loaded; the WebKitGTK version
        // functions take no arguments and return a guint.
        unsafe {
            let ptr = libc::dlsym(libc::RTLD_DEFAULT, c_name.as_ptr());
            (!ptr.is_null()).then(|| std::mem::transmute::<*mut libc::c_void, VersionFn>(ptr))
        }
    };
    let major = lookup("webkit_get_major_version")?;
    let minor = lookup("webkit_get_minor_version")?;
    let micro = lookup("webkit_get_micro_version")?;
    // SAFETY: see above.
    unsafe { Some(format!("{}.{}.{}", major(), minor(), micro())) }
}

fn gpu(render_node: &Path) -> Option<String> {
    let name = render_node.file_name()?;
    let vendor = std::fs::read_to_string(Path::new("/sys/class/drm").join(name).join("device/vendor")).ok()?;
    Some(
        match vendor.trim() {
            "0x8086" => "intel",
            "0x1002" => "amd",
            "0x10de" => "nvidia",
            other => return Some(other.to_string()),
        }
        .to_string(),
    )
}

fn first_render_node() -> Option<PathBuf> {
    let mut nodes: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.file_name().and_then(|n| n.to_str()).map_or(false, |n| n.starts_with("renderD")))
        .collect();
    nodes.sort();
    nodes.into_iter().next()
}

/// The folders GStreamer loads plugins from, as the environment gives them.
pub fn gst_plugin_dirs(
    plugin_path: Option<&str>,
    system_path: Option<&str>,
    defaults: &[&str],
) -> Vec<PathBuf> {
    let split = |value: &str| -> Vec<PathBuf> {
        value.split(':').filter(|p| !p.is_empty()).map(PathBuf::from).collect()
    };
    let mut dirs = plugin_path.map(split).unwrap_or_default();
    match system_path {
        Some(system) => dirs.extend(split(system)),
        None => dirs.extend(defaults.iter().map(PathBuf::from)),
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|dir| seen.insert(dir.clone()));
    dirs
}

const DEFAULT_GST_DIRS: [&str; 5] = [
    "/usr/lib64/gstreamer-1.0",
    "/usr/lib/x86_64-linux-gnu/gstreamer-1.0",
    "/usr/lib/aarch64-linux-gnu/gstreamer-1.0",
    "/usr/lib/gstreamer-1.0",
    "/usr/local/lib/gstreamer-1.0",
];

fn find_plugin(dirs: &[PathBuf], file: &str) -> Option<String> {
    dirs.iter()
        .map(|dir| dir.join(file))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

pub fn gst_plugins() -> GstPlugins {
    let plugin_path = env("GST_PLUGIN_PATH_1_0").or_else(|| env("GST_PLUGIN_PATH"));
    let system_path = env("GST_PLUGIN_SYSTEM_PATH_1_0").or_else(|| env("GST_PLUGIN_SYSTEM_PATH"));
    let dirs = gst_plugin_dirs(plugin_path.as_deref(), system_path.as_deref(), &DEFAULT_GST_DIRS);
    let appdir = env("APPDIR");
    let va = find_plugin(&dirs, "libgstva.so");
    let bundled = match (&appdir, &va) {
        (Some(appdir), Some(found)) => found.starts_with(appdir.as_str()),
        (Some(appdir), None) => dirs.iter().any(|dir| dir.starts_with(appdir)),
        _ => false,
    };
    GstPlugins {
        dirs: dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect(),
        vaapi: find_plugin(&dirs, "libgstvaapi.so"),
        libav: find_plugin(&dirs, "libgstlibav.so"),
        va,
        bundled,
    }
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

async fn run_vainfo(program: &Path, args: &[&str]) -> Option<String> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).kill_on_drop(true);
    // The AppImage's library path would make the system vainfo load the bundled libva.
    command.env_remove("LD_LIBRARY_PATH");
    let output = tokio::time::timeout(VAINFO_TIMEOUT, command.output()).await.ok()?.ok()?;
    Some(format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

pub async fn check_vaapi(render_node: Option<&Path>) -> VaapiCheck {
    let Some(program) = which("vainfo") else {
        return VaapiCheck::default();
    };
    let mut last_output = String::new();
    let mut attempts: Vec<Vec<String>> = vec![Vec::new()];
    if let Some(node) = render_node {
        attempts.push(vec![
            "--display".to_string(),
            "drm".to_string(),
            "--device".to_string(),
            node.to_string_lossy().into_owned(),
        ]);
    }
    for args in attempts {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let Some(output) = run_vainfo(&program, &args).await else {
            continue;
        };
        if let Some(info) = parse_vainfo(&output) {
            let ok = !info.decode.is_empty();
            return VaapiCheck {
                vainfo_installed: true,
                ok,
                info: Some(info),
                error: None,
            };
        }
        last_output = output;
    }
    VaapiCheck {
        vainfo_installed: true,
        ok: false,
        info: None,
        error: Some(vainfo_failure(&last_output)),
    }
}

/// The lines of a failed `vainfo` run that say what went wrong.
pub fn vainfo_failure(output: &str) -> String {
    let lines: Vec<&str> = output.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let errors: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| {
            let lower = l.to_ascii_lowercase();
            lower.contains("error") || lower.contains("failed") || lower.contains("can't") || lower.contains("cannot")
        })
        .collect();
    let picked = if errors.is_empty() { &lines[..] } else { &errors[..] };
    let start = picked.len().saturating_sub(4);
    if picked.is_empty() {
        "vainfo did not answer".to_string()
    } else {
        picked[start..].join("\n")
    }
}

/// Plain-language advice for what the report found.
pub fn tips(report: &HardwareReport) -> Vec<String> {
    let mut tips = Vec::new();
    let vendor = report.gpu_vendor.as_deref();
    let driver_tip = match vendor {
        Some("intel") => "Install Intel's VA-API driver: intel-media-driver (Fedora, from RPM Fusion; Debian/Ubuntu: intel-media-va-driver-non-free; Arch: intel-media-driver). GPUs older than Broadwell use libva-intel-driver instead.",
        Some("amd") => "Install Mesa's VA-API driver: mesa-va-drivers (Debian/Ubuntu), libva-mesa-driver (Arch). On Fedora, H.264 and HEVC decoding need mesa-va-drivers-freeworld from RPM Fusion.",
        Some("nvidia") => "With NVIDIA's own driver, install nvidia-vaapi-driver (it only decodes; set NVD_BACKEND=direct if vainfo still fails). With nouveau, install Mesa's VA-API driver.",
        _ => "Install the VA-API driver for your graphics card: intel-media-driver (Intel), mesa-va-drivers (AMD) or nvidia-vaapi-driver (NVIDIA).",
    };
    if report.render_node.is_none() {
        tips.push("No GPU render device (/dev/dri/renderD*) is visible, so video is decoded in software. Inside a sandbox or container, give it access to /dev/dri.".to_string());
    }
    if !report.vaapi.vainfo_installed {
        tips.push("Install vainfo to let Anix check VA-API (Fedora/Arch: libva-utils, Debian/Ubuntu: vainfo), then run the check again.".to_string());
    } else if report.vaapi.info.is_none() {
        tips.push(format!("VA-API is not working: no driver could be loaded. {}", driver_tip));
        if report.libva_driver_name.is_some() {
            tips.push("LIBVA_DRIVER_NAME is set in your environment; a wrong value stops VA-API from loading the right driver. Try starting Anix without it.".to_string());
        }
    } else if let Some(info) = &report.vaapi.info {
        if info.decode.is_empty() {
            tips.push(format!("VA-API is loaded but offers no video decoding. {}", driver_tip));
        } else if !info.decode.iter().any(|c| c == "H.264") {
            tips.push(format!(
                "Your VA-API driver cannot decode H.264, which most streams use, so those play in software. {}",
                driver_tip
            ));
        }
    }
    if report.gstreamer.va.is_none() && report.gstreamer.vaapi.is_none() {
        if report.running_from_appimage {
            tips.push("The GStreamer plugins Anix uses come with the AppImage, and they have no VA-API decoder, so the built-in player decodes in software. For hardware decoding use an external player such as mpv (Settings › Player).".to_string());
        } else {
            tips.push("GStreamer's VA plugin is missing, so the built-in player decodes in software. Install gstreamer1-plugins-bad-free (Fedora), gstreamer1.0-plugins-bad (Debian/Ubuntu) or gst-plugins-bad (Arch).".to_string());
        }
    }
    if report.gstreamer.libav.is_none() {
        tips.push("GStreamer's libav plugin (software decoders) was not found; some videos may not play. Install gstreamer1-libav or gstreamer1.0-libav.".to_string());
    }
    if report.dmabuf_disabled {
        tips.push(if report.dmabuf_set_by_app {
            "WebKit's DMA-BUF renderer is off: Anix turns it off because it leaves the window blank on some drivers. Video plays either way; the page may draw a little slower. To try it on, start Anix with WEBKIT_DISABLE_DMABUF_RENDERER=0.".to_string()
        } else {
            "WebKit's DMA-BUF renderer was turned off by your environment (WEBKIT_DISABLE_DMABUF_RENDERER).".to_string()
        });
    }
    if vendor == Some("nvidia") && report.session == "wayland" {
        tips.push("NVIDIA on Wayland: if the window turns blank or flickers, keep the DMA-BUF renderer off (the default) or try an X11 session.".to_string());
    }
    if tips.is_empty() {
        tips.push("Hardware decoding looks ready. If playback still stutters, try an external player (mpv) or a lower quality.".to_string());
    }
    tips
}

pub async fn report() -> HardwareReport {
    let wayland_display = env("WAYLAND_DISPLAY");
    let x11_display = env("DISPLAY");
    let xdg_session_type = env("XDG_SESSION_TYPE");
    let dmabuf_value = std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").ok();
    let render_node = first_render_node();
    let mut report = HardwareReport {
        session: session_kind(env("GDK_BACKEND").as_deref(), wayland_display.as_deref(), xdg_session_type.as_deref(), x11_display.as_deref()).to_string(),
        wayland_display,
        x11_display,
        xdg_session_type,
        desktop: env("XDG_CURRENT_DESKTOP"),
        webkitgtk_version: webkitgtk_version(),
        dmabuf_disabled: dmabuf_value.as_deref().map_or(false, |v| v != "0"),
        dmabuf_value,
        dmabuf_set_by_app: DMABUF_SET_BY.load(Ordering::SeqCst) == 1,
        gpu_vendor: render_node.as_deref().and_then(gpu),
        render_node: render_node.as_ref().map(|p| p.to_string_lossy().into_owned()),
        libva_driver_name: env("LIBVA_DRIVER_NAME"),
        running_from_appimage: env("APPIMAGE").is_some(),
        vaapi: check_vaapi(render_node.as_deref()).await,
        gstreamer: tokio::task::spawn_blocking(gst_plugins).await.unwrap_or_default(),
        tips: Vec::new(),
    };
    report.tips = tips(&report);
    report
}
