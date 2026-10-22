//! The tray icon and background mode.
//!
//! With "Keep running in the tray" on, closing the main window (the title bar's close button
//! or the desktop's) hides it instead of quitting; the tray menu brings it back, lists the next
//! airing episodes of belled anime and quits. Release notifications keep running meanwhile.
//! Closing quits as before when the desktop has no tray to show the icon in, so the window can
//! never be lost.

use crate::db;
use crate::notifier::{ScheduleSource, OPEN_ANIME_EVENT};
use crate::state::AppState;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

pub const TRAY_ID: &str = "anix-tray";
const MAIN_WINDOW: &str = "main";
const MENU_OPEN: &str = "tray-open";
const MENU_QUIT: &str = "tray-quit";
const MENU_ANIME_PREFIX: &str = "tray-anime:";
/// How many upcoming episodes the menu lists.
const UPCOMING_COUNT: usize = 6;
/// How far ahead the menu looks.
const UPCOMING_WINDOW_S: i64 = 7 * 24 * 3600;
const UPCOMING_REFRESH: Duration = Duration::from_secs(15 * 60);

/// The setting, as last applied.
static ENABLED: AtomicBool = AtomicBool::new(false);
static REFRESHER_STARTED: AtomicBool = AtomicBool::new(false);
/// The upcoming episodes the menu was last built with.
static UPCOMING: Lazy<Mutex<Vec<Upcoming>>> = Lazy::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upcoming {
    pub media_id: i64,
    pub title: String,
    pub episode: i32,
    pub airing_at: i64,
}

/// Library names the tray backend (libappindicator) loads at run time; without one of them
/// creating a tray icon aborts the process, so it is looked for first.
const INDICATOR_LIBS: [&str; 4] = [
    "libayatana-appindicator3.so.1",
    "libappindicator3.so.1",
    "libayatana-appindicator3.so",
    "libappindicator3.so",
];

/// Whether an AppIndicator library can be loaded.
pub fn indicator_library() -> Option<&'static str> {
    INDICATOR_LIBS.into_iter().find(|name| {
        let Ok(c_name) = std::ffi::CString::new(*name) else {
            return false;
        };
        // SAFETY: dlopen/dlclose with a valid C string; the handle is closed right away.
        unsafe {
            let handle = libc::dlopen(c_name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
            if handle.is_null() {
                false
            } else {
                libc::dlclose(handle);
                true
            }
        }
    })
}

/// Whether the session has a StatusNotifier host, which is what shows tray icons (KDE, most
/// panels; GNOME only with the AppIndicator extension).
pub fn tray_host_present() -> bool {
    let check = || -> zbus::Result<bool> {
        let connection = zbus::blocking::Connection::session()?;
        let proxy = zbus::blocking::fdo::DBusProxy::new(&connection)?;
        let name = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher")?;
        Ok(proxy.name_has_owner(name)?)
    };
    check().unwrap_or(false)
}

/// Whether closing the window should hide it right now.
fn hide_on_close() -> bool {
    ENABLED.load(Ordering::SeqCst) && tray_host_present()
}

/// Brings the main window back and to the front.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Hides the main window when the app was started from the autostart entry and the tray is on.
pub fn hide_if_started_minimised(app: &AppHandle, args: &[String]) {
    let asked = args.iter().any(|arg| arg == super::MINIMIZED_ARG);
    if asked && ENABLED.load(Ordering::SeqCst) && tray_host_present() {
        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
            let _ = window.hide();
        }
    }
}

/// The window-event hook: a close request on the main window hides it while the tray is on.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    match event {
        WindowEvent::CloseRequested { api, .. } if hide_on_close() => {
            api.prevent_close();
            let _ = window.hide();
        }
        // Hidden helper windows (site sessions) would otherwise keep the process alive.
        WindowEvent::Destroyed => window.app_handle().exit(0),
        _ => {}
    }
}

/// Switches the tray on or off. Must not be called while the event loop is blocked.
pub fn set_enabled(app: &AppHandle, enabled: bool) {
    let enabled = enabled && indicator_library().is_some();
    ENABLED.store(enabled, Ordering::SeqCst);
    let existing = app.tray_by_id(TRAY_ID);
    match (enabled, existing) {
        (true, None) => {
            if let Err(e) = build(app) {
                tracing::warn!("The tray icon could not be created: {}", e);
                ENABLED.store(false, Ordering::SeqCst);
                return;
            }
            start_refresher(app.clone());
        }
        (false, Some(_)) => {
            app.remove_tray_by_id(TRAY_ID);
        }
        _ => {}
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = menu(app, &UPCOMING.lock().unwrap_or_else(|p| p.into_inner()))?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Anix")
        .menu(&menu)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()));
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        MENU_OPEN => show_main_window(app),
        MENU_QUIT => app.exit(0),
        other => {
            if let Some(media_id) = other.strip_prefix(MENU_ANIME_PREFIX).and_then(|id| id.parse::<i64>().ok()) {
                show_main_window(app);
                if let Err(e) = app.emit_to(MAIN_WINDOW, OPEN_ANIME_EVENT, media_id) {
                    tracing::warn!("Could not open anime {} from the tray: {}", media_id, e);
                }
            }
        }
    }
}

/// The menu label of one upcoming episode, with its local air time.
pub fn upcoming_label(item: &Upcoming, now: i64) -> String {
    let title: String = if item.title.chars().count() > 40 {
        format!("{}…", item.title.chars().take(39).collect::<String>())
    } else {
        item.title.clone()
    };
    let episode = crate::i18n::fill(crate::i18n::current(), "tray_episode", &[("episode", &item.episode.to_string())]);
    format!("{} · {} · {}", title, episode, when_label(item.airing_at, now))
}

/// "today 18:30", "tomorrow 09:00" or "Tue 18:30", in local time.
fn when_label(at: i64, now: i64) -> String {
    let (Some(at_tm), Some(now_tm)) = (local_tm(at), local_tm(now)) else {
        return String::new();
    };
    let time = format!("{:02}:{:02}", at_tm.tm_hour, at_tm.tm_min);
    let same_year = at_tm.tm_year == now_tm.tm_year;
    let lang = crate::i18n::current();
    if same_year && at_tm.tm_yday == now_tm.tm_yday {
        crate::i18n::fill(lang, "today", &[("time", &time)])
    } else if same_year && at_tm.tm_yday == now_tm.tm_yday + 1 {
        crate::i18n::fill(lang, "tomorrow", &[("time", &time)])
    } else {
        format!("{} {}", crate::i18n::weekday(lang, at_tm.tm_wday.rem_euclid(7) as usize), time)
    }
}

fn local_tm(at: i64) -> Option<libc::tm> {
    let t: libc::time_t = at as libc::time_t;
    // SAFETY: localtime_r writes into the zeroed struct it is given.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            None
        } else {
            Some(tm)
        }
    }
}

fn menu(app: &AppHandle, upcoming: &[Upcoming]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    let lang = crate::i18n::current();
    menu.append(&MenuItem::with_id(app, MENU_OPEN, crate::i18n::text(lang, "tray_open"), true, None::<&str>)?)?;
    if !upcoming.is_empty() {
        let now = db::current_unix_timestamp();
        let submenu = Submenu::new(app, crate::i18n::text(lang, "tray_airing_next"), true)?;
        for item in upcoming {
            let id = format!("{}{}", MENU_ANIME_PREFIX, item.media_id);
            submenu.append(&MenuItem::with_id(app, id, upcoming_label(item, now), true, None::<&str>)?)?;
        }
        menu.append(&submenu)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, MENU_QUIT, crate::i18n::text(lang, "tray_quit"), true, None::<&str>)?)?;
    Ok(menu)
}

/// The next airings of belled anime, soonest first.
pub fn pick_upcoming(
    airings: &[crate::notifier::Airing],
    bells: &[crate::notifier::Bell],
    now: i64,
    count: usize,
) -> Vec<Upcoming> {
    let mut list: Vec<Upcoming> = airings
        .iter()
        .filter(|a| a.airing_at > now)
        .filter_map(|a| {
            let bell = bells.iter().find(|b| b.media_id == a.media_id)?;
            Some(Upcoming {
                media_id: a.media_id,
                title: bell.media_title.clone(),
                episode: a.episode,
                airing_at: a.airing_at,
            })
        })
        .collect();
    list.sort_by_key(|item| (item.airing_at, item.media_id, item.episode));
    list.dedup_by_key(|item| (item.media_id, item.episode));
    list.truncate(count);
    list
}

/// Keeps the "Airing next" list current while the tray is on.
fn start_refresher(app: AppHandle) {
    if REFRESHER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            if ENABLED.load(Ordering::SeqCst) {
                refresh_upcoming(&app).await;
            }
            tokio::select! {
                _ = tokio::time::sleep(UPCOMING_REFRESH) => {}
                _ = crate::commands::schedule::BELLS_CHANGED_FOR_TRAY.notified() => {}
            }
        }
    });
}

async fn refresh_upcoming(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let bells = match db::list_notification_bells(&state.pool).await {
        Ok(bells) => bells,
        Err(e) => {
            tracing::warn!("The tray cannot read the bells: {}", e);
            return;
        }
    };
    let now = db::current_unix_timestamp();
    let upcoming = if bells.is_empty() {
        Vec::new()
    } else {
        let ids: Vec<i64> = bells.iter().map(|b| b.media_id).collect();
        // Through the catalogue, so the list keeps working while AniList is down and also covers
        // anime with only a stand-in id.
        match state.catalogue.fetch_marked(&ids, now, now + UPCOMING_WINDOW_S).await {
            Ok(marked) => pick_upcoming(&marked.airings, &bells, now, UPCOMING_COUNT),
            // Offline: keep what the menu shows.
            Err(_) => return,
        }
    };
    *UPCOMING.lock().unwrap_or_else(|p| p.into_inner()) = upcoming;
    // Rebuilt every time: the "today"/"tomorrow" labels change with the clock.
    relabel(app);
}

/// Rebuilds the menu from the last "Airing next" list, e.g. after the interface language changed.
pub fn relabel(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let upcoming = UPCOMING.lock().unwrap_or_else(|p| p.into_inner()).clone();
        match menu(app, &upcoming) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => tracing::warn!("The tray menu could not be rebuilt: {}", e),
        }
    }
}
