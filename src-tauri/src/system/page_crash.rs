//! Recovery when WebKit's web process, the one that runs the page, dies.
//!
//! WebKitGTK does not reload a crashed page by itself, and the window has no system title bar
//! (the page draws it), so without this the window stays blank and embedded mpv keeps playing
//! with no controls. On a crash this stops mpv, switches to embedded mpv when it was the web
//! player that crashed and this build can run mpv (`embedded_mpv::after_page_crash`, which
//! restarts the app), and otherwise reloads the page. The app uses hash routes, so a reload
//! brings back the same screen. A page that keeps crashing is not reloaded endlessly.

use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Manager};

/// Crashes within one minute that still get a reload; one more means a crash loop.
pub const RELOADS_PER_MINUTE: usize = 3;
const LOOP_WINDOW_S: u64 = 60;
const MAIN_WINDOW: &str = "main";
/// `WEBKIT_WEB_PROCESS_TERMINATED_BY_API`: an intended stop, not a crash.
#[cfg(target_os = "linux")]
const TERMINATED_BY_API: i32 = 2;

static STARTED: Lazy<Instant> = Lazy::new(Instant::now);
/// Seconds since `STARTED` of the recent crashes.
static CRASHES: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// Records a crash at `now_s` (seconds of a monotonic clock) and says whether to reload the
/// page: only while there were at most [`RELOADS_PER_MINUTE`] crashes in the last minute.
pub fn allow_reload(crashes: &mut Vec<u64>, now_s: u64) -> bool {
    crashes.retain(|at| now_s.saturating_sub(*at) < LOOP_WINDOW_S);
    crashes.push(now_s);
    crashes.len() <= RELOADS_PER_MINUTE
}

/// The in-app route of a page URL (`tauri://localhost/#/watch/1` → `/watch/1`); `None` for the
/// start page and anything that is not an in-app route.
pub fn route_from_url(url: &str) -> Option<String> {
    let (_, route) = url.split_once('#')?;
    (route != "/" && crate::embedded_mpv::policy::valid_route(route)).then(|| route.to_string())
}

/// Connects to the main webview's `web-process-terminated` signal. Called once from `setup`.
#[cfg(target_os = "linux")]
pub fn watch(app: &AppHandle) {
    use gtk::glib::prelude::ObjectExt;
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else { return };
    Lazy::force(&STARTED);
    let handle = app.clone();
    let connected = window.with_webview(move |webview| {
        webview.inner().connect_local("web-process-terminated", false, move |args| {
            // The reason is an enum; read as a number so no WebKit bindings are needed here.
            let reason = args
                .get(1)
                .and_then(|v| v.transform::<i32>().ok())
                .and_then(|v| v.get::<i32>().ok());
            if reason != Some(TERMINATED_BY_API) {
                let app = handle.clone();
                // Off the GTK main thread: stopping mpv and saving notices must not block it.
                let _ = std::thread::Builder::new().name("page-crash".into()).spawn(move || recover(&app, reason));
            }
            None
        });
    });
    if let Err(e) = connected {
        tracing::warn!("The page crash handler could not be connected: {}", e);
    }
}

fn recover(app: &AppHandle, reason: Option<i32>) {
    // 0 = crashed, 1 = exceeded the memory limit (WebKitWebProcessTerminationReason).
    tracing::error!("The page's web process stopped (WebKit reason {:?}); recovering", reason);
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else { return };
    let route = window.url().ok().and_then(|url| route_from_url(url.as_str()));
    // Does not return when it switches to embedded mpv and restarts.
    crate::embedded_mpv::after_page_crash(route);

    let allowed = allow_reload(&mut CRASHES.lock().unwrap_or_else(|p| p.into_inner()), STARTED.elapsed().as_secs());
    if !allowed {
        tracing::error!("The page keeps crashing, so it is not reloaded again");
        return show_crash_loop_dialog(app);
    }
    crate::notices::push_translated(app, crate::notices::PLAYER, "page_reloaded_title", "page_reloaded_body", None);
    if let Err(e) = window.reload() {
        tracing::warn!("The page could not be reloaded after the crash: {}", e);
    }
}

/// The window is blank and has no title bar, so a native dialog explains it, and closing it
/// quits rather than leaving a window nobody can use.
fn show_crash_loop_dialog(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let lang = crate::i18n::current();
    let handle = app.clone();
    app.dialog()
        .message(crate::i18n::text(lang, "page_crash_loop"))
        .title("Anix")
        .kind(MessageDialogKind::Error)
        .show(move |_| handle.exit(1));
}
