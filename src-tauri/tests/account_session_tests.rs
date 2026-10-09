//! Token storage, finishing a login, MAL token refresh and what a rejected token does.
//! AniList and MyAnimeList are a loopback server; nothing here uses the network.

mod account_support;

use anilili_lib::accounts;

use accounts::oauth::{Redirect, Service};
use accounts::session::{self, Notice};
use accounts::store::{self, AccountStore, Profile};
use account_support::{closed_address, far_future, FakeServer, World};
use anilili_lib::commands::settings::{read_settings_file, write_settings_file};
use anilili_lib::db::current_unix_timestamp;
use anilili_lib::error::AppError;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

fn profile(name: &str) -> Profile {
    Profile {
        id: 7,
        name: name.to_string(),
        avatar_url: Some("https://img.example/a.png".to_string()),
    }
}

const VIEWER: &str = r#"{"data":{"Viewer":{"id":42,"name":"mehul","avatar":{"large":"https://img.example/v.png"},"statistics":{"anime":{"count":3,"episodesWatched":30,"minutesWatched":720,"meanScore":80.5}}}}}"#;
const INVALID_TOKEN: &str = r#"{"errors":[{"message":"Invalid token","status":400}],"data":null}"#;

fn tokens(access: &str, refresh: &str) -> String {
    json!({ "token_type": "Bearer", "expires_in": 2_678_400, "access_token": access, "refresh_token": refresh })
        .to_string()
}

// ---- Storage ------------------------------------------------------------------------------

#[tokio::test]
async fn test_account_store_keeps_other_settings_and_is_private() {
    let world = World::new("http://127.0.0.1:9").await;
    write_settings_file(world.store.path(), &json!({ "autoplay": false, "download_dir": "/tmp/x" })).unwrap();

    assert_eq!(world.store.status(), store::AccountsStatus::default());
    world.store.save_anilist("ani-token", Some(far_future()), &profile("mehul")).unwrap();
    world.store.save_mal("mal-access", "mal-refresh", far_future(), &profile("mehul_mal")).unwrap();

    let settings = world.settings();
    assert_eq!(settings["autoplay"], json!(false));
    assert_eq!(settings["download_dir"], json!("/tmp/x"));
    assert_eq!(settings["anilist_token"], json!("ani-token"));
    assert_eq!(settings["mal_access_token"], json!("mal-access"));
    assert_eq!(settings["mal_refresh_token"], json!("mal-refresh"));
    let mode = std::fs::metadata(world.store.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);

    let status = world.store.status();
    assert!(status.anilist.connected && status.mal.connected);
    assert_eq!(status.anilist.name.as_deref(), Some("mehul"));
    assert_eq!(status.mal.name.as_deref(), Some("mehul_mal"));
    assert_eq!(status.mal.avatar_url.as_deref(), Some("https://img.example/a.png"));
    // What the frontend is told holds no token.
    let told = serde_json::to_string(&status).unwrap();
    assert!(!told.contains("token") && !told.contains("mal-access"), "{}", told);

    world.store.clear(Service::Mal).unwrap();
    let settings = world.settings();
    assert!(settings.get("mal_access_token").is_none());
    assert!(settings.get("mal_refresh_token").is_none());
    assert!(settings.get("mal_user_name").is_none());
    assert_eq!(settings["anilist_token"], json!("ani-token"));
    assert_eq!(settings["autoplay"], json!(false));
    assert!(!world.store.status().mal.connected);
}

#[tokio::test]
async fn test_account_store_clear_only_removes_the_token_it_was_given() {
    let world = World::new("http://127.0.0.1:9").await;
    world.store.save_anilist("new-token", None, &profile("mehul")).unwrap();
    assert!(!world.store.clear_if_token(Service::AniList, "old-token").unwrap());
    assert!(world.store.is_connected(Service::AniList));
    assert!(world.store.clear_if_token(Service::AniList, "new-token").unwrap());
    assert!(!world.store.is_connected(Service::AniList));
}

#[test]
fn test_account_settings_helpers_hide_and_preserve_account_keys() {
    let stored = json!({
        "autoplay": true,
        "anilist_token": "ani-token",
        "anilist_viewer_name": "mehul",
        "mal_access_token": "mal-access",
        "mal_refresh_token": "mal-refresh",
        "mal_token_expires_at": 99,
    });

    let mut for_frontend = stored.clone();
    store::redact_secrets(&mut for_frontend);
    assert_eq!(
        for_frontend,
        json!({ "autoplay": true, "anilist_viewer_name": "mehul", "mal_token_expires_at": 99 })
    );

    // The frontend saves what it was given plus its change, and tries to set account keys.
    let mut incoming = json!({
        "autoplay": false,
        "anilist_viewer_name": "someone else",
        "mal_access_token": "forged",
        "mal_user_name": "forged",
    });
    store::keep_stored_account_keys(&stored, &mut incoming);
    assert_eq!(
        incoming,
        json!({
            "autoplay": false,
            "anilist_token": "ani-token",
            "anilist_viewer_name": "mehul",
            "mal_access_token": "mal-access",
            "mal_refresh_token": "mal-refresh",
            "mal_token_expires_at": 99,
        })
    );

    // After a disconnect nothing the frontend still holds brings the account back.
    let mut stale = json!({ "autoplay": true, "anilist_token": "ani-token" });
    store::keep_stored_account_keys(&json!({ "autoplay": true }), &mut stale);
    assert_eq!(stale, json!({ "autoplay": true }));
}

#[test]
fn test_account_jwt_expiry() {
    // {"alg":"none"}.{"sub":"42","exp":1900000000}.sig
    let token = "eyJhbGciOiJub25lIn0.eyJzdWIiOiI0MiIsImV4cCI6MTkwMDAwMDAwMH0.sig";
    assert_eq!(store::jwt_expiry(token), Some(1_900_000_000));
    assert_eq!(store::jwt_expiry("not-a-jwt"), None);
    assert_eq!(store::jwt_expiry("a.!!!.c"), None);

    let dir = std::env::temp_dir().join(format!("anilili-account-jwt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    // A token saved by an older version: no expiry key next to it.
    write_settings_file(&path, &json!({ "anilist_token": token })).unwrap();
    let session = AccountStore::at(&path).anilist().expect("session");
    assert_eq!(session.expires_at, Some(1_900_000_000));
    assert_eq!(session.profile, None);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Finishing a login --------------------------------------------------------------------

#[tokio::test]
async fn test_account_mal_login_exchanges_the_code_and_stores_tokens() {
    let server = FakeServer::start(|request| {
        if request.url.starts_with("/token") {
            (200, tokens("access-1", "refresh-1"))
        } else if request.url.starts_with("/v2/users/@me") {
            (200, r#"{"id":99,"name":"mehul_mal","picture":"https://img.example/p.jpg"}"#.to_string())
        } else {
            (404, "{}".to_string())
        }
    });
    let world = World::new(&server.base).await;

    let redirect = Redirect::Code { code: "the-code".to_string(), state: Some("s2".to_string()) };
    let profile = session::complete_login(&world.ctx(), Service::Mal, redirect, "s2", Some("the-verifier"))
        .await
        .expect("login");
    assert_eq!(profile.name, "mehul_mal");

    let exchange = &server.requests_to("/token")[0];
    assert_eq!(exchange.method, "POST");
    assert_eq!(exchange.form("grant_type").as_deref(), Some("authorization_code"));
    assert_eq!(exchange.form("code").as_deref(), Some("the-code"));
    assert_eq!(exchange.form("code_verifier").as_deref(), Some("the-verifier"));
    assert_eq!(exchange.form("client_id").as_deref(), Some("4ae5f8056db821737a4f40f8816177cd"));
    assert_eq!(exchange.form("redirect_uri"), None);
    assert_eq!(
        server.requests_to("/users/@me")[0].authorization.as_deref(),
        Some("Bearer access-1")
    );

    let session = world.store.mal().expect("stored session");
    assert_eq!(session.access_token, "access-1");
    assert_eq!(session.refresh_token, "refresh-1");
    assert!(session.expires_at > current_unix_timestamp() + 2_000_000);
    assert_eq!(session.profile.unwrap().id, 99);
    assert_eq!(world.store.status().mal.name.as_deref(), Some("mehul_mal"));
}

#[tokio::test]
async fn test_account_anilist_login_checks_the_token_and_stores_it() {
    let server = FakeServer::start(|_| (200, VIEWER.to_string()));
    let world = World::new(&server.base).await;

    let redirect = Redirect::Token {
        access_token: "ani-token".to_string(),
        expires_in: Some(31_536_000),
        state: Some("s1".to_string()),
    };
    let profile = session::complete_login(&world.ctx(), Service::AniList, redirect, "s1", None)
        .await
        .expect("login");
    assert_eq!(profile, Profile { id: 42, name: "mehul".to_string(), avatar_url: Some("https://img.example/v.png".to_string()) });

    let request = &server.requests()[0];
    assert_eq!(request.authorization.as_deref(), Some("Bearer ani-token"));
    assert!(request.body.contains("Viewer"));

    let session = world.store.anilist().expect("stored session");
    assert_eq!(session.token, "ani-token");
    assert!(session.expires_at.unwrap() > current_unix_timestamp() + 31_000_000);
    assert_eq!(session::anilist_token(&world.ctx()).as_deref(), Some("ani-token"));
}

#[tokio::test]
async fn test_account_login_rejects_other_attempts_refusals_and_bad_tokens() {
    let server = FakeServer::start(|request| {
        if request.url.starts_with("/token") {
            (400, r#"{"error":"invalid_grant","message":"Authorization code has expired."}"#.to_string())
        } else {
            (400, INVALID_TOKEN.to_string())
        }
    });
    let world = World::new(&server.base).await;
    let ctx = world.ctx();

    // A redirect that carries another attempt's state is refused before anything is sent.
    for state in [Some("other".to_string()), None] {
        let token = Redirect::Token { access_token: "t".to_string(), expires_in: None, state: state.clone() };
        let error = session::complete_login(&ctx, Service::AniList, token, "s1", None).await.unwrap_err();
        assert!(error.contains("did not belong to this sign-in"), "{}", error);
        let code = Redirect::Code { code: "c".to_string(), state };
        let error = session::complete_login(&ctx, Service::Mal, code, "s2", Some("v")).await.unwrap_err();
        assert!(error.contains("did not belong to this sign-in"), "{}", error);
    }
    assert!(server.requests().is_empty());

    let denied = Redirect::Denied { error: "access_denied".to_string(), description: None };
    assert_eq!(
        session::complete_login(&ctx, Service::Mal, denied, "s2", Some("v")).await.unwrap_err(),
        "MyAnimeList sign-in was not approved."
    );
    assert_eq!(
        session::complete_login(&ctx, Service::AniList, Redirect::Empty, "s1", None).await.unwrap_err(),
        "AniList did not return a sign-in result."
    );
    // A token where a code belongs is not a result either.
    let mismatched = Redirect::Token { access_token: "t".to_string(), expires_in: None, state: Some("s2".to_string()) };
    assert!(session::complete_login(&ctx, Service::Mal, mismatched, "s2", Some("v")).await.is_err());

    let code = Redirect::Code { code: "old".to_string(), state: Some("s2".to_string()) };
    assert_eq!(
        session::complete_login(&ctx, Service::Mal, code, "s2", Some("v")).await.unwrap_err(),
        "MyAnimeList did not accept the sign-in code."
    );
    let token = Redirect::Token { access_token: "bad".to_string(), expires_in: None, state: Some("s1".to_string()) };
    assert_eq!(
        session::complete_login(&ctx, Service::AniList, token, "s1", None).await.unwrap_err(),
        "AniList did not accept the token it issued."
    );

    assert_eq!(world.store.status(), store::AccountsStatus::default());
    assert!(world.recorder.notices().is_empty());
}

// ---- MAL tokens ---------------------------------------------------------------------------

#[tokio::test]
async fn test_account_mal_token_is_refreshed_when_expired() {
    let server = FakeServer::start(|_| (200, tokens("access-2", "refresh-2")));
    let world = World::new(&server.base).await;
    let ctx = world.ctx();
    assert_eq!(session::mal_token(&ctx, None).await.unwrap(), None);

    // Still good for a month: used as it is.
    world.store.save_mal("access-1", "refresh-1", far_future(), &profile("mehul")).unwrap();
    assert_eq!(session::mal_token(&ctx, None).await.unwrap().as_deref(), Some("access-1"));
    assert!(server.requests().is_empty());

    // Expires within the margin: refreshed, and the rotated pair is on disk.
    world.store.save_mal_tokens("access-1", "refresh-1", current_unix_timestamp() + 60).unwrap();
    assert_eq!(session::mal_token(&ctx, None).await.unwrap().as_deref(), Some("access-2"));
    let refresh = &server.requests()[0];
    assert_eq!(refresh.url, "/token");
    assert_eq!(refresh.form("grant_type").as_deref(), Some("refresh_token"));
    assert_eq!(refresh.form("refresh_token").as_deref(), Some("refresh-1"));
    assert_eq!(refresh.form("client_id").as_deref(), Some("4ae5f8056db821737a4f40f8816177cd"));

    let session_now = world.store.mal().unwrap();
    assert_eq!(session_now.access_token, "access-2");
    assert_eq!(session_now.refresh_token, "refresh-2");
    assert!(session_now.expires_at > current_unix_timestamp() + 2_000_000);
    // The profile survives a refresh.
    assert_eq!(session_now.profile.unwrap().name, "mehul");

    assert_eq!(session::mal_token(&ctx, None).await.unwrap().as_deref(), Some("access-2"));
    assert_eq!(server.requests().len(), 1);
    assert!(world.recorder.notices().is_empty());
}

#[tokio::test]
async fn test_account_mal_refused_refresh_signs_out() {
    let server = FakeServer::start(|_| {
        (401, r#"{"error":"invalid_request","message":"The refresh token is invalid."}"#.to_string())
    });
    let world = World::new(&server.base).await;
    world.store.save_mal("access-1", "refresh-1", current_unix_timestamp() - 10, &profile("mehul")).unwrap();

    let result = session::mal_token(&world.ctx(), None).await;
    assert!(matches!(result, Err(AppError::AuthRequired)), "{:?}", result);
    assert!(!world.store.is_connected(Service::Mal));
    assert!(world.settings().get("mal_refresh_token").is_none());
    assert_eq!(
        world.recorder.notices(),
        vec![Notice::SignedOut {
            service: Service::Mal,
            message: "MyAnimeList no longer accepts the saved login. Connect your account again in Settings.".to_string(),
        }]
    );
    // Signed out now: no token, and no further request.
    assert_eq!(session::mal_token(&world.ctx(), None).await.unwrap(), None);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn test_account_mal_refresh_that_cannot_reach_mal_keeps_the_session() {
    // Server error.
    let server = FakeServer::start(|_| (503, "busy".to_string()));
    let world = World::new(&server.base).await;
    world.store.save_mal("access-1", "refresh-1", current_unix_timestamp() - 10, &profile("mehul")).unwrap();
    let result = session::mal_token(&world.ctx(), None).await;
    assert!(matches!(result, Err(AppError::Network(_))), "{:?}", result);
    assert!(world.store.is_connected(Service::Mal));

    // Inside the margin but not expired yet: the old token is still used.
    world.store.save_mal_tokens("access-1", "refresh-1", current_unix_timestamp() + 120).unwrap();
    assert_eq!(session::mal_token(&world.ctx(), None).await.unwrap().as_deref(), Some("access-1"));

    // A block page in front of MAL is not MAL refusing the refresh token.
    let blocked_server = FakeServer::start(|_| (403, "<html>Access denied</html>".to_string()));
    let blocked = World::new(&blocked_server.base).await;
    blocked.store.save_mal("access-1", "refresh-1", current_unix_timestamp() - 10, &profile("mehul")).unwrap();
    assert!(matches!(session::mal_token(&blocked.ctx(), None).await, Err(AppError::Network(_))));
    assert!(blocked.store.is_connected(Service::Mal) && blocked.recorder.notices().is_empty());

    // Offline.
    let offline = World::new(&closed_address()).await;
    offline.store.save_mal("access-1", "refresh-1", current_unix_timestamp() - 10, &profile("mehul")).unwrap();
    let result = session::mal_token(&offline.ctx(), None).await;
    assert!(matches!(result, Err(AppError::Unreachable(_))), "{:?}", result);
    assert_eq!(offline.store.mal().unwrap().refresh_token, "refresh-1");
    assert!(world.recorder.notices().is_empty() && offline.recorder.notices().is_empty());
}

#[tokio::test]
async fn test_account_mal_401_refreshes_once_and_retries() {
    let server = FakeServer::start(|request| {
        if request.url.starts_with("/token") {
            (200, tokens("access-2", "refresh-2"))
        } else if request.authorization.as_deref() == Some("Bearer access-2") {
            (200, r#"{"id":99,"name":"mehul_mal"}"#.to_string())
        } else {
            (401, r#"{"error":"invalid_token"}"#.to_string())
        }
    });
    let world = World::new(&server.base).await;
    let ctx = world.ctx();
    // Not expired by the clock, but MAL has revoked it.
    world.store.save_mal("access-1", "refresh-1", far_future(), &profile("mehul")).unwrap();

    let user = session::with_mal(&ctx, |token| async move { ctx.mal.get_user(&token).await })
        .await
        .expect("retried with the new token");
    assert_eq!(user.name, "mehul_mal");

    let seen: Vec<String> = server
        .requests()
        .iter()
        .map(|r| format!("{} {}", r.url, r.authorization.clone().unwrap_or_default()))
        .collect();
    assert_eq!(
        seen,
        vec!["/v2/users/@me Bearer access-1", "/token ", "/v2/users/@me Bearer access-2"]
    );
    assert_eq!(world.store.mal().unwrap().access_token, "access-2");
    assert_eq!(world.store.mal().unwrap().refresh_token, "refresh-2");
    assert!(world.recorder.notices().is_empty());
}

#[tokio::test]
async fn test_account_mal_401_after_a_refresh_signs_out() {
    let server = FakeServer::start(|request| {
        if request.url.starts_with("/token") {
            (200, tokens("access-2", "refresh-2"))
        } else {
            (401, r#"{"error":"invalid_token"}"#.to_string())
        }
    });
    let world = World::new(&server.base).await;
    let ctx = world.ctx();
    world.store.save_mal("access-1", "refresh-1", far_future(), &profile("mehul")).unwrap();

    let result = session::with_mal(&ctx, |token| async move { ctx.mal.get_user(&token).await }).await;
    assert!(matches!(result, Err(AppError::AuthRequired)));
    assert_eq!(server.requests().len(), 3);
    assert!(!world.store.is_connected(Service::Mal));
    assert!(matches!(
        world.recorder.notices().as_slice(),
        [Notice::SignedOut { service: Service::Mal, .. }]
    ));

    // Not connected: nothing is sent at all.
    let result = session::with_mal(&ctx, |token| async move { ctx.mal.get_user(&token).await }).await;
    assert!(matches!(result, Err(AppError::AuthRequired)));
    assert_eq!(server.requests().len(), 3);
}

// ---- AniList tokens -----------------------------------------------------------------------

#[tokio::test]
async fn test_account_anilist_rejected_token_signs_out() {
    for (status, body) in [(400, INVALID_TOKEN), (401, r#"{"errors":[{"message":"Unauthorized.","status":401}]}"#)] {
        let server = FakeServer::start(move |_| (status, body.to_string()));
        let world = World::new(&server.base).await;
        let ctx = world.ctx();
        world.store.save_anilist("ani-token", Some(far_future()), &profile("mehul")).unwrap();
        write_settings_file(
            world.store.path(),
            &{
                let mut settings = read_settings_file(world.store.path()).unwrap();
                settings["autoplay"] = json!(false);
                settings
            },
        )
        .unwrap();

        let result = session::refresh_anilist_profile(&ctx).await;
        assert!(matches!(result, Err(AppError::AuthRequired)), "{:?}", result);
        assert!(!world.store.is_connected(Service::AniList));
        assert_eq!(world.settings(), json!({ "autoplay": false }));
        assert_eq!(
            world.recorder.notices(),
            vec![Notice::SignedOut {
                service: Service::AniList,
                message: "AniList no longer accepts the saved login. Connect your account again in Settings.".to_string(),
            }]
        );
        assert_eq!(
            session::error_message(Service::AniList, true, &AppError::AuthRequired),
            "AniList no longer accepts the saved login. Connect your account again in Settings."
        );
        assert_eq!(
            session::error_message(Service::AniList, false, &AppError::AuthRequired),
            "AniList is not connected."
        );
    }
}

#[tokio::test]
async fn test_account_anilist_other_failures_keep_the_session() {
    let server = FakeServer::start(|_| (500, r#"{"errors":[{"message":"Internal Server Error","status":500}]}"#.to_string()));
    let world = World::new(&server.base).await;
    world.store.save_anilist("ani-token", Some(far_future()), &profile("mehul")).unwrap();
    assert!(matches!(
        session::refresh_anilist_profile(&world.ctx()).await,
        Err(AppError::Network(_))
    ));
    assert!(world.store.is_connected(Service::AniList));

    let offline = World::new(&closed_address()).await;
    offline.store.save_anilist("ani-token", None, &profile("mehul")).unwrap();
    assert!(matches!(
        session::refresh_anilist_profile(&offline.ctx()).await,
        Err(AppError::Unreachable(_))
    ));
    assert!(offline.store.is_connected(Service::AniList));
    assert!(world.recorder.notices().is_empty() && offline.recorder.notices().is_empty());
}

#[tokio::test]
async fn test_account_anilist_expired_token_signs_out_without_a_request() {
    let server = FakeServer::start(|_| (200, VIEWER.to_string()));
    let world = World::new(&server.base).await;
    world.store.save_anilist("ani-token", Some(current_unix_timestamp() - 5), &profile("mehul")).unwrap();

    assert_eq!(session::anilist_token(&world.ctx()), None);
    assert!(matches!(
        session::refresh_anilist_profile(&world.ctx()).await,
        Err(AppError::AuthRequired)
    ));
    assert!(server.requests().is_empty());
    assert!(!world.store.is_connected(Service::AniList));
    assert_eq!(world.recorder.notices().len(), 1);
}

#[tokio::test]
async fn test_account_anilist_profile_is_refreshed_and_viewer_id_reused() {
    let server = FakeServer::start(|_| (200, VIEWER.to_string()));
    let world = World::new(&server.base).await;
    let ctx = world.ctx();
    // A login from an older version: token and id only.
    write_settings_file(world.store.path(), &json!({ "anilist_token": "ani-token" })).unwrap();
    assert_eq!(world.store.status().anilist.name, None);

    assert_eq!(session::anilist_viewer_id(&ctx).await.unwrap(), 42);
    assert_eq!(world.store.status().anilist.name.as_deref(), Some("mehul"));
    assert_eq!(server.requests().len(), 1);
    // Known now: no second request.
    assert_eq!(session::anilist_viewer_id(&ctx).await.unwrap(), 42);
    assert_eq!(server.requests().len(), 1);
}
