//! The sign-in window: a separate webview that shows the service's own login page and is
//! watched for the registered localhost redirect. Comick does not allow its login page in an
//! embedded webview; its sign-in runs in the system browser (see `loopback`).
//!
//! The window loads a remote site. It is not listed in any capability, and Tauri refuses IPC
//! from remote origins that no capability allows, so the page cannot call the app's commands.

use super::oauth::{self, Redirect, Service};
use super::store::Profile;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const LOGIN_EVENT: &str = "account-login";

pub fn window_label(service: Service) -> &'static str {
    match service {
        Service::AniList => "login-anilist",
        Service::Mal => "login-mal",
        Service::Comick => "login-comick",
    }
}

/// Payload of `account-login`: how a sign-in window ended.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LoginResult {
    pub service: Service,
    /// `connected`, `cancelled` or `failed`.
    pub outcome: &'static str,
    pub name: Option<String>,
    pub message: Option<String>,
}

struct Attempt {
    service: Service,
    state: String,
    code_verifier: Option<String>,
    /// Set by whichever comes first: a redirect being handled or the window going away.
    finished: AtomicBool,
}

impl Attempt {
    /// Navigation hook. Returns whether the webview may go to `url`.
    fn on_navigation(self: &Arc<Self>, app: &AppHandle, url: &Url) -> bool {
        match oauth::parse_redirect(self.service, url) {
            Redirect::Elsewhere => true,
            // The redirect without anything in it: let it load, the page-load hook reads the
            // address again once WebKit has settled on it.
            Redirect::Empty => {
                tracing::info!(
                    "{} sign-in: redirect without a result at navigation, waiting for the page load",
                    self.service.label()
                );
                true
            }
            found => {
                self.finish(app, found, "navigation");
                false
            }
        }
    }

    /// Page-load hook: catches a redirect the navigation hook let through.
    fn on_page_load(self: &Arc<Self>, app: &AppHandle, url: &Url, event: PageLoadEvent) {
        match oauth::parse_redirect(self.service, url) {
            // Host and path only: the query of a sign-in page can carry the state or a code.
            Redirect::Elsewhere if matches!(event, PageLoadEvent::Finished) => tracing::info!(
                "{} sign-in: page loaded: {}{}",
                self.service.label(),
                url.host_str().unwrap_or("?"),
                url.path()
            ),
            Redirect::Elsewhere => {}
            Redirect::Empty => {
                if matches!(event, PageLoadEvent::Finished) {
                    self.finish(app, Redirect::Empty, "page load");
                }
            }
            found => self.finish(app, found, "page load"),
        }
    }

    /// `seen_by` names the hook that caught the redirect, for the log. The address itself is
    /// never logged: it carries the token or code.
    fn finish(self: &Arc<Self>, app: &AppHandle, redirect: Redirect, seen_by: &str) {
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        let kind = match &redirect {
            Redirect::Token { .. } => "token",
            Redirect::Code { .. } => "code",
            Redirect::Denied { .. } => "error",
            Redirect::Elsewhere | Redirect::Empty => "nothing",
        };
        tracing::info!(
            "{} sign-in: redirect with {} caught at {}",
            self.service.label(),
            kind,
            seen_by
        );
        let attempt = Arc::clone(self);
        let app = app.clone();
        // Not on the webview's own thread: this runs inside one of its callbacks.
        tauri::async_runtime::spawn(async move {
            if let Some(window) = app.get_webview_window(window_label(attempt.service)) {
                let _ = window.destroy();
            }
            let result = super::finish_login(
                &app,
                attempt.service,
                redirect,
                &attempt.state,
                attempt.code_verifier.as_deref(),
            )
            .await;
            report(&app, attempt.service, result);
        });
    }

    fn window_closed(&self, app: &AppHandle) {
        if !self.finished.swap(true, Ordering::SeqCst) {
            tracing::info!("{} sign-in: window closed before a result came back", self.service.label());
            super::emit(
                app,
                LOGIN_EVENT,
                LoginResult {
                    service: self.service,
                    outcome: "cancelled",
                    name: None,
                    message: None,
                },
            );
        }
    }
}

pub(crate) fn report(app: &AppHandle, service: Service, result: Result<Profile, String>) {
    let payload = match result {
        Ok(profile) => {
            tracing::info!("{} connected", service.label());
            LoginResult {
                service,
                outcome: "connected",
                name: Some(profile.name).filter(|name| !name.is_empty()),
                message: None,
            }
        }
        Err(message) => {
            tracing::warn!("{} sign-in failed: {}", service.label(), message);
            LoginResult {
                service,
                outcome: "failed",
                name: None,
                message: Some(message),
            }
        }
    };
    super::emit(app, LOGIN_EVENT, payload);
}

fn focus_existing(app: &AppHandle, service: Service) -> bool {
    match app.get_webview_window(window_label(service)) {
        Some(window) => {
            let _ = window.unminimize();
            let _ = window.set_focus();
            true
        }
        None => false,
    }
}

/// Opens the sign-in window of `service`, or brings it to the front when it is already open.
/// The result of the sign-in arrives as an `account-login` event.
pub fn open(app: &AppHandle, service: Service) -> Result<(), String> {
    if service == Service::Comick {
        return super::loopback::open(app);
    }

    if focus_existing(app, service) {
        return Ok(());
    }

    let state = oauth::new_state()?;
    let (authorize_url, code_verifier) = match service {
        Service::AniList => (oauth::anilist_authorize_url(&state), None),
        Service::Comick => return Err("Comick signs in through the browser.".to_string()),
        Service::Mal => {
            let verifier = oauth::new_pkce_verifier()?;
            (oauth::mal_authorize_url(&state, &verifier), Some(verifier))
        }
    };
    let authorize_url: Url = authorize_url
        .parse()
        .map_err(|e| format!("Invalid {} sign-in address: {}", service.label(), e))?;

    let attempt = Arc::new(Attempt {
        service,
        state,
        code_verifier,
        finished: AtomicBool::new(false),
    });

    let navigation = (Arc::clone(&attempt), app.clone());
    let page_load = (Arc::clone(&attempt), app.clone());
    let built = WebviewWindowBuilder::new(app, window_label(service), WebviewUrl::External(authorize_url))
        .title(format!("Sign in to {}", service.label()))
        .inner_size(520.0, 760.0)
        .min_inner_size(400.0, 520.0)
        .center()
        .focused(true)
        // A session of its own that is gone when the window closes: nothing of the account
        // stays in the app's web data, and connecting again can use another account.
        .incognito(true)
        .devtools(false)
        .on_navigation(move |url| navigation.0.on_navigation(&navigation.1, url))
        .on_page_load(move |_window, payload| {
            page_load.0.on_page_load(&page_load.1, payload.url(), payload.event())
        })
        .build();

    let window = match built {
        Ok(window) => window,
        // A second click that raced the first one: the window exists by now.
        Err(_) if focus_existing(app, service) => return Ok(()),
        Err(e) => {
            return Err(format!(
                "Could not open the {} sign-in window: {}",
                service.label(),
                e
            ))
        }
    };

    tracing::info!("{} sign-in: window opened", service.label());
    let closed = (attempt, app.clone());
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            closed.0.window_closed(&closed.1);
        }
    });
    Ok(())
}

/// Closes the sign-in window of `service` if one is open.
pub fn close(app: &AppHandle, service: Service) {
    if service == Service::Comick {
        super::loopback::cancel(app);
        return;
    }
    if let Some(window) = app.get_webview_window(window_label(service)) {
        let _ = window.destroy();
    }
}
