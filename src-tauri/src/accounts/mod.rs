//! AniList, MyAnimeList and Comick accounts: sign-in, token storage and list sync.
//!
//! `oauth`, `comick`, `store`, `session` and `sync` do not depend on the running app and are
//! tested on their own; this file, `login` and `loopback` connect them to it.

pub mod comick;
pub mod login;
pub mod loopback;
pub mod oauth;
pub mod session;
pub mod store;
pub mod sync;

use crate::state::AppState;
use comick::ComickClient;
use once_cell::sync::Lazy;
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

static COMICK_CLIENT: Lazy<ComickClient> =
    Lazy::new(|| ComickClient::new(crate::clients::http::build_http_client()));

pub fn context<'a>(
    state: &'a AppState,
    store: &'a AccountStore,
    notifier: &'a AppNotifier,
) -> Context<'a> {
    Context {
        store,
        anilist: &state.anilist_client,
        mal: &state.mal_client,
        comick: &COMICK_CLIENT,
        pool: &state.pool,
        notifier,
    }
}

/// The account status for the frontend, with what this build offers.
pub fn status(store: &AccountStore) -> AccountsStatus {
    let mut status = store.status();
    status.comick_available = Service::Comick.available();
    status
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

/// Removes the stored session of `service`. AniList and MyAnimeList tokens are only
/// forgotten; Comick tokens are also revoked, in the background. No list is changed.
pub fn disconnect(app: &AppHandle, service: Service) -> Result<AccountsStatus, String> {
    login::close(app, service);
    let store = AccountStore::open_default()?;
    let comick_session = match service {
        Service::Comick => store.comick(),
        _ => None,
    };
    store.clear(service)?;
    emit_changed(app, service, false, None);
    if let Some(session) = comick_session {
        tauri::async_runtime::spawn(async move {
            revoke_comick(&COMICK_CLIENT, &session).await;
        });
    }
    Ok(status(&store))
}

/// Revokes both tokens of a Comick session that is being forgotten. Failures are logged only:
/// the local copy is already gone.
pub async fn revoke_comick(client: &ComickClient, session: &store::ComickSession) {
    let mut tokens = vec![(session.access_token.as_str(), "access_token")];
    if let Some(refresh) = session.refresh_token.as_deref() {
        tokens.insert(0, (refresh, "refresh_token"));
    }
    for (token, hint) in tokens {
        if let Err(e) = client.revoke(token, hint).await {
            tracing::warn!("could not revoke the Comick {}: {}", hint, e);
        }
    }
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
