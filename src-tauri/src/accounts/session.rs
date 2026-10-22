//! Signing in, keeping tokens usable and signing out.

use super::comick::{ComickClient, RefreshError};
use super::oauth::{self, Redirect, Service};
use super::store::{AccountStore, ComickTokenSet, Profile};
use crate::clients::anilist::AniListClient;
use crate::clients::mal::{MalClient, MalTokenResponse};
use crate::commands::mal::MAL_CLIENT_ID;
use crate::db::current_unix_timestamp;
use crate::error::AppError;
use sqlx::SqlitePool;
use std::future::Future;

/// MAL tokens are refreshed this long before they expire, so a request never races the expiry.
const MAL_EXPIRY_MARGIN_S: i64 = 5 * 60;
const ANILIST_EXPIRY_MARGIN_S: i64 = 60;
/// Comick access tokens last 15 minutes.
const COMICK_EXPIRY_MARGIN_S: i64 = 60;

/// Something the frontend should hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The stored session of `service` was dropped because the service no longer accepts it.
    SignedOut { service: Service, message: String },
    /// The remote list entry of `anilist_id` was changed by a sync.
    Synced { service: Service, anilist_id: i32 },
}

pub trait Notifier: Send + Sync {
    fn notify(&self, notice: Notice);
}

/// Everything account code works with. Built per operation from the app state.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub store: &'a AccountStore,
    pub anilist: &'a AniListClient,
    pub mal: &'a MalClient,
    pub comick: &'a ComickClient,
    pub pool: &'a SqlitePool,
    pub notifier: &'a dyn Notifier,
}

pub fn signed_out_message(service: Service) -> String {
    format!(
        "{} no longer accepts the saved login. Connect your account again in Settings.",
        service.label()
    )
}

pub fn not_connected_message(service: Service) -> String {
    format!("{} is not connected.", service.label())
}

/// The text shown for a failed account request.
pub fn error_message(service: Service, connected_before: bool, error: &AppError) -> String {
    match error {
        AppError::AuthRequired if connected_before => signed_out_message(service),
        AppError::AuthRequired => not_connected_message(service),
        // The account features have no fallback catalogue; say so instead of a raw HTTP error.
        other if service == Service::AniList && crate::offline::is_offline_error(other) => format!(
            "AniList is unavailable right now ({}). Your AniList list, sync and statistics will work again once it is back; browsing uses other catalogues meanwhile.",
            other.reason()
        ),
        other => other.to_string(),
    }
}

fn sign_out(ctx: &Context<'_>, service: Service, token: &str) {
    match ctx.store.clear_if_token(service, token) {
        Ok(true) => {
            tracing::warn!("{} rejected the saved login; signed out", service.label());
            ctx.notifier.notify(Notice::SignedOut {
                service,
                message: signed_out_message(service),
            });
        }
        Ok(false) => {}
        Err(e) => tracing::error!("could not remove the {} login: {}", service.label(), e),
    }
}

/// The stored AniList token, or `None` when not connected. A token past its expiry signs the
/// account out (AniList tokens last a year and cannot be refreshed).
pub fn anilist_token(ctx: &Context<'_>) -> Option<String> {
    let session = ctx.store.anilist()?;
    let expired = session
        .expires_at
        .is_some_and(|at| at <= current_unix_timestamp() + ANILIST_EXPIRY_MARGIN_S);
    if expired {
        sign_out(ctx, Service::AniList, &session.token);
        return None;
    }
    Some(session.token)
}

/// Runs `call` with the AniList token. `AuthRequired` means not connected, or that AniList
/// rejected the token, in which case the account has been signed out.
pub async fn with_anilist<T, F, Fut>(ctx: &Context<'_>, call: F) -> Result<T, AppError>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let token = anilist_token(ctx).ok_or(AppError::AuthRequired)?;
    let result = call(token.clone()).await;
    if matches!(result, Err(AppError::AuthRequired)) {
        sign_out(ctx, Service::AniList, &token);
    }
    result
}

static MAL_REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn expires_at(tokens: &MalTokenResponse) -> i64 {
    current_unix_timestamp() + tokens.expires_in.max(0)
}

/// A usable MAL access token, or `None` when not connected. The token is refreshed when it is
/// about to expire, or when `rejected` names the stored token that MAL just answered 401 to.
/// A refresh that MAL refuses signs the account out; one that could not reach MAL keeps the
/// session for a later attempt.
pub async fn mal_token(ctx: &Context<'_>, rejected: Option<&str>) -> Result<Option<String>, AppError> {
    let needs_refresh = |access_token: &str, expires_at: i64| match rejected {
        Some(rejected) => access_token == rejected,
        None => expires_at - MAL_EXPIRY_MARGIN_S <= current_unix_timestamp(),
    };

    let Some(session) = ctx.store.mal() else {
        return Ok(None);
    };
    if !needs_refresh(&session.access_token, session.expires_at) {
        return Ok(Some(session.access_token));
    }

    let _guard = MAL_REFRESH.lock().await;
    // Another task may have refreshed (or signed out) while this one waited.
    let Some(session) = ctx.store.mal() else {
        return Ok(None);
    };
    if !needs_refresh(&session.access_token, session.expires_at) {
        return Ok(Some(session.access_token));
    }

    match ctx.mal.refresh_token(MAL_CLIENT_ID, &session.refresh_token).await {
        Ok(tokens) => {
            ctx.store
                .save_mal_tokens(&tokens.access_token, &tokens.refresh_token, expires_at(&tokens))
                .map_err(AppError::Other)?;
            Ok(Some(tokens.access_token))
        }
        Err(AppError::AuthRequired) => {
            sign_out(ctx, Service::Mal, &session.access_token);
            Err(AppError::AuthRequired)
        }
        Err(e) => {
            tracing::warn!("MyAnimeList token refresh failed: {}", e);
            let still_valid = rejected.is_none() && session.expires_at > current_unix_timestamp();
            if still_valid {
                Ok(Some(session.access_token))
            } else {
                Err(e)
            }
        }
    }
}

/// Runs `call` with a MAL access token, refreshing it first when it has expired and once more
/// when MAL answers 401. `AuthRequired` means not connected or signed out.
pub async fn with_mal<T, F, Fut>(ctx: &Context<'_>, call: F) -> Result<T, AppError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let token = mal_token(ctx, None).await?.ok_or(AppError::AuthRequired)?;
    match call(token.clone()).await {
        Err(AppError::AuthRequired) => {}
        other => return other,
    }

    let fresh = mal_token(ctx, Some(&token)).await?.ok_or(AppError::AuthRequired)?;
    let result = call(fresh.clone()).await;
    if matches!(result, Err(AppError::AuthRequired)) {
        sign_out(ctx, Service::Mal, &fresh);
    }
    result
}

static COMICK_REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A usable Comick access token, or `None` when not connected. Works like [`mal_token`], with
/// Comick's rules: one refresh at a time, both rotated tokens stored in one write, and no
/// second use of a refresh token whose refresh may have gone through, since Comick can revoke
/// the whole grant for that; such a session is signed out instead.
pub async fn comick_token(ctx: &Context<'_>, rejected: Option<&str>) -> Result<Option<String>, AppError> {
    let needs_refresh = |access_token: &str, expires_at: i64| match rejected {
        Some(rejected) => access_token == rejected,
        None => expires_at - COMICK_EXPIRY_MARGIN_S <= current_unix_timestamp(),
    };

    let Some(session) = ctx.store.comick() else {
        return Ok(None);
    };
    if !needs_refresh(&session.access_token, session.expires_at) {
        return Ok(Some(session.access_token));
    }

    let _guard = COMICK_REFRESH.lock().await;
    let Some(session) = ctx.store.comick() else {
        return Ok(None);
    };
    if !needs_refresh(&session.access_token, session.expires_at) {
        return Ok(Some(session.access_token));
    }

    let Some(refresh_token) = session.refresh_token.as_deref() else {
        // Connected without offline access: nothing to renew the token with.
        sign_out(ctx, Service::Comick, &session.access_token);
        return Err(AppError::AuthRequired);
    };

    match ctx.comick.refresh(refresh_token).await {
        Ok(tokens) => {
            let saved = ctx
                .store
                .save_comick_tokens(
                    &session.access_token,
                    &ComickTokenSet {
                        access_token: &tokens.access_token,
                        refresh_token: tokens.refresh_token.as_deref(),
                        expires_at: current_unix_timestamp() + tokens.expires_in.max(0),
                        scope: tokens.scope.as_deref(),
                    },
                )
                .map_err(AppError::Other)?;
            if !saved {
                // A new sign-in replaced the session while this refresh ran.
                return Ok(ctx.store.comick().map(|session| session.access_token));
            }
            Ok(Some(tokens.access_token))
        }
        Err(RefreshError::Refused) => {
            sign_out(ctx, Service::Comick, &session.access_token);
            Err(AppError::AuthRequired)
        }
        Err(RefreshError::Lost(e)) => {
            tracing::warn!("Comick token refresh got no usable answer: {}", e);
            sign_out(ctx, Service::Comick, &session.access_token);
            Err(AppError::AuthRequired)
        }
        Err(RefreshError::NotProcessed(e)) => {
            tracing::warn!("Comick token refresh failed: {}", e);
            let still_valid = rejected.is_none() && session.expires_at > current_unix_timestamp();
            if still_valid {
                Ok(Some(session.access_token))
            } else {
                Err(e)
            }
        }
    }
}

/// Runs `call` with a Comick access token, refreshing it first when it has expired and once
/// more when Comick answers 401. `AuthRequired` means not connected or signed out.
pub async fn with_comick<T, F, Fut>(ctx: &Context<'_>, call: F) -> Result<T, AppError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let token = comick_token(ctx, None).await?.ok_or(AppError::AuthRequired)?;
    match call(token.clone()).await {
        Err(AppError::AuthRequired) => {}
        other => return other,
    }

    let fresh = comick_token(ctx, Some(&token)).await?.ok_or(AppError::AuthRequired)?;
    let result = call(fresh.clone()).await;
    if matches!(result, Err(AppError::AuthRequired)) {
        sign_out(ctx, Service::Comick, &fresh);
    }
    result
}

async fn anilist_profile(ctx: &Context<'_>, token: &str) -> Result<Profile, AppError> {
    let viewer = ctx.anilist.get_viewer(token).await?;
    Ok(Profile {
        id: viewer.id as i64,
        name: viewer.name,
        avatar_url: viewer.avatar_url,
    })
}

async fn mal_profile(ctx: &Context<'_>, token: &str) -> Result<Profile, AppError> {
    let user = ctx.mal.get_user(token).await?;
    Ok(Profile {
        id: user.id,
        name: user.name,
        avatar_url: user.picture,
    })
}

/// Finishes a login from the redirect the service ended on: checks that it answers this
/// attempt (`expected_state`), obtains the tokens and the profile, and stores them.
pub async fn complete_login(
    ctx: &Context<'_>,
    service: Service,
    redirect: Redirect,
    expected_state: &str,
    code_verifier: Option<&str>,
) -> Result<Profile, String> {
    let label = service.label();
    let wrong_attempt = || format!("The {} response did not belong to this sign-in. Please try again.", label);

    match (service, redirect) {
        (Service::AniList, Redirect::Token { access_token, expires_in, state }) => {
            if state.as_deref() != Some(expected_state) {
                return Err(wrong_attempt());
            }
            let profile = anilist_profile(ctx, &access_token).await.map_err(|e| match e {
                AppError::AuthRequired => "AniList did not accept the token it issued.".to_string(),
                other => format!("Could not load your AniList profile: {}", other),
            })?;
            let expires_at = expires_in
                .filter(|seconds| *seconds > 0)
                .map(|seconds| current_unix_timestamp() + seconds)
                .or_else(|| super::store::jwt_expiry(&access_token));
            ctx.store.save_anilist(&access_token, expires_at, &profile)?;
            Ok(profile)
        }
        (Service::Mal, Redirect::Code { code, state }) => {
            if state.as_deref() != Some(expected_state) {
                return Err(wrong_attempt());
            }
            let verifier = code_verifier.ok_or("No MyAnimeList sign-in is in progress.")?;
            let tokens = ctx
                .mal
                .exchange_code(MAL_CLIENT_ID, &code, verifier)
                .await
                .map_err(|e| match e {
                    AppError::AuthRequired => "MyAnimeList did not accept the sign-in code.".to_string(),
                    other => format!("Could not finish the MyAnimeList sign-in: {}", other),
                })?;
            let profile = mal_profile(ctx, &tokens.access_token)
                .await
                .map_err(|e| format!("Could not load your MyAnimeList profile: {}", e))?;
            ctx.store.save_mal(
                &tokens.access_token,
                &tokens.refresh_token,
                expires_at(&tokens),
                &profile,
            )?;
            Ok(profile)
        }
        (Service::Comick, Redirect::Code { code, state }) => {
            if state.as_deref() != Some(expected_state) {
                return Err(wrong_attempt());
            }
            let verifier = code_verifier.ok_or("No Comick sign-in is in progress.")?;
            let tokens = ctx.comick.exchange_code(&code, verifier).await.map_err(|e| match e {
                AppError::AuthRequired => "Comick did not accept the sign-in code.".to_string(),
                other => format!("Could not finish the Comick sign-in: {}", other),
            })?;
            if !tokens.can_write() {
                tracing::warn!("Comick granted read access only; list updates will be skipped");
            }
            let user = ctx
                .comick
                .me(&tokens.access_token)
                .await
                .map_err(|e| format!("Could not load your Comick profile: {}", e))?;
            let name = user.username.clone().unwrap_or_default();
            ctx.store.save_comick(
                &ComickTokenSet {
                    access_token: &tokens.access_token,
                    refresh_token: tokens.refresh_token.as_deref(),
                    expires_at: current_unix_timestamp() + tokens.expires_in.max(0),
                    scope: tokens.scope.as_deref(),
                },
                &user.id,
                &name,
                user.avatar_url.as_deref(),
            )?;
            Ok(Profile {
                id: 0,
                name,
                avatar_url: user.avatar_url,
            })
        }
        (_, Redirect::Denied { error, description }) => {
            Err(oauth::denial_message(service, &error, description.as_deref()))
        }
        _ => Err(format!("{} did not return a sign-in result.", label)),
    }
}

/// Fetches the AniList profile again and stores a changed name or avatar.
pub async fn refresh_anilist_profile(ctx: &Context<'_>) -> Result<crate::models::Viewer, AppError> {
    with_anilist(ctx, |token| async move {
        let viewer = ctx.anilist.get_viewer(&token).await?;
        let profile = Profile {
            id: viewer.id as i64,
            name: viewer.name.clone(),
            avatar_url: viewer.avatar_url.clone(),
        };
        let stored = ctx.store.anilist().and_then(|session| session.profile);
        if stored.as_ref() != Some(&profile) {
            if let Err(e) = ctx.store.save_anilist_profile(&token, &profile) {
                tracing::warn!("could not store the AniList profile: {}", e);
            }
        }
        Ok(viewer)
    })
    .await
}

/// The AniList user id of the connected account, asking AniList when it is not stored yet.
pub async fn anilist_viewer_id(ctx: &Context<'_>) -> Result<i32, AppError> {
    let stored = ctx
        .store
        .anilist()
        .and_then(|session| session.profile)
        .map(|profile| profile.id as i32)
        .filter(|id| *id > 0);
    match stored {
        Some(id) => Ok(id),
        None => refresh_anilist_profile(ctx).await.map(|viewer| viewer.id),
    }
}
