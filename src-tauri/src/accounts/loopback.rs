//! The Comick sign-in: Comick's own page in the system browser, its redirect answered by a
//! listener on [`oauth::COMICK_REDIRECT_URI`] for a few minutes. Comick forbids embedded
//! webviews for its login page, and the app never sees the user's password or cookies.

use super::login::{self, LoginResult, LOGIN_EVENT};
use super::oauth::{self, ComickCallback, Service};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tiny_http::{Header, Response, Server};

/// How long the listener waits for the browser to come back.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

struct Pending {
    authorize_url: String,
    server: Arc<Server>,
    cancelled: Arc<AtomicBool>,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

fn pending() -> MutexGuard<'static, Option<Pending>> {
    PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn open_browser(url: &str) -> Result<(), String> {
    crate::commands::player::open_in_browser(url)
        .map_err(|e| format!("Could not open the browser for the Comick sign-in: {}", e))
}

/// Starts a Comick sign-in, or shows the page of the one already waiting again. The result
/// arrives as an `account-login` event.
pub fn open(app: &AppHandle) -> Result<(), String> {
    let client_id = oauth::comick_client_id().ok_or("Comick is not available in this build.")?;
    let mut slot = pending();
    if let Some(waiting) = slot.as_ref() {
        return open_browser(&waiting.authorize_url);
    }

    let state = oauth::new_state()?;
    let verifier = oauth::new_pkce_verifier()?;
    let authorize_url = oauth::comick_authorize_url(client_id, &state, &verifier);
    let server = Server::http((oauth::COMICK_CALLBACK_HOST, oauth::COMICK_CALLBACK_PORT))
        .map(Arc::new)
        .map_err(|e| {
            format!(
                "Could not wait for the Comick sign-in on port {}: {}. Another program may be using it.",
                oauth::COMICK_CALLBACK_PORT,
                e
            )
        })?;
    open_browser(&authorize_url)?;

    let cancelled = Arc::new(AtomicBool::new(false));
    *slot = Some(Pending {
        authorize_url,
        server: Arc::clone(&server),
        cancelled: Arc::clone(&cancelled),
    });
    drop(slot);

    let app = app.clone();
    std::thread::Builder::new()
        .name("comick-login".to_string())
        .spawn(move || wait(app, server, cancelled, state, verifier))
        .map_err(|e| format!("Could not wait for the Comick sign-in: {}", e))?;
    Ok(())
}

/// Stops a waiting Comick sign-in; it ends as `cancelled`.
pub fn cancel(_app: &AppHandle) {
    if let Some(waiting) = pending().take() {
        waiting.cancelled.store(true, Ordering::SeqCst);
        waiting.server.unblock();
    }
}

enum Ended {
    Finished(Result<super::store::Profile, String>),
    Cancelled,
    TimedOut,
}

fn wait(app: AppHandle, server: Arc<Server>, cancelled: Arc<AtomicBool>, state: String, verifier: String) {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let ended = loop {
        if cancelled.load(Ordering::SeqCst) {
            break Ended::Cancelled;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break Ended::TimedOut;
        }
        let request = match server.recv_timeout(left.min(Duration::from_secs(1))) {
            Ok(Some(request)) => request,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("Comick sign-in listener failed: {}", e);
                break Ended::Cancelled;
            }
        };
        match oauth::classify_comick_callback(request.url(), &state) {
            ComickCallback::NotCallback => {
                let _ = request.respond(Response::from_string("Not found").with_status_code(404));
            }
            ComickCallback::WrongState => {
                tracing::warn!("Comick sign-in: callback for another attempt ignored");
                let _ = request.respond(page(
                    400,
                    "This sign-in link is not the one Anix is waiting for",
                    "Start the sign-in again from Anix › Settings › Account.",
                ));
            }
            ComickCallback::Answer(redirect) => {
                tracing::info!("Comick sign-in: callback received");
                let result = tauri::async_runtime::block_on(super::finish_login(
                    &app,
                    Service::Comick,
                    redirect,
                    &state,
                    Some(&verifier),
                ));
                let response = match &result {
                    Ok(_) => page(200, "Connected to Comick", "You can close this tab and return to Anix."),
                    Err(message) => page(200, "Comick sign-in failed", message),
                };
                let _ = request.respond(response);
                break Ended::Finished(result);
            }
        }
    };

    {
        let mut slot = pending();
        if slot.as_ref().is_some_and(|waiting| Arc::ptr_eq(&waiting.server, &server)) {
            *slot = None;
        }
    }
    drop(server);

    match ended {
        Ended::Finished(result) => login::report(&app, Service::Comick, result),
        Ended::Cancelled => super::emit(
            &app,
            LOGIN_EVENT,
            LoginResult {
                service: Service::Comick,
                outcome: "cancelled",
                name: None,
                message: None,
            },
        ),
        Ended::TimedOut => login::report(
            &app,
            Service::Comick,
            Err("The Comick sign-in was not finished in time. Please try again.".to_string()),
        ),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The small page the browser shows after the redirect.
fn page(status: u16, title: &str, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head>\
         <body style=\"font-family:system-ui,sans-serif;background:#111;color:#eee;display:flex;\
         min-height:90vh;align-items:center;justify-content:center;text-align:center\">\
         <main><h1 style=\"font-size:1.4rem\">{title}</h1><p>{message}</p></main></body></html>",
        title = escape(title),
        message = escape(message),
    );
    Response::from_string(html)
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").expect("static header"))
        .with_header(Header::from_bytes("Cache-Control", "no-store").expect("static header"))
}
