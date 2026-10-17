//! Comick: the authorize address, the loopback callback, token refresh with rotating refresh
//! tokens, finding the Comick title of an AniList anime and pushing progress to it. Comick and
//! AniList are a loopback server.

mod account_support;

use anix_lib::accounts;

use accounts::comick::{
    self, anilist_status_from_comick, comick_status, match_hid, progress_episodes, ComickProgress,
    ComickTitle, ExternalIds,
};
use accounts::oauth::{self, ComickCallback, Redirect, Service};
use accounts::session::{self, Notice};
use accounts::store::{self, ComickTokenSet};
use accounts::sync::{self, PendingKind};
use account_support::{far_future, FakeServer, Recorded, World};
use anix_lib::error::AppError;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use url::Url;

fn query_of(address: &str) -> HashMap<String, String> {
    Url::parse(address).unwrap().query_pairs().into_owned().collect()
}

// ---- Authorize address and PKCE -------------------------------------------------------------

#[test]
fn test_comick_pkce_s256_challenge() {
    // Computed independently with Python's hashlib and base64.urlsafe_b64encode, padding cut.
    assert_eq!(
        oauth::pkce_s256_challenge("dBjftJeZ4CVP-mB92K9uhvbuxSG4X4jT7dDPrOI6nhg"),
        "4RWEYJlaWpiwYTQlnbY_8GphHDPJBflseEqcXQQXA4M"
    );
    let verifier = oauth::new_pkce_verifier().unwrap();
    let challenge = oauth::pkce_s256_challenge(&verifier);
    assert_eq!(challenge.len(), 43);
    assert!(!challenge.contains('=') && !challenge.contains('+') && !challenge.contains('/'));
    assert_ne!(challenge, verifier);
}

#[test]
fn test_comick_authorize_url() {
    let address = oauth::comick_authorize_url("client-1", "state-1", "verifier-1");
    assert!(address.starts_with("https://comick.dev/api/auth/oauth2/authorize?"));
    let query = query_of(&address);
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["client_id"], "client-1");
    assert_eq!(query["redirect_uri"], "http://127.0.0.1:47823/oauth/comick/callback");
    assert_eq!(query["redirect_uri"], oauth::COMICK_REDIRECT_URI);
    assert_eq!(query["scope"], "library:read library:write offline_access");
    assert_eq!(query["resource"], "https://api.comick.dev/integrations/v1");
    assert_eq!(query["state"], "state-1");
    assert_eq!(query["code_challenge"], oauth::pkce_s256_challenge("verifier-1"));
    assert_eq!(query["code_challenge_method"], "S256");
    // Spaces are encoded, never sent raw.
    assert!(!address.contains(' '));
}

#[test]
fn test_comick_service_names() {
    assert_eq!(Service::from_key("comick"), Some(Service::Comick));
    assert_eq!(serde_json::to_string(&Service::Comick).unwrap(), "\"comick\"");
    assert_eq!(Service::Comick.label(), "Comick");
    assert!(Service::AniList.available() && Service::Mal.available());
    assert_eq!(Service::Comick.available(), oauth::comick_client_id().is_some());
}

// ---- Loopback callback ----------------------------------------------------------------------

#[test]
fn test_comick_callback_classification() {
    let classify = |target: &str| oauth::classify_comick_callback(target, "s1");

    assert_eq!(
        classify("/oauth/comick/callback?code=abc&state=s1"),
        ComickCallback::Answer(Redirect::Code { code: "abc".into(), state: Some("s1".into()) })
    );
    // A refusal still has to carry this attempt's state.
    assert_eq!(
        classify("/oauth/comick/callback?error=access_denied&state=s1"),
        ComickCallback::Answer(Redirect::Denied { error: "access_denied".into(), description: None })
    );
    assert_eq!(classify("/oauth/comick/callback?error=access_denied"), ComickCallback::WrongState);
    assert_eq!(classify("/oauth/comick/callback?code=abc&state=other"), ComickCallback::WrongState);
    assert_eq!(classify("/oauth/comick/callback?code=abc"), ComickCallback::WrongState);
    assert_eq!(classify("/oauth/comick/callback?state=s1"), ComickCallback::Answer(Redirect::Empty));

    for elsewhere in [
        "/favicon.ico",
        "/",
        "/oauth/comick/callback/",
        "/oauth/comick/callbackx?code=abc&state=s1",
        "/oauth/comick?code=abc&state=s1",
        "http://evil.example/oauth/comick/callback?code=abc&state=s1",
        "",
    ] {
        assert_eq!(classify(elsewhere), ComickCallback::NotCallback, "{}", elsewhere);
    }
}

#[test]
fn test_comick_callback_is_exact() {
    let url = |s: &str| Url::parse(s).unwrap();
    assert!(oauth::is_comick_callback(&url("http://127.0.0.1:47823/oauth/comick/callback?code=a")));
    for other in [
        "https://127.0.0.1:47823/oauth/comick/callback",
        "http://localhost:47823/oauth/comick/callback",
        "http://127.0.0.1:47824/oauth/comick/callback",
        "http://127.0.0.1/oauth/comick/callback",
        "http://127.0.0.1:47823/oauth/comick/other",
    ] {
        assert!(!oauth::is_comick_callback(&url(other)), "{}", other);
        assert_eq!(oauth::parse_redirect(Service::Comick, &url(other)), Redirect::Elsewhere);
    }
    // The AniList and MAL redirects are not Comick's, and the other way round.
    assert_eq!(
        oauth::parse_redirect(Service::Comick, &url("https://localhost/?code=abc&state=s")),
        Redirect::Elsewhere
    );
    assert_eq!(
        oauth::parse_redirect(Service::Mal, &url("http://127.0.0.1:47823/oauth/comick/callback?code=abc")),
        Redirect::Elsewhere
    );
}

// ---- Vocabulary and matching ----------------------------------------------------------------

#[test]
fn test_comick_status_mapping() {
    assert_eq!(comick_status("CURRENT"), 1);
    assert_eq!(comick_status("REPEATING"), 1);
    assert_eq!(comick_status("COMPLETED"), 2);
    assert_eq!(comick_status("PAUSED"), 3);
    assert_eq!(comick_status("DROPPED"), 4);
    assert_eq!(comick_status("PLANNING"), 5);
    for (number, status) in [(1, "CURRENT"), (2, "COMPLETED"), (3, "PAUSED"), (4, "DROPPED"), (5, "PLANNING")] {
        assert_eq!(anilist_status_from_comick(number), status);
        assert_eq!(comick_status(status) as i64, number);
    }
    assert_eq!(anilist_status_from_comick(0), "PLANNING");
}

#[test]
fn test_comick_progress_reading() {
    let p = |number: Option<&str>| ComickProgress { number: number.map(String::from) };
    assert_eq!(progress_episodes(None), Ok(0));
    assert_eq!(progress_episodes(Some(&p(None))), Ok(0));
    assert_eq!(progress_episodes(Some(&p(Some("12")))), Ok(12));
    assert_eq!(progress_episodes(Some(&p(Some(" 12.5 ")))), Ok(12));
    assert_eq!(progress_episodes(Some(&p(Some("0")))), Ok(0));
    assert_eq!(progress_episodes(Some(&p(Some("Special")))), Err(()));
    assert_eq!(progress_episodes(Some(&p(Some("-3")))), Err(()));
}

fn title(hid: &str, anilist: Option<&str>, mal: Option<&str>) -> ComickTitle {
    ComickTitle {
        hid: hid.to_string(),
        title: hid.to_string(),
        external_ids: ExternalIds {
            anilist: anilist.map(String::from),
            myanimelist: mal.map(String::from),
        },
    }
}

#[test]
fn test_comick_hid_matching() {
    let results = vec![
        title("sequel", Some("101"), Some("5101")),
        title("by-mal", None, Some("5100")),
        title("the-one", Some(" 100 "), Some("5100")),
    ];
    // The AniList id wins over a MyAnimeList-only match listed earlier.
    assert_eq!(match_hid(&results, 100, Some(5100)), Some("the-one".to_string()));
    assert_eq!(match_hid(&results, 101, None), Some("sequel".to_string()));

    // Without an AniList match the MyAnimeList id decides, but only for results without an
    // AniList id of their own.
    let mal_only = vec![title("other-show", Some("300"), Some("5100")), title("by-mal", None, Some("5100"))];
    assert_eq!(match_hid(&mal_only, 100, Some(5100)), Some("by-mal".to_string()));
    assert_eq!(match_hid(&mal_only, 100, None), None);
    assert_eq!(match_hid(&[title("x", Some("1000"), None)], 100, None), None);
    assert_eq!(match_hid(&[title("", Some("100"), None)], 100, None), None);
    assert_eq!(match_hid(&[], 100, Some(5100)), None);
}

#[test]
fn test_comick_scope_writes() {
    assert!(comick::scope_allows_writes(Some("library:read library:write offline_access")));
    assert!(!comick::scope_allows_writes(Some("library:read offline_access")));
    assert!(!comick::scope_allows_writes(Some("library:writer")));
    assert!(comick::scope_allows_writes(None));
}

// ---- A small Comick + AniList ---------------------------------------------------------------

#[derive(Default)]
struct Remote {
    /// Comick library entries by HID.
    library: HashMap<String, Value>,
    /// Answer to the next token requests, in order; the last one repeats.
    token_answers: Vec<(u16, String)>,
    /// Comick search results.
    titles: Vec<Value>,
    episodes: Option<i32>,
}

fn tokens(access: &str, refresh: &str) -> (u16, String) {
    (
        200,
        json!({
            "access_token": access,
            "refresh_token": refresh,
            "token_type": "Bearer",
            "expires_in": 900,
            "scope": "library:read library:write offline_access"
        })
        .to_string(),
    )
}

fn entry(hid: &str, status: i64, number: Option<&str>) -> Value {
    json!({
        "hid": hid, "title": "Show", "slug": "show", "media_type": "anime",
        "status": status, "status_name": "watching", "rating": null,
        "progress": number.map(|n| json!({ "unit": "episode", "hid": null, "number": n, "volume": null })),
        "cover_url": null, "total_units": 12, "url": "https://comick.dev/anime/show"
    })
}

fn respond(remote: &Mutex<Remote>, request: &Recorded) -> (u16, String) {
    let mut remote = remote.lock().unwrap();
    let path = request.url.split('?').next().unwrap_or_default().to_string();

    if path == "/graphql" {
        let media = json!({
            "id": 100, "idMal": 5100, "episodes": remote.episodes, "status": "FINISHED",
            "title": { "romaji": "Shou Romaji", "english": "Show", "native": null, "userPreferred": "Show" },
            "synonyms": [], "mediaListEntry": null
        });
        return (200, json!({ "data": { "Media": media } }).to_string());
    }
    if path == "/comick/oauth2/token" {
        if remote.token_answers.len() > 1 {
            return remote.token_answers.remove(0);
        }
        return remote.token_answers.first().cloned().unwrap_or((500, "{}".into()));
    }
    if path == "/comick/oauth2/revoke" {
        return (200, "{}".into());
    }
    if request.authorization.as_deref().map_or(true, |a| !a.starts_with("Bearer ")) {
        return (401, json!({ "error": "unauthorized" }).to_string());
    }
    if path == "/comick/v1/me" {
        return (200, json!({ "id": "1234-uuid", "username": "Reader", "avatar_url": "https://img/a.png" }).to_string());
    }
    if path == "/comick/v1/titles" {
        return (200, json!({ "data": remote.titles }).to_string());
    }
    if let Some(hid) = path.strip_prefix("/comick/v1/me/library/") {
        let hid = hid.to_string();
        return match request.method.as_str() {
            "GET" => match remote.library.get(&hid) {
                Some(found) => (200, json!({ "data": found }).to_string()),
                None => (404, json!({ "error": "not_found" }).to_string()),
            },
            "PUT" => {
                let body = request.json();
                let mut current = remote.library.get(&hid).cloned().unwrap_or_else(|| entry(&hid, 0, None));
                if let Some(status) = body.get("status") {
                    current["status"] = status.clone();
                }
                if let Some(number) = body.get("progress").and_then(|p| p.get("number")) {
                    current["progress"] = json!({ "unit": "episode", "hid": null, "number": number, "volume": null });
                }
                remote.library.insert(hid, current.clone());
                (200, json!({ "data": current }).to_string())
            }
            _ => (405, "{}".into()),
        };
    }
    (404, "{}".into())
}

fn comick_remote(setup: impl FnOnce(&mut Remote)) -> (Arc<Mutex<Remote>>, FakeServer) {
    let mut remote = Remote {
        titles: vec![
            json!({ "hid": "sequel-hid", "title": "Show 2", "external_ids": { "anilist": "101", "myanimelist": null, "mangabaka": null, "mangaupdates": null } }),
            json!({ "hid": "show-hid", "title": "Show", "external_ids": { "anilist": "100", "myanimelist": "5100", "mangabaka": null, "mangaupdates": null } }),
        ],
        episodes: Some(12),
        ..Default::default()
    };
    setup(&mut remote);
    let remote = Arc::new(Mutex::new(remote));
    let shared = Arc::clone(&remote);
    let server = FakeServer::start(move |request| respond(&shared, request));
    (remote, server)
}

fn connect_comick(world: &World, expires_at: i64) {
    world
        .store
        .save_comick(
            &ComickTokenSet {
                access_token: "ck-access",
                refresh_token: Some("ck-refresh"),
                expires_at,
                scope: Some("library:read library:write offline_access"),
            },
            "1234-uuid",
            "Reader",
            None,
        )
        .unwrap();
}

fn puts(server: &FakeServer) -> Vec<(String, Value)> {
    server
        .requests()
        .into_iter()
        .filter(|r| r.method == "PUT")
        .map(|r| (r.url.clone(), r.json()))
        .collect()
}

// ---- Sign-in --------------------------------------------------------------------------------

#[tokio::test]
async fn test_comick_login_exchanges_the_code_and_stores_the_session() {
    let (_remote, server) = comick_remote(|r| r.token_answers = vec![tokens("first-access", "first-refresh")]);
    let world = World::new(&server.base).await;

    let redirect = Redirect::Code { code: "the-code".into(), state: Some("s1".into()) };
    let profile = session::complete_login(&world.ctx(), Service::Comick, redirect, "s1", Some("the-verifier"))
        .await
        .unwrap();
    assert_eq!(profile.name, "Reader");

    let exchange = &server.requests_to("/comick/oauth2/token")[0];
    assert_eq!(exchange.form("grant_type").as_deref(), Some("authorization_code"));
    assert_eq!(exchange.form("client_id").as_deref(), Some("test-comick-client"));
    assert_eq!(exchange.form("code").as_deref(), Some("the-code"));
    assert_eq!(exchange.form("code_verifier").as_deref(), Some("the-verifier"));
    assert_eq!(exchange.form("redirect_uri").as_deref(), Some(oauth::COMICK_REDIRECT_URI));
    assert_eq!(exchange.form("resource").as_deref(), Some(oauth::COMICK_RESOURCE));
    assert!(exchange.form("client_secret").is_none());
    assert_eq!(server.requests_to("/comick/v1/me")[0].authorization.as_deref(), Some("Bearer first-access"));

    let stored = world.store.comick().unwrap();
    assert_eq!(stored.access_token, "first-access");
    assert_eq!(stored.refresh_token.as_deref(), Some("first-refresh"));
    assert_eq!(stored.user_id.as_deref(), Some("1234-uuid"));
    assert!(stored.expires_at > anix_lib::db::current_unix_timestamp() + 800);
    let status = world.store.status();
    assert!(status.comick.connected);
    assert_eq!(status.comick.name.as_deref(), Some("Reader"));
    assert_eq!(status.comick.avatar_url.as_deref(), Some("https://img/a.png"));

    // The tokens never reach the frontend and cannot be overwritten by it.
    let mut for_frontend = world.settings();
    store::redact_secrets(&mut for_frontend);
    assert!(for_frontend.get(store::COMICK_ACCESS_TOKEN).is_none());
    assert!(for_frontend.get(store::COMICK_REFRESH_TOKEN).is_none());
    assert_eq!(for_frontend[store::COMICK_USER_NAME], "Reader");
    let mut incoming = json!({ "comick_access_token": "forged", "autoplay": true });
    store::keep_stored_account_keys(&world.settings(), &mut incoming);
    assert_eq!(incoming[store::COMICK_ACCESS_TOKEN], "first-access");
    assert_eq!(incoming[store::COMICK_REFRESH_TOKEN], "first-refresh");
}

#[tokio::test]
async fn test_comick_login_refusals() {
    let (_remote, server) = comick_remote(|r| {
        r.token_answers = vec![(400, json!({ "error": "invalid_grant" }).to_string())]
    });
    let world = World::new(&server.base).await;
    let ctx = world.ctx();
    let code = || Redirect::Code { code: "c".into(), state: Some("s1".into()) };

    let wrong_state = session::complete_login(&ctx, Service::Comick, code(), "other", Some("v")).await;
    assert!(wrong_state.unwrap_err().contains("did not belong"));
    assert!(server.requests().is_empty());

    assert_eq!(
        session::complete_login(&ctx, Service::Comick, code(), "s1", Some("v")).await.unwrap_err(),
        "Comick did not accept the sign-in code."
    );
    let denied = Redirect::Denied { error: "access_denied".into(), description: None };
    assert_eq!(
        session::complete_login(&ctx, Service::Comick, denied, "s1", Some("v")).await.unwrap_err(),
        "Comick sign-in was not approved."
    );
    assert!(!world.store.is_connected(Service::Comick));
}

// ---- Refresh --------------------------------------------------------------------------------

#[tokio::test]
async fn test_comick_refresh_rotates_both_tokens_in_one_write() {
    let (_remote, server) = comick_remote(|r| r.token_answers = vec![tokens("second-access", "second-refresh")]);
    let world = World::new(&server.base).await;
    connect_comick(&world, 0);

    let token = session::comick_token(&world.ctx(), None).await.unwrap();
    assert_eq!(token.as_deref(), Some("second-access"));
    let refresh = &server.requests_to("/comick/oauth2/token")[0];
    assert_eq!(refresh.form("grant_type").as_deref(), Some("refresh_token"));
    assert_eq!(refresh.form("refresh_token").as_deref(), Some("ck-refresh"));
    assert_eq!(refresh.form("client_id").as_deref(), Some("test-comick-client"));
    assert_eq!(refresh.form("resource").as_deref(), Some(oauth::COMICK_RESOURCE));

    let stored = world.store.comick().unwrap();
    assert_eq!(stored.access_token, "second-access");
    assert_eq!(stored.refresh_token.as_deref(), Some("second-refresh"));
    assert!(stored.expires_at > anix_lib::db::current_unix_timestamp() + 800);
    assert_eq!(stored.user_id.as_deref(), Some("1234-uuid"));

    // Fresh now: no second refresh.
    assert_eq!(session::comick_token(&world.ctx(), None).await.unwrap().as_deref(), Some("second-access"));
    assert_eq!(server.requests_to("/comick/oauth2/token").len(), 1);
}

#[tokio::test]
async fn test_comick_concurrent_refreshes_use_the_refresh_token_once() {
    let (_remote, server) = comick_remote(|r| {
        r.token_answers = vec![tokens("second-access", "second-refresh"), (400, json!({ "error": "invalid_grant" }).to_string())]
    });
    let world = World::new(&server.base).await;
    connect_comick(&world, 0);

    let ctx = world.ctx();
    let (a, b, c) = tokio::join!(
        session::comick_token(&ctx, None),
        session::comick_token(&ctx, None),
        session::comick_token(&ctx, None)
    );
    for result in [a, b, c] {
        assert_eq!(result.unwrap().as_deref(), Some("second-access"));
    }
    assert_eq!(server.requests_to("/comick/oauth2/token").len(), 1);
    assert!(world.store.is_connected(Service::Comick));
}

#[tokio::test]
async fn test_comick_refused_refresh_signs_out() {
    let (_remote, server) = comick_remote(|r| {
        r.token_answers = vec![(400, json!({ "error": "invalid_grant", "error_description": "revoked" }).to_string())]
    });
    let world = World::new(&server.base).await;
    connect_comick(&world, 0);

    assert!(matches!(session::comick_token(&world.ctx(), None).await, Err(AppError::AuthRequired)));
    assert!(!world.store.is_connected(Service::Comick));
    assert!(matches!(
        world.recorder.notices().as_slice(),
        [Notice::SignedOut { service: Service::Comick, .. }]
    ));
}

#[tokio::test]
async fn test_comick_refresh_with_a_lost_answer_signs_out_instead_of_reusing_the_token() {
    // Comick answered 200 but the body is unusable: the old refresh token may be spent.
    let (_remote, server) = comick_remote(|r| r.token_answers = vec![(200, "not json".to_string())]);
    let world = World::new(&server.base).await;
    connect_comick(&world, 0);

    assert!(matches!(session::comick_token(&world.ctx(), None).await, Err(AppError::AuthRequired)));
    assert!(!world.store.is_connected(Service::Comick));
    assert_eq!(server.requests_to("/comick/oauth2/token").len(), 1);
}

#[tokio::test]
async fn test_comick_refresh_comick_did_not_process_keeps_the_session() {
    let (remote, server) = comick_remote(|r| r.token_answers = vec![(503, "{}".to_string())]);
    let world = World::new(&server.base).await;

    // Still valid for a while: the old token is used and the session stays.
    connect_comick(&world, anix_lib::db::current_unix_timestamp() + 30);
    assert_eq!(session::comick_token(&world.ctx(), None).await.unwrap().as_deref(), Some("ck-access"));

    // Expired: an error, but the session and its refresh token are kept for later.
    connect_comick(&world, 0);
    assert!(matches!(session::comick_token(&world.ctx(), None).await, Err(AppError::Network(_))));
    assert_eq!(world.store.comick().unwrap().refresh_token.as_deref(), Some("ck-refresh"));
    assert!(world.recorder.notices().is_empty());

    remote.lock().unwrap().token_answers = vec![tokens("later-access", "later-refresh")];
    assert_eq!(session::comick_token(&world.ctx(), None).await.unwrap().as_deref(), Some("later-access"));
    assert_eq!(world.store.comick().unwrap().refresh_token.as_deref(), Some("later-refresh"));
}

#[tokio::test]
async fn test_comick_session_without_offline_access_ends_with_its_token() {
    let world = World::new("http://127.0.0.1:9").await;
    world
        .store
        .save_comick(
            &ComickTokenSet { access_token: "only", refresh_token: None, expires_at: 0, scope: Some("library:read") },
            "1234-uuid",
            "Reader",
            None,
        )
        .unwrap();
    assert!(matches!(session::comick_token(&world.ctx(), None).await, Err(AppError::AuthRequired)));
    assert!(!world.store.is_connected(Service::Comick));
}

#[tokio::test]
async fn test_comick_revoke_sends_both_tokens() {
    let (_remote, server) = comick_remote(|_| {});
    let world = World::new(&server.base).await;
    connect_comick(&world, far_future());
    let session = world.store.comick().unwrap();

    accounts::revoke_comick(&world.comick, &session).await;
    let revokes = server.requests_to("/comick/oauth2/revoke");
    assert_eq!(revokes.len(), 2);
    assert_eq!(revokes[0].form("token").as_deref(), Some("ck-refresh"));
    assert_eq!(revokes[0].form("token_type_hint").as_deref(), Some("refresh_token"));
    assert_eq!(revokes[1].form("token").as_deref(), Some("ck-access"));
    assert_eq!(revokes[1].form("token_type_hint").as_deref(), Some("access_token"));
    assert!(revokes.iter().all(|r| r.form("client_id").as_deref() == Some("test-comick-client")));
}

// ---- Sync -----------------------------------------------------------------------------------

#[tokio::test]
async fn test_comick_watched_episode_adds_the_title_and_caches_its_hid() {
    let (remote, server) = comick_remote(|_| {});
    let world = World::new(&server.base).await;
    connect_comick(&world, far_future());

    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 3).await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.skipped, report.failed), (1, 0, 0));

    assert_eq!(
        puts(&server),
        vec![("/comick/v1/me/library/show-hid".to_string(), json!({ "status": 1, "progress": { "number": "3" } }))]
    );
    let search = &server.requests_to("/comick/v1/titles")[0];
    let query = query_of(&format!("http://x{}", search.url));
    assert_eq!(query["media_type"], "anime");
    assert_eq!(query["q"], "Shou Romaji");
    assert_eq!(search.authorization.as_deref(), Some("Bearer ck-access"));
    assert_eq!(
        sync::known_comick_hid(&world.pool, 100).await.unwrap(),
        Some(Some("show-hid".to_string()))
    );
    assert_eq!(world.recorder.notices(), vec![Notice::Synced { service: Service::Comick, anilist_id: 100 }]);
    assert!(sync::pending(&world.pool, Service::Comick).await.unwrap().is_empty());

    // The next episode only moves the progress, and the title is not searched again.
    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 4).await.unwrap();
    sync::drain(&world.ctx()).await;
    assert_eq!(puts(&server)[1].1, json!({ "progress": { "number": "4" } }));
    assert_eq!(server.requests_to("/comick/v1/titles").len(), 1);

    // The last episode completes it.
    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 12).await.unwrap();
    sync::drain(&world.ctx()).await;
    assert_eq!(puts(&server)[2].1, json!({ "status": 2, "progress": { "number": "12" } }));
    assert_eq!(remote.lock().unwrap().library["show-hid"]["status"], 2);
}

#[tokio::test]
async fn test_comick_progress_is_never_lowered() {
    let (_remote, server) = comick_remote(|r| {
        r.library.insert("show-hid".into(), entry("show-hid", 1, Some("8")));
    });
    let world = World::new(&server.base).await;
    connect_comick(&world, far_future());

    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 5).await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.skipped), (0, 1));
    assert!(puts(&server).is_empty());

    // A label that is not a number is left alone too.
    let (_remote, server) = comick_remote(|r| {
        r.library.insert("show-hid".into(), entry("show-hid", 1, Some("Movie")));
    });
    let world = World::new(&server.base).await;
    connect_comick(&world, far_future());
    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 5).await.unwrap();
    assert_eq!(sync::drain(&world.ctx()).await.skipped, 1);
    assert!(puts(&server).is_empty());
}

#[tokio::test]
async fn test_comick_watchlist_addition_is_plan_to_watch() {
    let (_remote, server) = comick_remote(|_| {});
    let world = World::new(&server.base).await;
    connect_comick(&world, far_future());

    assert!(sync::queue_watchlist_addition(&world.ctx(), 100).await.unwrap());
    sync::drain(&world.ctx()).await;
    assert_eq!(puts(&server), vec![("/comick/v1/me/library/show-hid".to_string(), json!({ "status": 5 }))]);

    // Already listed: left as it is.
    assert!(sync::queue_watchlist_addition(&world.ctx(), 100).await.unwrap());
    assert_eq!(sync::drain(&world.ctx()).await.skipped, 1);
    assert_eq!(puts(&server).len(), 1);
}

#[tokio::test]
async fn test_comick_title_not_found_is_remembered() {
    let (_remote, server) = comick_remote(|r| {
        r.titles = vec![json!({ "hid": "other", "title": "Other", "external_ids": { "anilist": "999", "myanimelist": null, "mangabaka": null, "mangaupdates": null } })]
    });
    let world = World::new(&server.base).await;
    connect_comick(&world, far_future());

    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 1).await.unwrap();
    assert_eq!(sync::drain(&world.ctx()).await.skipped, 1);
    // Every title of the anime was tried (romaji, then English).
    let searched: Vec<String> = server
        .requests_to("/comick/v1/titles")
        .iter()
        .map(|r| query_of(&format!("http://x{}", r.url))["q"].clone())
        .collect();
    assert_eq!(searched, vec!["Shou Romaji".to_string(), "Show".to_string()]);
    assert_eq!(sync::known_comick_hid(&world.pool, 100).await.unwrap(), Some(None));

    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 2).await.unwrap();
    sync::drain(&world.ctx()).await;
    assert_eq!(server.requests_to("/comick/v1/titles").len(), 2);
    assert!(puts(&server).is_empty());
}

#[tokio::test]
async fn test_comick_read_only_grant_pushes_nothing() {
    let (_remote, server) = comick_remote(|_| {});
    let world = World::new(&server.base).await;
    world
        .store
        .save_comick(
            &ComickTokenSet {
                access_token: "ck-access",
                refresh_token: Some("ck-refresh"),
                expires_at: far_future(),
                scope: Some("library:read offline_access"),
            },
            "1234-uuid",
            "Reader",
            None,
        )
        .unwrap();
    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 1).await.unwrap();
    assert_eq!(sync::drain(&world.ctx()).await.skipped, 1);
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn test_comick_expired_token_is_refreshed_before_a_push() {
    let (_remote, server) = comick_remote(|r| r.token_answers = vec![tokens("second-access", "second-refresh")]);
    let world = World::new(&server.base).await;
    connect_comick(&world, 0);

    sync::enqueue(&world.pool, Service::Comick, 100, PendingKind::Progress, 2).await.unwrap();
    assert_eq!(sync::drain(&world.ctx()).await.updated, 1);
    let put = server.requests().into_iter().find(|r| r.method == "PUT").unwrap();
    assert_eq!(put.authorization.as_deref(), Some("Bearer second-access"));
    assert_eq!(server.requests_to("/comick/oauth2/token").len(), 1);
}

#[tokio::test]
async fn test_comick_library_pages() {
    let pages = Arc::new(Mutex::new(0));
    let seen = Arc::clone(&pages);
    let server = FakeServer::start(move |request| {
        *seen.lock().unwrap() += 1;
        if request.url.contains("cursor=next-1") {
            (200, json!({ "data": [entry("b", 2, Some("12"))], "next_cursor": null }).to_string())
        } else {
            (200, json!({ "data": [entry("a", 1, Some("3"))], "next_cursor": "next-1" }).to_string())
        }
    });
    let world = World::new(&server.base).await;
    connect_comick(&world, far_future());

    let library = sync::comick_library(&world.ctx()).await.unwrap();
    assert_eq!(library.iter().map(|e| e.hid.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    assert_eq!(library[1].status, 2);
    assert_eq!(library[0].progress.as_ref().and_then(|p| p.number.as_deref()), Some("3"));
    let first = &server.requests()[0];
    assert!(first.url.starts_with("/comick/v1/me/library?"));
    assert_eq!(query_of(&format!("http://x{}", first.url))["media_type"], "anime");
    assert_eq!(*pages.lock().unwrap(), 2);
}
