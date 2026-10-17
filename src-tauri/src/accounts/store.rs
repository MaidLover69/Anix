//! Account tokens and profiles, kept in `settings.json` (owner-only, written atomically).
//!
//! Only the backend reads and writes these keys. `redact_secrets` and
//! `keep_stored_account_keys` are for the settings commands, so that the tokens never reach
//! the frontend and a settings save from the frontend cannot change or drop them.

use super::oauth::Service;
use crate::commands::settings::{read_settings_file, write_settings_file};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub const ANILIST_TOKEN: &str = "anilist_token";
/// Unix seconds.
pub const ANILIST_TOKEN_EXPIRES_AT: &str = "anilist_token_expires_at";
pub const ANILIST_VIEWER_ID: &str = "anilist_viewer_id";
pub const ANILIST_VIEWER_NAME: &str = "anilist_viewer_name";
pub const ANILIST_VIEWER_AVATAR: &str = "anilist_viewer_avatar";

pub const MAL_ACCESS_TOKEN: &str = "mal_access_token";
pub const MAL_REFRESH_TOKEN: &str = "mal_refresh_token";
/// Unix seconds.
pub const MAL_TOKEN_EXPIRES_AT: &str = "mal_token_expires_at";
pub const MAL_USER_ID: &str = "mal_user_id";
pub const MAL_USER_NAME: &str = "mal_user_name";
pub const MAL_USER_PICTURE: &str = "mal_user_picture";

pub const COMICK_ACCESS_TOKEN: &str = "comick_access_token";
pub const COMICK_REFRESH_TOKEN: &str = "comick_refresh_token";
/// Unix seconds.
pub const COMICK_TOKEN_EXPIRES_AT: &str = "comick_token_expires_at";
/// The granted scopes, space-separated.
pub const COMICK_SCOPE: &str = "comick_scope";
/// The account's public UUID.
pub const COMICK_USER_ID: &str = "comick_user_id";
pub const COMICK_USER_NAME: &str = "comick_user_name";
pub const COMICK_USER_AVATAR: &str = "comick_user_avatar";

/// The keys that must never be handed to the frontend.
pub const SECRET_KEYS: [&str; 5] = [
    ANILIST_TOKEN,
    MAL_ACCESS_TOKEN,
    MAL_REFRESH_TOKEN,
    COMICK_ACCESS_TOKEN,
    COMICK_REFRESH_TOKEN,
];

const ANILIST_KEYS: [&str; 5] = [
    ANILIST_TOKEN,
    ANILIST_TOKEN_EXPIRES_AT,
    ANILIST_VIEWER_ID,
    ANILIST_VIEWER_NAME,
    ANILIST_VIEWER_AVATAR,
];

const MAL_KEYS: [&str; 6] = [
    MAL_ACCESS_TOKEN,
    MAL_REFRESH_TOKEN,
    MAL_TOKEN_EXPIRES_AT,
    MAL_USER_ID,
    MAL_USER_NAME,
    MAL_USER_PICTURE,
];

const COMICK_KEYS: [&str; 7] = [
    COMICK_ACCESS_TOKEN,
    COMICK_REFRESH_TOKEN,
    COMICK_TOKEN_EXPIRES_AT,
    COMICK_SCOPE,
    COMICK_USER_ID,
    COMICK_USER_NAME,
    COMICK_USER_AVATAR,
];

/// Every key owned by this module.
pub fn account_keys() -> impl Iterator<Item = &'static str> {
    ANILIST_KEYS.into_iter().chain(MAL_KEYS).chain(COMICK_KEYS)
}

static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

/// Held around every read-modify-write of `settings.json`, so a token written by a login or a
/// refresh and a settings save from the frontend cannot overwrite each other.
pub fn settings_lock() -> MutexGuard<'static, ()> {
    SETTINGS_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Removes the account tokens from a settings object on its way to the frontend.
pub fn redact_secrets(settings: &mut Value) {
    if let Some(object) = settings.as_object_mut() {
        for key in SECRET_KEYS {
            object.remove(key);
        }
    }
}

/// Makes `incoming`, a settings object sent by the frontend, carry the account keys exactly
/// as they are in `stored`: whatever the frontend sent for them is ignored.
pub fn keep_stored_account_keys(stored: &Value, incoming: &mut Value) {
    let Some(incoming) = incoming.as_object_mut() else {
        return;
    };
    for key in account_keys() {
        match stored.get(key) {
            Some(value) => incoming.insert(key.to_string(), value.clone()),
            None => incoming.remove(key),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AniListSession {
    pub token: String,
    /// Unix seconds; `None` when neither the login nor the token itself says.
    pub expires_at: Option<i64>,
    pub profile: Option<Profile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalSession {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub profile: Option<Profile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComickSession {
    pub access_token: String,
    /// `None` when the user did not allow offline access: the session ends with the token.
    pub refresh_token: Option<String>,
    /// Unix seconds.
    pub expires_at: i64,
    pub scope: Option<String>,
    /// `id` is 0: Comick identifies accounts by UUID, stored as `user_id`.
    pub profile: Option<Profile>,
    pub user_id: Option<String>,
}

/// One successful Comick token response, stored in a single write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComickTokenSet<'a> {
    pub access_token: &'a str,
    pub refresh_token: Option<&'a str>,
    pub expires_at: i64,
    pub scope: Option<&'a str>,
}

/// What the frontend is told about one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AccountStatus {
    pub connected: bool,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AccountsStatus {
    pub anilist: AccountStatus,
    pub mal: AccountStatus,
    pub comick: AccountStatus,
    /// Whether this build can sign in to Comick (it was built with a Comick client id).
    pub comick_available: bool,
}

fn status_of(profile: Option<Profile>) -> AccountStatus {
    AccountStatus {
        connected: true,
        name: profile.as_ref().map(|p| p.name.clone()).filter(|name| !name.is_empty()),
        avatar_url: profile.and_then(|p| p.avatar_url),
    }
}

/// The `exp` claim (Unix seconds) of a JWT such as AniList's access token.
pub fn jwt_expiry(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    claims.get("exp")?.as_i64()
}

fn text(settings: &Value, key: &str) -> Option<String> {
    settings
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(String::from)
}

fn profile(settings: &Value, id_key: &str, name_key: &str, avatar_key: &str) -> Option<Profile> {
    Some(Profile {
        id: settings.get(id_key)?.as_i64()?,
        name: text(settings, name_key).unwrap_or_default(),
        avatar_url: text(settings, avatar_key),
    })
}

fn put_profile(settings: &mut Map<String, Value>, keys: [&str; 3], profile: &Profile) {
    settings.insert(keys[0].to_string(), json!(profile.id));
    settings.insert(keys[1].to_string(), json!(profile.name));
    settings.insert(keys[2].to_string(), json!(profile.avatar_url));
}

/// The account part of one settings file.
#[derive(Debug, Clone)]
pub struct AccountStore {
    path: PathBuf,
}

impl AccountStore {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The app's own `settings.json`.
    pub fn open_default() -> Result<Self, String> {
        let dir = crate::db::get_config_dir().map_err(|e| e.to_string())?;
        Ok(Self::at(dir.join("settings.json")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Value {
        let _guard = settings_lock();
        read_settings_file(&self.path).unwrap_or_else(|e| {
            tracing::warn!("could not read {}: {}", self.path.display(), e);
            json!({})
        })
    }

    fn update(&self, change: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
        let _guard = settings_lock();
        let mut settings = read_settings_file(&self.path)?;
        let before = settings.clone();
        if let Some(object) = settings.as_object_mut() {
            change(object);
        }
        if settings == before {
            return Ok(());
        }
        write_settings_file(&self.path, &settings)
    }

    pub fn anilist(&self) -> Option<AniListSession> {
        let settings = self.read();
        let token = text(&settings, ANILIST_TOKEN)?;
        Some(AniListSession {
            expires_at: settings
                .get(ANILIST_TOKEN_EXPIRES_AT)
                .and_then(Value::as_i64)
                .or_else(|| jwt_expiry(&token)),
            profile: profile(
                &settings,
                ANILIST_VIEWER_ID,
                ANILIST_VIEWER_NAME,
                ANILIST_VIEWER_AVATAR,
            ),
            token,
        })
    }

    pub fn mal(&self) -> Option<MalSession> {
        let settings = self.read();
        Some(MalSession {
            access_token: text(&settings, MAL_ACCESS_TOKEN)?,
            refresh_token: text(&settings, MAL_REFRESH_TOKEN)?,
            expires_at: settings
                .get(MAL_TOKEN_EXPIRES_AT)
                .and_then(Value::as_i64)
                .unwrap_or(0),
            profile: profile(&settings, MAL_USER_ID, MAL_USER_NAME, MAL_USER_PICTURE),
        })
    }

    pub fn comick(&self) -> Option<ComickSession> {
        let settings = self.read();
        let name = text(&settings, COMICK_USER_NAME);
        let avatar_url = text(&settings, COMICK_USER_AVATAR);
        let user_id = text(&settings, COMICK_USER_ID);
        Some(ComickSession {
            access_token: text(&settings, COMICK_ACCESS_TOKEN)?,
            refresh_token: text(&settings, COMICK_REFRESH_TOKEN),
            expires_at: settings
                .get(COMICK_TOKEN_EXPIRES_AT)
                .and_then(Value::as_i64)
                .unwrap_or(0),
            scope: text(&settings, COMICK_SCOPE),
            profile: user_id.as_ref().map(|_| Profile {
                id: 0,
                name: name.unwrap_or_default(),
                avatar_url,
            }),
            user_id,
        })
    }

    pub fn is_connected(&self, service: Service) -> bool {
        match service {
            Service::AniList => self.anilist().is_some(),
            Service::Mal => self.mal().is_some(),
            Service::Comick => self.comick().is_some(),
        }
    }

    pub fn status(&self) -> AccountsStatus {
        AccountsStatus {
            anilist: self
                .anilist()
                .map(|session| status_of(session.profile))
                .unwrap_or_default(),
            mal: self
                .mal()
                .map(|session| status_of(session.profile))
                .unwrap_or_default(),
            comick: self
                .comick()
                .map(|session| status_of(session.profile))
                .unwrap_or_default(),
            comick_available: false,
        }
    }

    pub fn save_anilist(
        &self,
        token: &str,
        expires_at: Option<i64>,
        profile: &Profile,
    ) -> Result<(), String> {
        self.update(|settings| {
            settings.insert(ANILIST_TOKEN.to_string(), json!(token));
            match expires_at {
                Some(at) => settings.insert(ANILIST_TOKEN_EXPIRES_AT.to_string(), json!(at)),
                None => settings.remove(ANILIST_TOKEN_EXPIRES_AT),
            };
            put_profile(
                settings,
                [ANILIST_VIEWER_ID, ANILIST_VIEWER_NAME, ANILIST_VIEWER_AVATAR],
                profile,
            );
        })
    }

    /// Updates the saved name and avatar while `token` is still the stored one.
    pub fn save_anilist_profile(&self, token: &str, profile: &Profile) -> Result<(), String> {
        self.update(|settings| {
            if settings.get(ANILIST_TOKEN).and_then(Value::as_str) == Some(token) {
                put_profile(
                    settings,
                    [ANILIST_VIEWER_ID, ANILIST_VIEWER_NAME, ANILIST_VIEWER_AVATAR],
                    profile,
                );
            }
        })
    }

    pub fn save_mal_tokens(
        &self,
        access_token: &str,
        refresh_token: &str,
        expires_at: i64,
    ) -> Result<(), String> {
        self.update(|settings| {
            settings.insert(MAL_ACCESS_TOKEN.to_string(), json!(access_token));
            settings.insert(MAL_REFRESH_TOKEN.to_string(), json!(refresh_token));
            settings.insert(MAL_TOKEN_EXPIRES_AT.to_string(), json!(expires_at));
        })
    }

    pub fn save_mal(
        &self,
        access_token: &str,
        refresh_token: &str,
        expires_at: i64,
        profile: &Profile,
    ) -> Result<(), String> {
        self.update(|settings| {
            settings.insert(MAL_ACCESS_TOKEN.to_string(), json!(access_token));
            settings.insert(MAL_REFRESH_TOKEN.to_string(), json!(refresh_token));
            settings.insert(MAL_TOKEN_EXPIRES_AT.to_string(), json!(expires_at));
            put_profile(settings, [MAL_USER_ID, MAL_USER_NAME, MAL_USER_PICTURE], profile);
        })
    }

    fn put_comick_tokens(settings: &mut Map<String, Value>, tokens: &ComickTokenSet<'_>) {
        settings.insert(COMICK_ACCESS_TOKEN.to_string(), json!(tokens.access_token));
        match tokens.refresh_token {
            Some(refresh) => settings.insert(COMICK_REFRESH_TOKEN.to_string(), json!(refresh)),
            None => settings.remove(COMICK_REFRESH_TOKEN),
        };
        settings.insert(COMICK_TOKEN_EXPIRES_AT.to_string(), json!(tokens.expires_at));
        if let Some(scope) = tokens.scope {
            settings.insert(COMICK_SCOPE.to_string(), json!(scope));
        }
    }

    /// Replaces the tokens after a refresh, all in one write, while `previous_access_token` is
    /// still the stored one. Returns false when the session changed meanwhile.
    pub fn save_comick_tokens(
        &self,
        previous_access_token: &str,
        tokens: &ComickTokenSet<'_>,
    ) -> Result<bool, String> {
        let mut saved = false;
        self.update(|settings| {
            if settings.get(COMICK_ACCESS_TOKEN).and_then(Value::as_str) == Some(previous_access_token) {
                Self::put_comick_tokens(settings, tokens);
                saved = true;
            }
        })?;
        Ok(saved)
    }

    pub fn save_comick(
        &self,
        tokens: &ComickTokenSet<'_>,
        user_id: &str,
        name: &str,
        avatar_url: Option<&str>,
    ) -> Result<(), String> {
        self.update(|settings| {
            remove_keys(settings, Service::Comick);
            Self::put_comick_tokens(settings, tokens);
            settings.insert(COMICK_USER_ID.to_string(), json!(user_id));
            settings.insert(COMICK_USER_NAME.to_string(), json!(name));
            settings.insert(COMICK_USER_AVATAR.to_string(), json!(avatar_url));
        })
    }

    /// Removes every stored key of one account.
    pub fn clear(&self, service: Service) -> Result<(), String> {
        self.update(|settings| remove_keys(settings, service))
    }

    /// Removes the account only while `token` is still its stored access token, so a session
    /// that was replaced by a new login in the meantime is left alone. Returns whether it did.
    pub fn clear_if_token(&self, service: Service, token: &str) -> Result<bool, String> {
        let token_key = match service {
            Service::AniList => ANILIST_TOKEN,
            Service::Mal => MAL_ACCESS_TOKEN,
            Service::Comick => COMICK_ACCESS_TOKEN,
        };
        let mut cleared = false;
        self.update(|settings| {
            if settings.get(token_key).and_then(Value::as_str) == Some(token) {
                remove_keys(settings, service);
                cleared = true;
            }
        })?;
        Ok(cleared)
    }
}

fn remove_keys(settings: &mut Map<String, Value>, service: Service) {
    let keys: &[&str] = match service {
        Service::AniList => &ANILIST_KEYS,
        Service::Mal => &MAL_KEYS,
        Service::Comick => &COMICK_KEYS,
    };
    for key in keys {
        settings.remove(*key);
    }
}
