use crate::accounts;
use accounts::oauth::Service;
use accounts::store::{AccountStore, AccountsStatus};
use tauri::AppHandle;

pub const ANILIST_CLIENT_ID: &str = "45552";

/// Opens the sign-in window of `service` (`anilist` or `mal`), or focuses it when it is
/// already open. How the sign-in ends is reported by the `account-login` event.
#[tauri::command]
pub async fn start_account_login(app: AppHandle, service: Service) -> Result<(), String> {
    accounts::login::open(&app, service)
}

/// Which accounts are connected, with the name and avatar to show. Never contains a token.
#[tauri::command]
pub async fn get_account_status(app: AppHandle) -> Result<AccountsStatus, String> {
    let status = AccountStore::open_default()?.status();
    // A login saved by an older version has no name yet; `account-changed` follows once it has.
    if status.anilist.connected && status.anilist.name.is_none() {
        accounts::spawn_profile_refresh(app);
    }
    Ok(status)
}

/// Forgets the stored login of `service`. The remote lists are not touched.
#[tauri::command]
pub async fn disconnect_account(app: AppHandle, service: Service) -> Result<AccountsStatus, String> {
    accounts::disconnect(&app, service)
}
