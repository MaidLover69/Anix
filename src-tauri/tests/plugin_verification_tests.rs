//! A plugin whose site sits behind an anti-bot check, from the first refusal to playing through
//! the session the user verified. The in-app browser is played by `BrowserStandIn`, which makes
//! real requests to loopback sites the way a page of the site would; everything else is the
//! app's own code: the plugin engine, its `fetch`, the provider manager and the stream check.

mod plugin_support;

use anix_lib::commands::verification_entries;
use anix_lib::plugins::net::{plugins_for_site, NetContext, WireRequest};
use anix_lib::plugins::registry::{PluginRegistry, RegistryOptions};
use anix_lib::providers::common::with_session_allowance;
use anix_lib::providers::manager::ProviderManager;
use anix_lib::web_session::{self, SessionState, SessionUse, SiteSession, SiteVerification, WebRequest};
use plugin_support::{guarded, BrowserStandIn, Req, Resp, Site, PASS_COOKIE};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "anix-verification-test-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn registry_with(session: Arc<dyn SiteSession>, name: &str) -> PluginRegistry {
    PluginRegistry::open(
        scratch_dir(name),
        RegistryOptions {
            allow_insecure: true,
            allow_private_network: true,
            session,
            ..RegistryOptions::default()
        },
    )
}

fn manager(registry: &PluginRegistry) -> ProviderManager {
    let registry = registry.clone();
    ProviderManager::with_source_offline(Arc::new(move || registry.providers()))
}

/// A plugin for a site whose API (`api`) and player page (`embed`) are on different hosts, both
/// behind a check. What it asks covers what plugins ask: GET with headers, POST with a JSON
/// body, a binary download, a binary upload, a redirect, an error answer and text that is not
/// UTF-8. It checks every answer itself and fails loudly when one is not what the site sent.
const GUARDED_PLUGIN: &str = r#"
class Provider {
  constructor() { this.api = '{{api}}'; this.embed = '{{embed}}'; this.media = $getUserPreference('media'); }
  getSettings() { return { episodeServers: ['main'], supportsDub: false }; }
  async search(opts) {
    const res = await fetch(this.api + '/guarded/search?q=' + encodeURIComponent(opts.query), {
      headers: { 'X-Requested-With': 'XMLHttpRequest', Referer: this.api + '/guarded/home?tab=1', Cookie: 'mine=1', 'User-Agent': 'plugin', Accept: 'application/json' },
    });
    if (!res.ok) throw new Error('search answered HTTP ' + res.status);
    return res.json();
  }
  async findEpisodes(id) {
    const need = (fine, what) => { if (!fine) throw new Error('wrong answer: ' + what); };
    const res = await fetch(this.api + '/guarded/episodes', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id, note: 'café' }),
    });
    need(res.status === 200, 'episodes answered HTTP ' + res.status);
    const list = res.json();

    const missing = await fetch(this.api + '/guarded/missing');
    need(missing.status === 404 && !missing.ok && missing.json().error === 'no such thing', 'error answer ' + missing.status);

    const moved = await fetch(this.api + '/guarded/moved');
    need(moved.status === 200 && moved.redirected && moved.url === this.api + '/guarded/landed' && moved.text() === 'landed', 'redirect ' + moved.url);

    const raw = await $anilili.fetchBytes(this.api + '/guarded/bytes');
    const bytes = $anilili.base64ToBytes(raw.base64);
    need(raw.ok && bytes.length === 260, 'binary length ' + bytes.length);
    for (let i = 0; i < 256; i++) need(bytes[i] === i, 'binary byte ' + i);
    need(bytes[256] === 255 && bytes[257] === 254 && bytes[258] === 0 && bytes[259] === 128, 'binary tail');
    need(raw.headers['Content-Type'] === 'application/octet-stream', 'binary type ' + raw.headers['Content-Type']);

    const latin = await fetch(this.api + '/guarded/latin');
    need(latin.text() === 'café', 'charset text ' + latin.text());
    need(latin.body.length === 4 && latin.body[3] === 0xe9, 'charset bytes');

    const sent = new Uint8Array([0, 255, 128, 10, 13, 34, 92]);
    const echoed = await fetch(this.api + '/guarded/echo', { method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: sent });
    need(echoed.body.length === sent.length && sent.every((b, i) => echoed.body[i] === b), 'binary upload');
    return list;
  }
  async findEpisodeServer(episode, server) {
    const page = await fetch(this.embed + '/guarded/embed?e=' + encodeURIComponent(episode.id) + '&m=' + encodeURIComponent(this.media));
    if (!page.ok) throw new Error('embed answered HTTP ' + page.status);
    return { server, headers: { Referer: this.embed + '/' }, videoSources: [{ url: page.json().file, type: 'mp4', quality: '720p', subtitles: [] }] };
  }
}
"#;

const OPEN_PLUGIN: &str = r#"
class Provider {
  getSettings() { return { episodeServers: [], supportsDub: false }; }
  async search(opts) { return [{ id: 'show', title: opts.query, url: '', subOrDub: 'sub' }]; }
  async findEpisodes(id) { return [{ id: 'ep', number: 1, url: '' }]; }
  async findEpisodeServer(episode, server) {
    return { server, headers: {}, videoSources: [{ url: '{{media}}/video/open.mp4', type: 'mp4', quality: '480p', subtitles: [] }] };
  }
}
"#;

type Seen = Arc<Mutex<Vec<(String, HashMap<String, String>)>>>;

/// The sites of one scenario, each on its own loopback port and so its own origin.
struct World {
    api: Site,
    embed: Site,
    /// A stream host that serves anyone.
    media: Site,
    /// A stream host that answers everyone with its check page.
    walled_media: Site,
    /// Requests the API site answered for a visitor who passed its check.
    seen: Seen,
}

impl World {
    async fn start() -> World {
        let video = |_: &Req| {
            Resp::new(206, "video/mp4", b"\0\0\0\x20ftypisom".to_vec()).with_header("Content-Range", "bytes 0-11/1000")
        };
        let media = Site::start(video).await;
        let walled_media =
            Site::start(|_: &Req| Resp::new(403, "text/html", plugin_support::CHECK_PAGE)).await;
        let embed = Site::start(guarded(|request: &Req| match request.route() {
            "/guarded/embed" => Resp::json(json!({ "file": format!("{}/video/ep.mp4", request.query("m")) })),
            _ => Resp::status(404),
        }))
        .await;

        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let (embed_base, media_base, seen_by_site) = (embed.base(), media.base(), seen.clone());
        let api = Site::start(guarded(move |request: &Req| {
            let base = format!("http://{}", request.header("host"));
            if request.route().starts_with("/guarded/") {
                seen_by_site
                    .lock()
                    .unwrap()
                    .push((format!("{} {}", request.method, request.route()), request.headers.clone()));
            }
            match (request.method.as_str(), request.route()) {
                ("GET", "/index.json") => Resp::json(json!([{
                    "id": "guarded", "name": "Guarded Plugin", "version": "1", "type": "onlinestream-provider",
                    "language": "javascript", "payload": GUARDED_PLUGIN,
                    "userConfig": { "version": 1, "requiresConfig": false, "fields": [
                        { "type": "text", "name": "api", "label": "Site", "default": base },
                        { "type": "text", "name": "embed", "label": "Player", "default": embed_base },
                        { "type": "text", "name": "media", "label": "Streams", "default": media_base },
                    ] }
                }])),
                ("GET", "/open.json") => Resp::json(json!([{
                    "id": "open", "name": "Open Plugin", "version": "1", "type": "onlinestream-provider",
                    "language": "javascript", "payload": OPEN_PLUGIN,
                    "userConfig": { "version": 1, "fields": [{ "type": "text", "name": "media", "label": "Streams", "default": media_base }] }
                }])),
                ("GET", "/guarded/search") => Resp::json(json!([
                    { "id": "show", "title": request.query("q"), "url": "", "subOrDub": "sub" }
                ])),
                ("POST", "/guarded/episodes") => match serde_json::from_slice::<Value>(&request.body) {
                    Ok(body) if body["note"] == "café" => Resp::json(json!([
                        { "id": format!("{}-1", body["id"].as_str().unwrap_or("?")), "number": 1, "url": "" }
                    ])),
                    _ => Resp::status(400),
                },
                ("GET", "/guarded/missing") => Resp::new(404, "application/json", r#"{"error":"no such thing"}"#),
                ("GET", "/guarded/moved") => Resp::status(302).with_header("Location", "/guarded/landed"),
                ("GET", "/guarded/landed") => Resp::text("landed"),
                ("GET", "/guarded/bytes") => {
                    let bytes: Vec<u8> = (0..=255u8).chain([255, 254, 0, 128]).collect();
                    Resp::new(200, "application/octet-stream", bytes)
                }
                ("GET", "/guarded/latin") => Resp::new(200, "text/plain; charset=iso-8859-1", b"caf\xe9".to_vec()),
                ("POST", "/guarded/echo") => Resp::new(200, "application/octet-stream", request.body.clone()),
                _ => Resp::status(404),
            }
        }))
        .await;
        World {
            api,
            embed,
            media,
            walled_media,
            seen,
        }
    }

    fn key(site: &Site) -> String {
        format!("127.0.0.1:{}", site.addr.port())
    }

    fn seen(&self, what: &str) -> Vec<HashMap<String, String>> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(request, _)| request == what)
            .map(|(_, headers)| headers.clone())
            .collect()
    }
}

fn state_of(browser: &BrowserStandIn, site: &Site) -> SessionState {
    browser.state(web_session::site_for_origin(&site.base()).unwrap())
}

async fn play(manager: &ProviderManager, anilist_id: i64, provider: Option<&str>) -> Result<anix_lib::models::SourcesResult, String> {
    manager
        .get_sources_for_episode(anilist_id, None, 1.0, "sub", Some("Guarded Show".to_string()), provider)
        .await
        .map_err(|e| e.to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guarded_site_plays_once_the_user_verified_each_of_its_origins() {
    let world = World::start().await;
    let (api, embed) = (world.api.base(), world.embed.base());
    let browser = BrowserStandIn::new();
    let registry = registry_with(browser.clone(), "guarded");
    registry.add_repo(&world.api.url("/index.json")).await.unwrap();
    let manager = manager(&registry);

    // ---- (1) The plugin picked by name meets the check: the reason says so, and the site is
    // listed for verification under a name the user recognises, for this plugin.
    let failure = play(&manager, 7, Some("guarded")).await.unwrap_err();
    assert!(failure.contains("guarded: needs verification"), "{}", failure);
    assert_eq!(browser.asked_paths(&api), ["GET /guarded/search?q=Guarded%20Show"]);
    assert_eq!(state_of(&browser, &world.api), SessionState::Refused(api.clone()));

    let api_key = World::key(&world.api);
    assert_eq!(plugins_for_site(&api_key), ["guarded"]);
    let label = registry.site_label(&api_key, &api_key);
    assert_eq!(label, format!("Guarded Plugin ({})", api_key));
    let pending = vec![SiteVerification { site: api_key.clone(), label: label.clone(), origin: api.clone() }];
    let entries = verification_entries(pending, |site| plugins_for_site(site));
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].site.as_str(), entries[0].label.as_str()), (api_key.as_str(), label.as_str()));
    assert_eq!(entries[0].providers, ["guarded"]);

    // A lookup across all plugins says the same. While other plugins are still being tried the
    // site that waits for the user is not asked; as the last resort it is asked once more.
    let failure = play(&manager, 7, None).await.unwrap_err();
    assert!(failure.contains("guarded: needs verification"), "{}", failure);
    assert_eq!(browser.asked().len(), 2);

    // ---- (2) The user passes the check of the API's origin. Its requests now go through the
    // session; the player page is another origin with a check of its own.
    browser.verify(&api);
    let failure = play(&manager, 7, Some("guarded")).await.unwrap_err();
    assert!(failure.contains("guarded: needs verification"), "{}", failure);
    assert_eq!(state_of(&browser, &world.api), SessionState::Warm);
    assert_eq!(state_of(&browser, &world.embed), SessionState::Refused(embed.clone()));
    let embed_key = World::key(&world.embed);
    assert_eq!(plugins_for_site(&embed_key), ["guarded"]);
    assert_eq!(registry.site_label(&embed_key, &embed_key), format!("Guarded Plugin ({})", embed_key));

    // Everything the plugin asked of the API arrived the way it sent it. (The plugin checked
    // the answers itself: bytes, redirect, error answer, character set.)
    let asked = browser.asked_paths(&api);
    for expected in [
        "GET /guarded/search?q=Guarded%20Show",
        "POST /guarded/episodes",
        "GET /guarded/missing",
        "GET /guarded/moved",
        "GET /guarded/bytes",
        "GET /guarded/latin",
        "POST /guarded/echo",
    ] {
        assert!(asked.iter().any(|path| path == expected), "{} missing from {:?}", expected, asked);
    }
    let search = world.seen("GET /guarded/search").pop().expect("the search reached the site");
    assert_eq!(search["x-requested-with"], "XMLHttpRequest");
    assert_eq!(search["accept"], "application/json");
    // The session's own cookie, not the plugin's; the plugin's Referer on the site kept.
    assert_eq!(search["cookie"], PASS_COOKIE);
    assert_eq!(search["referer"], format!("{}/guarded/home?tab=1", api));
    assert_ne!(search.get("user-agent").map(String::as_str), Some("plugin"));
    let posted = world.seen("POST /guarded/episodes").pop().expect("the POST reached the site");
    assert_eq!(posted["content-type"], "application/json");
    // Nothing of the session went to the plugin's side: no request by the plain client ever
    // carried the cookie.
    for (origin, request) in browser.asked() {
        assert!(
            !request.headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("cookie")),
            "{} {:?}",
            origin,
            request
        );
    }

    // ---- (3) The second origin verified: the episode plays. The stream itself is fetched by
    // the plain client from its own host, never through a session.
    browser.verify(&embed);
    let found = play(&manager, 7, Some("guarded")).await.unwrap();
    assert_eq!(found.provider.as_deref(), Some("guarded"));
    assert_eq!(found.streams[0].url, world.media.url("/video/ep.mp4"));
    assert_eq!(found.streams[0].stream_type, "mp4");
    assert_eq!(world.media.count("GET /video/ep.mp4"), 1);
    assert!(browser.asked_paths(&world.media.base()).is_empty());
    assert!(browser.asked_paths(&embed).iter().any(|path| path.starts_with("GET /guarded/embed?e=show-1")));

    // A stream host that answers with its own check is named for what it is; no session is
    // tried for it.
    registry
        .set_plugin_config("guarded", &HashMap::from([("media".to_string(), json!(world.walled_media.base()))]))
        .await
        .unwrap();
    let failure = play(&manager, 7, Some("guarded")).await.unwrap_err();
    assert!(
        failure.contains("guarded: stream check failed: 127.0.0.1 answered with its anti-bot check"),
        "{}",
        failure
    );
    assert!(browser.asked_paths(&world.walled_media.base()).is_empty());
    registry.set_plugin_config("guarded", &HashMap::new()).await.unwrap();

    // ---- (4) After a restart there are no windows, but the sites answered before: they are
    // asked through their sessions again without the user being asked, even by a lookup that
    // may not start a session for a site it never reached.
    browser.restart();
    assert_eq!(state_of(&browser, &world.api), SessionState::Resumable);
    let listed = manager.get_all_episodes(8, None, Some("Guarded Show".to_string())).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].sub.len(), 1);
    assert_eq!(state_of(&browser, &world.api), SessionState::Warm);
    assert!(play(&manager, 8, None).await.is_ok());

    // The site stops accepting the session: back to needing verification, until the user
    // verifies again.
    browser.revoke(&api);
    let failure = play(&manager, 9, None).await.unwrap_err();
    assert!(failure.contains("guarded: needs verification"), "{}", failure);
    assert_eq!(state_of(&browser, &world.api), SessionState::Refused(api.clone()));
    browser.verify(&api);
    assert!(play(&manager, 9, None).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lookup_across_all_plugins_reaches_the_check_as_its_last_resort() {
    let world = World::start().await;
    let browser = BrowserStandIn::new();
    let registry = registry_with(browser.clone(), "lastresort");
    registry.add_repo(&world.api.url("/index.json")).await.unwrap();
    let manager = manager(&registry);

    // Listing episodes never loads a site in the browser that was never reached through it.
    assert!(manager.get_all_episodes(3, None, Some("Guarded Show".to_string())).await.unwrap().is_empty());
    assert!(browser.asked().is_empty());

    // Nothing else delivers, so the session is tried, and the reason is the one the Verify
    // button is offered for.
    let failure = play(&manager, 3, None).await.unwrap_err();
    assert!(failure.contains("guarded: needs verification"), "{}", failure);
    assert_eq!(browser.asked_paths(&world.api.base()), ["GET /guarded/search?q=Guarded%20Show"]);
    assert_eq!(plugins_for_site(&World::key(&world.api)), ["guarded"]);

    // With a plugin that delivers, the guarded site's session is never started.
    let quiet = BrowserStandIn::new();
    let both = registry_with(quiet.clone(), "both");
    both.add_repo(&world.api.url("/index.json")).await.unwrap();
    both.add_repo(&world.api.url("/open.json")).await.unwrap();
    let found = play(&self::manager(&both), 3, None).await.unwrap();
    assert_eq!(found.provider.as_deref(), Some("open"));
    assert!(quiet.asked().is_empty());
}

/// Starting a session takes longer than a plugin call is given (a window is created, the site
/// loads in it). On the path that may start one that time is added to the limits; a lookup
/// across all plugins is never held up by it.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_that_is_slow_to_start_gets_its_time_without_stalling_other_plugins() {
    let world = World::start().await;
    let (api, embed) = (world.api.base(), world.embed.base());
    let browser = BrowserStandIn::new();
    browser.verify(&api);
    browser.verify(&embed);
    // Longer than any single plugin call may take; two of them longer than a whole attempt.
    browser.set_start_delay(Duration::from_secs(10));
    let registry = registry_with(browser.clone(), "slow");
    registry.add_repo(&world.api.url("/index.json")).await.unwrap();
    registry.add_repo(&world.api.url("/open.json")).await.unwrap();
    let manager = manager(&registry);

    // The open plugin wins at once. The guarded one only met its check and waits for nothing.
    let started = Instant::now();
    let found = play(&manager, 4, None).await.unwrap();
    assert_eq!(found.provider.as_deref(), Some("open"));
    assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());

    // Picked by name, the guarded plugin is given the time its two sessions need to start.
    let started = Instant::now();
    let found = play(&manager, 4, Some("guarded")).await.unwrap();
    assert_eq!(found.provider.as_deref(), Some("guarded"));
    assert!(started.elapsed() >= Duration::from_secs(20), "took {:?}", started.elapsed());
    assert_eq!(state_of(&browser, &world.embed), SessionState::Warm);

    // With both sessions in use the same lookup is quick again.
    let started = Instant::now();
    assert!(play(&manager, 5, Some("guarded")).await.is_ok());
    assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_that_never_starts_fails_in_bounded_time_with_a_reason() {
    let world = World::start().await;
    let browser = BrowserStandIn::new();
    // Longer than a session is ever waited for.
    browser.set_start_delay(Duration::from_secs(600));
    let net = NetContext::new("stuck", browser.clone(), true);
    let request: WireRequest =
        serde_json::from_value(json!({ "url": world.api.url("/guarded/search?q=a"), "method": "GET" })).unwrap();

    let started = Instant::now();
    let answer = net.perform(request, vec![], SessionUse::Create).await.unwrap();
    // The plugin gets the site's own refusal; the failure names the browser as well.
    assert_eq!(answer.meta.status, 403);
    assert!(started.elapsed() < Duration::from_secs(40), "took {:?}", started.elapsed());
    let reason = net.failure_since(started).expect("a reason").reason();
    assert!(reason.contains("blocked by anti-bot check") && reason.contains("still loading"), "{}", reason);
    // The whole wait counts as session time, and is over.
    assert!(net.session_time_since(started) >= Duration::from_secs(20));
    assert!(!net.waiting_for_session());
}

#[tokio::test]
async fn time_limits_grow_only_by_time_spent_waiting_for_a_session() {
    let base = Duration::from_millis(150);
    let allowance = Duration::from_millis(600);
    let work = |length: u64| async move {
        tokio::time::sleep(Duration::from_millis(length)).await;
        length
    };

    // No session involved: the limit is the base.
    assert_eq!(with_session_allowance(work(50), base, allowance, |_| Duration::ZERO).await, Some(50));
    let started = Instant::now();
    assert_eq!(with_session_allowance(work(2000), base, allowance, |_| Duration::ZERO).await, None);
    assert!(started.elapsed() < Duration::from_millis(1000), "took {:?}", started.elapsed());

    // All of the time was a session starting: the work gets the base on top of it.
    let waited = |since: Instant| since.elapsed();
    assert_eq!(with_session_allowance(work(500), base, allowance, waited).await, Some(500));

    // The allowance has an end.
    let started = Instant::now();
    assert_eq!(with_session_allowance(work(5000), base, allowance, waited).await, None);
    let took = started.elapsed();
    assert!(took >= base + allowance && took < Duration::from_millis(2500), "took {:?}", took);
}

/// What the real site did: its check page reloads itself, and a request that is in flight in
/// the page then fails like a network error instead of being answered with the check. That is
/// still a site waiting for the user, and must be offered for verification.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_page_that_aborts_requests_still_needs_verification() {
    let world = World::start().await;
    let (api, embed) = (world.api.base(), world.embed.base());
    let browser = BrowserStandIn::new();
    browser.set_check_page_aborts(&api);
    let registry = registry_with(browser.clone(), "aborting");
    registry.add_repo(&world.api.url("/index.json")).await.unwrap();
    let manager = manager(&registry);

    for provider in [Some("guarded"), None] {
        let failure = play(&manager, 11, provider).await.unwrap_err();
        assert!(failure.contains("guarded: needs verification"), "{:?}: {}", provider, failure);
        assert!(!failure.contains("did not get through"), "{}", failure);
    }
    assert_eq!(state_of(&browser, &world.api), SessionState::Refused(api.clone()));
    assert_eq!(plugins_for_site(&World::key(&world.api)), ["guarded"]);

    // The verification window's probe: still refused while the page is the check page, passed
    // once the page is the site and the refused request is answered.
    let refused = WebRequest::get("/guarded/search?q=Guarded%20Show");
    assert_eq!(browser.probe(&api, &refused).await, Some(true));
    browser.verify(&api);
    assert_eq!(browser.probe(&api, &refused).await, Some(false));

    // The API answers now; the player page's origin has its own check.
    browser.set_check_page_aborts(&embed);
    let failure = play(&manager, 11, Some("guarded")).await.unwrap_err();
    assert!(failure.contains("guarded: needs verification"), "{}", failure);
    assert_eq!(state_of(&browser, &world.embed), SessionState::Refused(embed.clone()));

    // A request that fails on a page that is the site itself is that request's failure: the
    // site is not listed, and the reason says what happened.
    browser.verify(&embed);
    browser.set_failing_path("/guarded/embed");
    let failure = play(&manager, 11, Some("guarded")).await.unwrap_err();
    assert!(failure.contains("did not get through") && failure.contains("TypeError: Load failed"), "{}", failure);
    assert!(!failure.contains("needs verification"), "{}", failure);
    assert_ne!(state_of(&browser, &world.embed), SessionState::Refused(embed.clone()));
    // Its probe cannot tell yet: the page is the site, but the request does not complete.
    assert_eq!(browser.probe(&embed, &WebRequest::get("/guarded/embed?e=1")).await, None);
}
