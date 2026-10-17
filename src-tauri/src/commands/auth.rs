use crate::accounts;
use crate::state::AppState;
use accounts::comick::ComickEntry;
use accounts::oauth::Service;
use accounts::session;
use accounts::store::{AccountStore, AccountsStatus};
use tauri::{AppHandle, State};

pub const ANILIST_CLIENT_ID: &str = "45552";

/// Opens the sign-in window of `service` (`anilist` or `mal`), or focuses it when it is
/// already open; for `comick`, opens Comick's sign-in page in the system browser. How the
/// sign-in ends is reported by the `account-login` event.
#[tauri::command]
pub async fn start_account_login(app: AppHandle, service: Service) -> Result<(), String> {
    accounts::login::open(&app, service)
}

/// Which accounts are connected, with the name and avatar to show. Never contains a token.
#[tauri::command]
pub async fn get_account_status(app: AppHandle) -> Result<AccountsStatus, String> {
    let status = accounts::status(&AccountStore::open_default()?);
    // A login saved by an older version has no name yet; `account-changed` follows once it has.
    if status.anilist.connected && status.anilist.name.is_none() {
        accounts::spawn_profile_refresh(app);
    }
    Ok(status)
}

/// Stops a sign-in that is still waiting (closes its window, or stops waiting for the browser).
#[tauri::command]
pub async fn cancel_account_login(app: AppHandle, service: Service) -> Result<(), String> {
    accounts::login::close(&app, service);
    Ok(())
}

/// The connected user's Comick anime library.
#[tauri::command]
pub async fn get_comick_library(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ComickEntry>, String> {
    let (store, notifier) = accounts::parts(&app)?;
    let ctx = accounts::context(&state, &store, &notifier);
    let connected = store.is_connected(Service::Comick);
    accounts::sync::comick_library(&ctx)
        .await
        .map_err(|e| session::error_message(Service::Comick, connected, &e))
}

/// Forgets the stored login of `service`; a Comick login is also revoked. The remote lists
/// are not touched.
#[tauri::command]
pub async fn disconnect_account(app: AppHandle, service: Service) -> Result<AccountsStatus, String> {
    accounts::disconnect(&app, service)
}
