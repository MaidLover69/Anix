//! AniList and MyAnimeList accounts: sign-in windows, token storage and list sync.
//!
//! `oauth`, `store`, `session` and `sync` do not depend on the running app and are tested on
//! their own; this file and `login` connect them to it.

pub mod login;
pub mod oauth;
pub mod session;
pub mod store;
pub mod sync;

use crate::state::AppState;
use oauth::{Redirect, Service};
use serde::Serialize;
use session::{Context, Notice, Notifier};
use store::{AccountStore, AccountsStatus, Profile};
use tauri::{AppHandle, Emitter, Manager};

/// The stored session of an account changed: connected, disconnected or signed out.
pub const CHANGED_EVENT: &str = "account-changed";
/// A sync changed a remote list entry.
pub const SYNCED_EVENT: &str = "account-synced";

const MAIN_WINDOW: &str = "main";

#[derive(Debug, Clone, Serialize)]
struct AccountChanged {
    service: Service,
    connected: bool,
    /// Set when the service ended the session rather than the user.
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AccountSynced {
    service: Service,
    anilist_id: i32,
}

/// Events go to the main window only; sign-in windows never receive them.
pub(crate) fn emit<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Err(e) = app.emit_to(MAIN_WINDOW, event, payload) {
        tracing::warn!("could not emit {}: {}", event, e);
    }
}

fn emit_changed(app: &AppHandle, service: Service, connected: bool, message: Option<String>) {
    emit(
        app,
        CHANGED_EVENT,
        AccountChanged {
            service,
            connected,
            message,
        },
    );
}

/// Turns account notices into frontend events.
pub struct AppNotifier(pub AppHandle);

impl Notifier for AppNotifier {
    fn notify(&self, notice: Notice) {
        match notice {
            Notice::SignedOut { service, message } => {
                emit_changed(&self.0, service, false, Some(message))
            }
            Notice::Synced { service, anilist_id } => {
                emit(&self.0, SYNCED_EVENT, AccountSynced { service, anilist_id })
            }
        }
    }
}

/// Builds the [`Context`] of one operation: `let (store, notifier) = parts(&app)?;` then
/// `context(&state, &store, &notifier)`.
pub fn parts(app: &AppHandle) -> Result<(AccountStore, AppNotifier), String> {
    Ok((AccountStore::open_default()?, AppNotifier(app.clone())))
}

pub fn context<'a>(
    state: &'a AppState,
    store: &'a AccountStore,
    notifier: &'a AppNotifier,
) -> Context<'a> {
    Context {
        store,
        anilist: &state.anilist_client,
        mal: &state.mal_client,
        pool: &state.pool,
        notifier,
    }
}

/// Completes a sign-in after its window reached the redirect, then pushes what was waiting
/// for this account.
pub(crate) async fn finish_login(
    app: &AppHandle,
    service: Service,
    redirect: Redirect,
    expected_state: &str,
    code_verifier: Option<&str>,
) -> Result<Profile, String> {
    let (store, notifier) = parts(app)?;
    let state = app
        .try_state::<AppState>()
        .ok_or("The app is still starting. Please try again.")?;
    let ctx = context(&state, &store, &notifier);
    let profile =
        session::complete_login(&ctx, service, redirect, expected_state, code_verifier).await?;
    emit_changed(app, service, true, None);
    spawn_sync(app.clone());
    Ok(profile)
}

/// Removes the stored session of `service`. Nothing is revoked or deleted remotely.
pub fn disconnect(app: &AppHandle, service: Service) -> Result<AccountsStatus, String> {
    login::close(app, service);
    let store = AccountStore::open_default()?;
    store.clear(service)?;
    emit_changed(app, service, false, None);
    Ok(store.status())
}

/// Sends everything queued for the connected services, in the background. Called after a
/// watched episode, a watchlist addition and a sign-in, and once at startup for what earlier
/// runs left in the queue.
pub fn spawn_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let (store, notifier) = match parts(&app) {
            Ok(parts) => parts,
            Err(e) => {
                tracing::warn!("list sync skipped: {}", e);
                return;
            }
        };
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let ctx = context(&state, &store, &notifier);
        sync::drain(&ctx).await;
    });
}

/// Loads the AniList name and avatar of a login that was stored without them.
pub fn spawn_profile_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Ok((store, notifier)) = parts(&app) else {
            return;
        };
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let ctx = context(&state, &store, &notifier);
        match session::refresh_anilist_profile(&ctx).await {
            Ok(_) => emit_changed(&app, Service::AniList, true, None),
            Err(e) => tracing::warn!("could not load the AniList profile: {}", e),
        }
    });
}

/// An episode has just crossed the watched threshold: queue it and push in the background.
pub async fn episode_watched(app: &AppHandle, state: &AppState, anilist_id: i32, episode_number: f64) {
    match sync::queue_watched_episode(&state.pool, anilist_id, episode_number).await {
        Ok(true) => spawn_sync(app.clone()),
        Ok(false) => {}
        Err(e) => tracing::warn!("could not queue list progress for anime {}: {}", anilist_id, e),
    }
}

/// An anime was added to the in-app watchlist: add it to the connected lists as planning.
pub async fn watchlist_added(app: &AppHandle, state: &AppState, anilist_id: i32) {
    let queued = async {
        let (store, notifier) = parts(app)?;
        let ctx = context(state, &store, &notifier);
        sync::queue_watchlist_addition(&ctx, anilist_id)
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    match queued {
        Ok(true) => spawn_sync(app.clone()),
        Ok(false) => {}
        Err(e) => tracing::warn!("could not queue the watchlist addition of anime {}: {}", anilist_id, e),
    }
}
