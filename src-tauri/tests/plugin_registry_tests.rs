//! Plugin repositories, storage, and plugins as providers behind the provider manager.

mod plugin_support;

use anix_lib::error::AppError;
use anix_lib::models::Category;
use anix_lib::plugins::provider::servers_for;
use anix_lib::plugins::registry::{index_candidates, valid_plugin_id, DriveEndpoints, PluginRegistry, RegistryOptions};
use anix_lib::providers::manager::{ProviderManager, NO_PLUGIN_MESSAGE};
use anix_lib::web_session::SiteSession;
use plugin_support::{FakeSession, NoSession, Req, Resp, Site};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "anix-plugin-test-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn options(session: Arc<dyn SiteSession>) -> RegistryOptions {
    RegistryOptions {
        allow_insecure: true,
        allow_private_network: true,
        session,
        ..RegistryOptions::default()
    }
}

fn open(dir: &PathBuf) -> PluginRegistry {
    PluginRegistry::open(dir.clone(), options(Arc::new(NoSession)))
}

fn manager(registry: &PluginRegistry) -> ProviderManager {
    let registry = registry.clone();
    ProviderManager::with_source_offline(Arc::new(move || registry.providers()))
}

/// The plugin served by the fixture site. `base` comes from the manifest's user config, the
/// way community plugins take their site address.
const FIXTURE_PLUGIN: &str = r#"
class Provider {
  constructor() { this.base = $getUserPreference('base'); }
  getSettings() { return { episodeServers: ['alpha', 'beta'], supportsDub: true }; }
  async search(opts) {
    const res = await fetch(this.base + '/site/search?q=' + encodeURIComponent(opts.query) + '&dub=' + opts.dub);
    if (!res.ok) throw new Error('search answered HTTP ' + res.status);
    return res.json().map((hit) => ({ id: hit.id + (opts.dub ? '|dub' : '|sub'), title: hit.title, url: this.base + '/site/a/' + hit.id, subOrDub: opts.dub ? 'dub' : 'sub', year: hit.year }));
  }
  async findEpisodes(id) {
    const res = await fetch(this.base + '/site/episodes?id=' + encodeURIComponent(id));
    if (!res.ok) throw new Error('episodes answered HTTP ' + res.status);
    return res.json();
  }
  async findEpisodeServer(episode, server) {
    const res = await fetch(this.base + '/site/server/' + server + '?ep=' + encodeURIComponent(episode.id));
    if (!res.ok) throw new Error(server + ' is down');
    return res.json();
  }
}
"#;

struct Fixture {
    site: Site,
    version: Arc<Mutex<String>>,
    index_broken: Arc<Mutex<bool>>,
    /// What `/site/server/beta` answers with instead of its stream, when set.
    beta_down: Arc<Mutex<bool>>,
}

impl Fixture {
    fn index(&self) -> String {
        self.site.url("/repo/index.json")
    }
}

async fn fixture() -> Fixture {
    let version = Arc::new(Mutex::new("1.0.0".to_string()));
    let index_broken = Arc::new(Mutex::new(false));
    let beta_down = Arc::new(Mutex::new(false));
    let (v, broken, down) = (version.clone(), index_broken.clone(), beta_down.clone());
    let site = Site::start(move |request: &Req| {
        let base = format!("http://{}", request.header("host"));
        let version = v.lock().unwrap().clone();
        match request.route() {
            "/repo/index.json" if *broken.lock().unwrap() => Resp::html("<html>maintenance</html>"),
            "/repo/index.json" => Resp::json(json!([
                {
                    "id": "fixture", "name": "Fixture Site", "version": version, "author": "tests",
                    "description": "A site that exists only in tests", "type": "onlinestream-provider",
                    "language": "javascript", "lang": "en", "icon": "https://example.org/icon.png",
                    "payloadURI": "plugins/fixture.js",
                    "userConfig": { "fields": [{ "name": "base", "default": base }] }
                },
                { "id": "broken", "name": "Broken", "version": "0.1.0", "type": "onlinestream-provider", "language": "javascript", "payloadURI": "plugins/broken.js" },
                { "id": "typed", "name": "Typed", "version": "2.0.0", "type": "onlinestream-provider", "language": "typescript", "manifestURI": format!("{}/repo/typed/manifest.json", base) },
                { "id": "gone", "name": "Gone", "version": "1.0.0", "type": "onlinestream-provider", "language": "javascript", "payloadURI": "plugins/missing.js" },
                { "id": "a-manga-thing", "name": "Manga", "type": "manga-provider", "language": "javascript", "payloadURI": "plugins/manga.js" },
                { "id": "../escape", "name": "Bad id", "version": "1", "type": "onlinestream-provider", "payloadURI": "plugins/fixture.js" }
            ])),
            "/repo/plugins/fixture.js" => {
                let marker = format!("\n// version {}\n", version);
                let source = if version == "1.0.0" {
                    FIXTURE_PLUGIN.to_string()
                } else {
                    FIXTURE_PLUGIN.replace("['alpha', 'beta']", "['alpha', 'beta', 'gamma']")
                };
                Resp::new(200, "text/javascript", source + &marker)
            }
            "/repo/plugins/broken.js" => Resp::new(200, "text/javascript", "class Provider { getSettings( { "),
            "/repo/typed/manifest.json" => Resp::json(json!({
                "id": "typed", "name": "Typed", "version": "2.0.0", "type": "onlinestream-provider", "language": "typescript",
                "payload": "class Provider { getSettings(): { episodeServers: string[]; supportsDub: boolean } { return { episodeServers: [], supportsDub: false } }\n async search(opts: any): Promise<any[]> { return [] } }"
            })),
            "/repo/one.json" => Resp::json(json!({
                "id": "single", "name": "Single", "version": "3.1.0", "type": "onlinestream-provider", "language": "javascript",
                "lang": "ru", "languages": { "dub": ["RU"] },
                "payload": "class Provider { getSettings() { return { episodeServers: ['only'], supportsDub: false }; } }"
            })),
            "/repo/empty.json" => Resp::json(json!([{ "id": "m", "type": "manga-provider" }])),
            "/repo/big.json" => Resp::new(200, "application/json", vec![b' '; 3 * 1024 * 1024]),

            "/site/search" => {
                if request.query("dub") == "true" {
                    Resp::json(json!([{ "id": "right", "title": "Test Show", "year": 2020 }]))
                } else {
                    // The site ranks a sequel first; the right entry is second.
                    Resp::json(json!([
                        { "id": "sequel", "title": "Test Show Season 2", "year": 2022 },
                        { "id": "right", "title": "Test Show", "year": 2020 }
                    ]))
                }
            }
            "/site/episodes" => {
                let id = request.query("id");
                let entry = |n: u32| json!({ "id": format!("{}|{}", id, n), "number": n, "url": format!("{}/site/watch/{}", base, n), "title": format!("Chapter {}", n), "image": format!("{}/thumb/{}.jpg", base, n) });
                match id.as_str() {
                    "right|sub" => Resp::json(json!([entry(2), entry(1), entry(3)])),
                    "right|dub" => Resp::json(json!([entry(1)])),
                    _ => Resp::status(404),
                }
            }
            "/site/server/alpha" => Resp::status(500),
            "/site/server/beta" if *down.lock().unwrap() => Resp::json(json!({
                "server": "beta", "headers": {},
                "videoSources": [{ "url": format!("{}/hls/dead/master.m3u8", base), "type": "m3u8", "quality": "auto", "subtitles": [] }]
            })),
            "/site/server/beta" | "/site/server/gamma" => {
                let episode = request.query("ep");
                let audio = if episode.contains("|dub|") { "dub" } else { "sub" };
                Resp::json(json!({
                    "server": "beta",
                    "headers": { "Referer": format!("{}/site/", base) },
                    "videoSources": [
                        { "url": format!("{}/hls/{}/master.m3u8", base, audio), "type": "m3u8", "quality": "1080p", "label": "Soft sub",
                          "subtitles": [{ "id": "en", "url": format!("{}/subs/{}/en.ass", base, audio), "language": "English", "isDefault": true, "format": "vtt" }] },
                        { "url": format!("{}/hls/{}-low/master.m3u8", base, audio), "type": "m3u8", "quality": "360p",
                          "subtitles": [{ "id": "en", "url": format!("{}/subs/{}/en.ass", base, audio), "language": "English", "isDefault": true }] },
                        { "url": format!("{}/hls/{}-raw/master.m3u8", base, audio), "type": "m3u8", "quality": "raw", "subtitles": [] }
                    ],
                    "skip": { "introStart": 12.5, "introEnd": 98, "outroStart": 1300 }
                }))
            }
            route if route.starts_with("/hls/dead/") => Resp::status(404),
            route if route.ends_with("/master.m3u8") => {
                // The playlist is only served with the Referer the server result names.
                if request.header("referer").ends_with("/site/") {
                    Resp::new(200, "application/vnd.apple.mpegurl", "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nmedia.m3u8\n")
                } else {
                    Resp::status(403)
                }
            }
            route if route.ends_with("/media.m3u8") => Resp::new(200, "application/vnd.apple.mpegurl", "#EXTM3U\n#EXTINF:4,\nseg0.ts\n#EXT-X-ENDLIST\n"),
            route if route.ends_with("/seg0.ts") => Resp::new(200, "video/mp2t", vec![0x47u8; 188]),
            _ => Resp::status(404),
        }
    })
    .await;
    Fixture {
        site,
        version,
        index_broken,
        beta_down,
    }
}

#[test]
fn repository_links_resolve_to_index_addresses() {
    assert_eq!(
        index_candidates("https://github.com/someone/anix-plugins").unwrap(),
        vec![
            "https://raw.githubusercontent.com/someone/anix-plugins/main/index.json",
            "https://raw.githubusercontent.com/someone/anix-plugins/master/index.json",
        ]
    );
    assert_eq!(
        index_candidates("github.com/o/r.git/").unwrap()[0],
        "https://raw.githubusercontent.com/o/r/main/index.json"
    );
    assert_eq!(
        index_candidates("https://github.com/o/r/tree/dev").unwrap()[0],
        "https://raw.githubusercontent.com/o/r/dev/index.json"
    );
    assert_eq!(
        index_candidates("https://github.com/o/r/blob/main/marketplace/main.json").unwrap(),
        vec!["https://raw.githubusercontent.com/o/r/main/marketplace/main.json"]
    );
    assert_eq!(
        index_candidates("https://raw.githubusercontent.com/o/r/main/index.json").unwrap(),
        vec!["https://raw.githubusercontent.com/o/r/main/index.json"]
    );
    assert!(index_candidates("https://").is_err());

    assert!(valid_plugin_id("gojo-wtf_2"));
    for bad in ["", "../x", "a/b", "a b", "a.b", "é", &"x".repeat(65)] {
        assert!(!valid_plugin_id(bad), "{}", bad);
    }
}

#[test]
fn sub_and_dub_servers_are_told_apart_by_name() {
    let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let gg = names(&["GG-SUB", "GG-DUB"]);
    assert_eq!(servers_for(&gg, &Category::Sub), names(&["GG-SUB"]));
    assert_eq!(servers_for(&gg, &Category::Dub), names(&["GG-DUB"]));
    let plain = names(&["kickassanime", "default", "icarus"]);
    assert_eq!(servers_for(&plain, &Category::Dub), names(&["default", "kickassanime", "icarus"]));
    assert_eq!(servers_for(&[], &Category::Sub), names(&["default"]));
    assert_eq!(servers_for(&names(&["Dubbed"]), &Category::Sub), names(&["Dubbed"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repository_is_added_refreshed_and_removed() {
    let fx = fixture().await;
    let dir = scratch_dir("repo");
    let registry = open(&dir);
    let changes = Arc::new(AtomicUsize::new(0));
    let counter = changes.clone();
    registry.set_notifier(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(registry.list_repos().is_empty());

    // Links that are not plugin lists are refused and leave nothing behind.
    let other_json = registry.add_repo(&fx.site.url("/site/search")).await.unwrap_err();
    assert!(other_json.contains("no streaming plugins"), "{}", other_json);
    let not_json = registry.add_repo(&fx.site.url("/repo/plugins/fixture.js")).await.unwrap_err();
    assert!(not_json.contains("not JSON"), "{}", not_json);
    let missing = registry.add_repo(&fx.site.url("/repo/nothing.json")).await.unwrap_err();
    assert!(missing.contains("HTTP 404"), "{}", missing);
    let empty = registry.add_repo(&fx.site.url("/repo/empty.json")).await.unwrap_err();
    assert!(empty.contains("no streaming plugins"), "{}", empty);
    let big = registry.add_repo(&fx.site.url("/repo/big.json")).await.unwrap_err();
    assert!(big.contains("larger than"), "{}", big);
    let insecure = PluginRegistry::open(scratch_dir("strict"), RegistryOptions { session: Arc::new(NoSession), ..RegistryOptions::default() });
    let refused = insecure.add_repo(&fx.index()).await.unwrap_err();
    assert!(refused.contains("only https"), "{}", refused);
    assert!(registry.list_repos().is_empty());
    assert!(registry.list_plugins().is_empty());

    let link = format!("  {}  ", fx.index());
    let repo = registry.add_repo(&link).await.unwrap();
    assert_eq!(repo.url, fx.index());
    assert_eq!(repo.plugin_count, 4);
    assert!(repo.updated_at_ms > 0);
    let note = repo.error.clone().unwrap();
    assert!(note.contains("not a usable plugin id"), "{}", note);
    assert!(registry.add_repo(&fx.index()).await.unwrap_err().contains("already been added"));

    let plugins = registry.list_plugins();
    let by_id = |id: &str| plugins.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("{} missing", id)).clone();
    assert_eq!(plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["fixture", "broken", "typed", "gone"]);
    let fixture = by_id("fixture");
    assert_eq!((fixture.name.as_str(), fixture.version.as_str(), fixture.author.as_str(), fixture.lang.as_str()), ("Fixture Site", "1.0.0", "tests", "en"));
    assert_eq!(fixture.repo_url, repo.url);
    assert_eq!(fixture.status, "ready");
    assert!(fixture.enabled && fixture.supports_dub);
    assert_eq!(fixture.servers, ["alpha", "beta"]);
    assert_eq!(fixture.icon.as_deref(), Some("https://example.org/icon.png"));
    // No `languages` in the manifest: its `lang` covers the sub, and the dub it has.
    assert_eq!((fixture.sub_languages.clone(), fixture.dub_languages.clone()), (vec!["en".to_string()], vec!["en".to_string()]));
    assert!(by_id("typed").sub_languages.is_empty() && by_id("typed").dub_languages.is_empty());
    // A plugin that cannot run is listed with its reason and does not disturb the others.
    let broken = by_id("broken");
    assert_eq!(broken.status, "error");
    assert!(broken.error.is_some());
    assert_eq!((broken.author.as_str(), broken.description.as_str(), broken.lang.as_str()), ("", "", ""));
    assert_eq!(by_id("typed").status, "ready");
    let gone = by_id("gone");
    assert_eq!(gone.status, "error");
    assert!(gone.error.unwrap().contains("HTTP 404"));
    assert_eq!(manager(&registry).provider_names(), ["fixture", "typed"]);
    assert!(dir.join("payloads/fixture.js").is_file());
    assert!(dir.join("payloads/typed.ts").is_file());
    assert!(!dir.join("payloads").join("..").join("escape.js").exists());

    // Same version: nothing is downloaded again.
    let payload_hits = fx.site.count("/repo/plugins/fixture.js");
    registry.refresh_repos().await;
    assert_eq!(fx.site.count("/repo/plugins/fixture.js"), payload_hits);

    // A new version is picked up.
    *fx.version.lock().unwrap() = "1.1.0".to_string();
    let refreshed = registry.refresh_repos().await;
    assert_eq!(refreshed.len(), 1);
    assert_eq!(fx.site.count("/repo/plugins/fixture.js"), payload_hits + 1);
    let fixture = registry.list_plugins().into_iter().find(|p| p.id == "fixture").unwrap();
    assert_eq!(fixture.version, "1.1.0");
    assert_eq!(fixture.servers, ["alpha", "beta", "gamma"]);
    assert!(std::fs::read_to_string(dir.join("payloads/fixture.js")).unwrap().contains("// version 1.1.0"));

    // An index that stops being one is reported; the installed plugins stay.
    *fx.index_broken.lock().unwrap() = true;
    let refreshed = registry.refresh_repos().await;
    assert!(refreshed[0].error.as_deref().unwrap().contains("not JSON"), "{:?}", refreshed[0].error);
    assert_eq!(refreshed[0].plugin_count, 4);
    assert_eq!(manager(&registry).provider_names(), ["fixture", "typed"]);
    *fx.index_broken.lock().unwrap() = false;

    // A single manifest is a repository of one.
    let single = registry.add_repo(&fx.site.url("/repo/one.json")).await.unwrap();
    assert_eq!(single.plugin_count, 1);
    assert_eq!(single.error, None);
    assert_eq!(manager(&registry).provider_names(), ["fixture", "typed", "single"]);
    // Declared languages win over `lang`; a dub-only site without `supportsDub` keeps its dub.
    let listed = registry.list_plugins();
    let declared = listed.iter().find(|p| p.id == "single").unwrap();
    assert!(!declared.supports_dub);
    assert!(declared.sub_languages.is_empty());
    assert_eq!(declared.dub_languages, ["ru"]);
    let as_provider = registry.providers().into_iter().find(|p| p.name() == "single").unwrap();
    assert_eq!(as_provider.languages().map(|l| l.dub), Some(vec!["ru".to_string()]));

    assert!(changes.load(Ordering::SeqCst) >= 5);
    registry.remove_repo(&fx.index()).await.unwrap();
    assert_eq!(registry.list_repos().len(), 1);
    assert_eq!(registry.list_plugins().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["single"]);
    assert!(!dir.join("payloads/fixture.js").exists());
    assert_eq!(manager(&registry).provider_names(), ["single"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn plugins_serve_episodes_and_sources_through_the_manager() {
    let fx = fixture().await;
    let registry = open(&scratch_dir("e2e"));

    // No plugin: no providers, an empty episode map and a message that points to Settings.
    let empty = manager(&registry);
    assert!(empty.get_all_episodes(77, None, Some("Test Show".to_string())).await.unwrap().is_empty());
    let none = empty.get_sources_for_episode(77, None, 1.0, "sub", Some("Test Show".to_string()), None).await.unwrap_err();
    assert_eq!(none.to_string(), NO_PLUGIN_MESSAGE);
    assert!(NO_PLUGIN_MESSAGE.contains("Settings → Plugins"));

    registry.add_repo(&fx.index()).await.unwrap();
    let manager = manager(&registry);

    let lists: Vec<_> = manager
        .get_all_episodes(77, None, Some("Test Show".to_string()))
        .await
        .unwrap()
        .into_iter()
        .filter(|list| !list.is_empty())
        .collect();
    assert_eq!(lists.len(), 1, "only the plugin that has the anime lists episodes");
    assert_eq!(lists[0].name, "fixture");
    assert_eq!(lists[0].sub.iter().map(|e| e.number).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
    assert_eq!(lists[0].dub.iter().map(|e| e.number).collect::<Vec<_>>(), [1.0]);
    assert_eq!(lists[0].sub[0].title.as_deref(), Some("Chapter 1"));
    assert!(lists[0].sub[0].image.as_deref().unwrap().ends_with("/thumb/1.jpg"));
    // The sequel the site ranks first was not taken.
    assert_eq!(fx.site.count("/site/episodes?id=sequel"), 0);
    assert_eq!(fx.site.count("/site/episodes?id=right%7Csub"), 1);
    manager.get_all_episodes(77, None, Some("Test Show".to_string())).await.unwrap();
    assert_eq!(fx.site.count("/site/episodes?id=right%7Csub"), 1, "episode lists are cached");

    let sub = manager.get_sources_for_episode(77, None, 2.0, "sub", Some("Test Show".to_string()), None).await.unwrap();
    assert_eq!(sub.provider.as_deref(), Some("fixture"));
    let first = &sub.streams[0];
    assert!(first.url.contains("/hls/sub"), "{}", first.url);
    assert_eq!(first.stream_type, "hls");
    assert_eq!(first.audio.as_deref(), Some("sub"));
    assert!(first.referer.as_deref().unwrap().ends_with("/site/"));
    assert!(first.headers.as_ref().unwrap().contains_key("Referer"));
    assert!(first.is_active);
    // The two streams that share the English track are offered; the one without it is not,
    // because the subtitles returned would not belong to it.
    let mut urls: Vec<&str> = sub.streams.iter().map(|s| s.url.rsplit("/hls/").next().unwrap()).collect();
    urls.sort();
    assert_eq!(urls, ["sub-low/master.m3u8", "sub/master.m3u8"]);
    let labelled = sub.streams.iter().find(|s| s.url.contains("/hls/sub/")).unwrap();
    assert_eq!(labelled.quality.as_deref(), Some("1080p - Soft sub"));
    assert_eq!(labelled.subtitle_variant.as_deref(), Some("s-sub"));
    assert_eq!(sub.subtitles.len(), 1);
    assert!(sub.subtitles[0].url.ends_with("/subs/sub/en.ass"));
    assert_eq!(sub.subtitles[0].language, "English");
    assert_eq!(sub.subtitles[0].is_default, Some(true));
    let skip = sub.skip.clone().unwrap();
    assert_eq!((skip.intro_start, skip.intro_end), (Some(12.5), Some(98.0)));
    assert_eq!((skip.outro_start, skip.outro_end), (None, None));

    let dub = manager.get_sources_for_episode(77, None, 1.0, "dub", Some("Test Show".to_string()), Some("fixture")).await.unwrap();
    assert!(dub.streams[0].url.contains("/hls/dub"));
    assert_eq!(dub.streams[0].audio.as_deref(), Some("dub"));
    assert!(dub.subtitles[0].url.ends_with("/subs/dub/en.ass"));

    // Failures name the plugin and say why.
    let reason = |result: Result<_, AppError>| -> String { result.map(|_: anix_lib::models::SourcesResult| ()).unwrap_err().to_string() };
    let no_dub = reason(manager.get_sources_for_episode(77, None, 3.0, "dub", Some("Test Show".to_string()), None).await);
    assert!(no_dub.contains("fixture: episode 3 not listed"), "{}", no_dub);
    assert!(no_dub.contains("typed: no dub"), "{}", no_dub);
    let unknown = reason(manager.get_sources_for_episode(78, None, 1.0, "sub", Some("Another Show Entirely".to_string()), None).await);
    assert!(unknown.contains("fixture, typed: anime not found"), "{}", unknown);
    *fx.beta_down.lock().unwrap() = true;
    let down = reason(manager.get_sources_for_episode(77, None, 1.0, "sub", Some("Test Show".to_string()), Some("fixture")).await);
    assert!(down.contains("alpha is down"), "{}", down);
    assert!(down.contains("stream check failed: HTTP 404"), "{}", down);
    *fx.beta_down.lock().unwrap() = false;

    // A disabled plugin takes no part, without a restart; enabling brings it back.
    registry.set_enabled("fixture", false).await.unwrap();
    assert_eq!(registry.list_plugins()[0].status, "disabled");
    assert_eq!(manager.provider_names(), ["typed"]);
    assert!(manager.get_all_episodes(77, None, Some("Test Show".to_string())).await.unwrap().iter().all(|list| list.is_empty()));
    let gone = reason(manager.get_sources_for_episode(77, None, 1.0, "sub", Some("Test Show".to_string()), Some("fixture")).await);
    assert!(gone.contains("Unknown provider 'fixture'"), "{}", gone);
    registry.set_enabled("fixture", true).await.unwrap();
    assert_eq!(manager.provider_names(), ["fixture", "typed"]);
    assert!(manager.get_sources_for_episode(77, None, 1.0, "sub", Some("Test Show".to_string()), None).await.is_ok());
    assert!(registry.set_enabled("nope", true).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn installed_plugins_survive_a_restart_and_work_offline() {
    let dir = scratch_dir("restart");
    let index;
    {
        let fx = fixture().await;
        index = fx.index();
        let registry = open(&dir);
        registry.add_repo(&index).await.unwrap();
        registry.add_repo(&fx.site.url("/repo/one.json")).await.unwrap();
        registry.set_enabled("typed", false).await.unwrap();
    }

    // The repository's server is gone; a new process opens the same directory.
    let registry = open(&dir);
    assert_eq!(registry.list_repos().len(), 2);
    let plugins = registry.list_plugins();
    assert_eq!(plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["fixture", "broken", "typed", "gone", "single"]);
    assert_eq!(plugins[0].status, "ready");
    assert_eq!(plugins[0].servers, ["alpha", "beta"]);
    assert_eq!(plugins[1].status, "error");
    assert_eq!(plugins[2].status, "disabled");
    // Providers are there before anything was loaded, and loading confirms them.
    assert_eq!(manager(&registry).provider_names(), ["fixture", "single"]);
    registry.load_all().await;
    assert_eq!(manager(&registry).provider_names(), ["fixture", "single"]);
    let settings = registry
        .runtime("single")
        .unwrap()
        .call("getSettings", &[], std::time::Duration::from_secs(5), anix_lib::web_session::SessionUse::Existing)
        .await
        .unwrap();
    assert_eq!(settings["episodeServers"], json!(["only"]));

    // Refreshing while the repository is unreachable reports it and keeps everything.
    let refreshed = registry.refresh_repos().await;
    assert!(refreshed.iter().all(|repo| repo.error.is_some()), "{:?}", refreshed);
    assert_eq!(registry.list_plugins().len(), 5);
    assert_eq!(manager(&registry).provider_names(), ["fixture", "single"]);

    // A damaged state file does not stop the app; it starts without plugins.
    std::fs::write(dir.join("state.json"), "{ not json").unwrap();
    let damaged = open(&dir);
    assert!(damaged.list_plugins().is_empty());
    assert!(dir.join("state.json.corrupt").is_file());
}

const GUARDED_PLUGIN: &str = r#"
class Provider {
  constructor() { this.base = $getUserPreference('base'); }
  getSettings() { return { episodeServers: [], supportsDub: false }; }
  async search(opts) {
    const res = await fetch(this.base + '/guarded/search?q=' + encodeURIComponent(opts.query));
    if (!res.ok) throw new Error('HTTP ' + res.status);
    return res.json();
  }
  async findEpisodes(id) { return [{ id: id + '-1', number: 1, url: this.base + '/watch/1' }]; }
  async findEpisodeServer(episode, server) {
    return { server, headers: {}, videoSources: [{ url: this.base + '/hls/open/master.m3u8', type: 'm3u8', quality: 'auto', subtitles: [] }] };
  }
}
"#;

async fn guarded_site() -> Site {
    Site::start(|request: &Req| {
        let base = format!("http://{}", request.header("host"));
        match request.route() {
            "/index.json" => Resp::json(json!([{
                "id": "guarded", "name": "Guarded", "version": "1", "type": "onlinestream-provider", "language": "javascript",
                "payload": GUARDED_PLUGIN, "userConfig": { "fields": [{ "name": "base", "default": base }] }
            }])),
            "/guarded/search" => Resp::new(403, "text/html", "<html><head><title>Just a moment...</title></head><body>challenge-platform</body></html>"),
            route if route.ends_with("/master.m3u8") => Resp::new(200, "application/vnd.apple.mpegurl", "#EXTM3U\n#EXTINF:4,\nseg0.ts\n"),
            route if route.ends_with("/seg0.ts") => Resp::new(200, "video/mp2t", vec![0x47u8; 188]),
            _ => Resp::status(404),
        }
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_anti_bot_page_is_answered_through_the_browser_session_by_the_usual_policy() {
    let site = guarded_site().await;
    let session = FakeSession::answering(r#"[{"id":"show","title":"Guarded Show","url":"x","subOrDub":"sub"}]"#);
    let registry = PluginRegistry::open(scratch_dir("guarded"), options(session.clone()));
    registry.add_repo(&site.url("/index.json")).await.unwrap();
    let manager = manager(&registry);

    // Listing never starts a session: the plugin's refusal is all there is.
    assert!(manager.get_all_episodes(5, None, Some("Guarded Show".to_string())).await.unwrap().is_empty());
    assert!(session.asked().is_empty());

    // A lookup across all plugins asks the session only as the last resort, and then succeeds.
    let found = manager.get_sources_for_episode(5, None, 1.0, "sub", Some("Guarded Show".to_string()), None).await.unwrap();
    assert_eq!(found.provider.as_deref(), Some("guarded"));
    assert_eq!(session.asked(), ["/guarded/search?q=Guarded%20Show"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_site_that_refuses_the_browser_too_is_reported_as_needing_verification() {
    let site = guarded_site().await;
    let session = FakeSession::refusing();
    let registry = PluginRegistry::open(scratch_dir("refused"), options(session.clone()));
    registry.add_repo(&site.url("/index.json")).await.unwrap();
    let manager = manager(&registry);

    let failure = manager
        .get_sources_for_episode(6, None, 1.0, "sub", Some("Guarded Show".to_string()), Some("guarded"))
        .await
        .unwrap_err()
        .to_string();
    assert!(failure.contains("guarded: needs verification"), "{}", failure);
    assert!(!session.asked().is_empty());
    let key = format!("127.0.0.1:{}", site.addr.port());
    assert_eq!(anix_lib::plugins::net::plugins_for_site(&key), ["guarded"]);

    // Without a session at all the plugin's failure is still named for what it is.
    let plain = PluginRegistry::open(scratch_dir("nosession"), options(Arc::new(NoSession)));
    plain.add_repo(&site.url("/index.json")).await.unwrap();
    let blocked = self::manager(&plain)
        .get_sources_for_episode(6, None, 1.0, "sub", Some("Guarded Show".to_string()), None)
        .await
        .unwrap_err()
        .to_string();
    assert!(blocked.contains("guarded: blocked by anti-bot check"), "{}", blocked);
}

// ---------------------------------------------------------------- plugin settings (userConfig)

mod settings {
    use super::*;
    use anix_lib::plugins::userconfig::{self, ConfigError, SavedConfig, UserConfig};
    use std::collections::HashMap;

    fn config(manifest: serde_json::Value) -> UserConfig {
        UserConfig::from_manifest(&manifest).expect("a userConfig")
    }

    fn saved(version: i64, values: &[(&str, &str)]) -> SavedConfig {
        SavedConfig {
            version,
            values: values.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
        }
    }

    /// The rules of Seanime's `loadUserConfig`.
    #[test]
    fn placeholders_are_replaced_the_way_seanime_replaces_them() {
        let manifest = json!({ "userConfig": { "version": 2, "requiresConfig": false, "fields": [
            { "type": "text", "name": "domain", "label": "Domain", "default": "https://site.example" },
            { "type": "select", "name": "server", "label": "Server", "default": "a",
              "options": [{ "value": "a", "label": "Alpha" }, { "value": "b", "label": "Beta" }] },
            { "type": "switch", "name": "dub", "label": "Dub", "default": "false" },
            { "type": "text", "name": "blank", "label": "Blank" },
            { "name": "count", "default": 3 },
            { "label": "nameless" },
        ] } });
        let config = config(manifest);
        assert_eq!(config.version, 2);
        assert_eq!(config.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["domain", "server", "dub", "blank", "count"]);
        assert_eq!(config.fields[1].options.len(), 2);
        assert_eq!((config.fields[4].kind.as_str(), config.fields[4].default.as_str(), config.fields[4].label.as_str()), ("text", "3", "count"));

        let payload = "const u = '{{domain}}/api/{{server}}'; const again = \"{{domain}}\"; const n = {{count}}; const d = {{dub}}; const b = '{{blank}}'; const other = '{{unknown}}';";

        // Nothing saved, not required: the defaults, every occurrence, unknown names untouched.
        let (code, values) = userconfig::apply(payload, Some(&config), None).unwrap();
        assert_eq!(
            code,
            "const u = 'https://site.example/api/a'; const again = \"https://site.example\"; const n = 3; const d = false; const b = ''; const other = '{{unknown}}';"
        );
        assert_eq!(values["domain"], "https://site.example");
        assert_eq!(values["blank"], "");
        assert_eq!(values.len(), 5);

        // Saved values win field by field; a field without one keeps its default.
        let (code, values) =
            userconfig::apply(payload, Some(&config), Some(&saved(2, &[("domain", "https://mirror.example"), ("dub", "true")]))).unwrap();
        assert!(code.starts_with("const u = 'https://mirror.example/api/a'; const again = \"https://mirror.example\";"), "{}", code);
        assert!(code.contains("const d = true;"));
        assert_eq!((values["domain"].as_str(), values["server"].as_str()), ("https://mirror.example", "a"));

        // The value goes in as it is, as in Seanime: no quoting or escaping of any kind.
        let (code, _) = userconfig::apply("x = '{{domain}}'", Some(&config), Some(&saved(2, &[("domain", "a'b\\c\"d")]))).unwrap();
        assert_eq!(code, "x = 'a'b\\c\"d'");
        // Fields are replaced one after the other, so a value can name a later field.
        let (code, _) = userconfig::apply("{{domain}}", Some(&config), Some(&saved(2, &[("domain", "{{server}}!")]))).unwrap();
        assert_eq!(code, "a!");

        // Values saved for another version of the configuration are not used.
        assert_eq!(userconfig::apply(payload, Some(&config), Some(&saved(1, &[]))).unwrap_err(), ConfigError::Incompatible);

        // A plugin that requires configuration does not run on defaults.
        let mut required = config.clone();
        required.requires_config = true;
        assert_eq!(userconfig::apply(payload, Some(&required), None).unwrap_err(), ConfigError::Missing);
        assert!(userconfig::apply(payload, Some(&required), Some(&saved(2, &[]))).is_ok());

        // No userConfig: the code is left alone, braces and all.
        let (code, values) = userconfig::apply(payload, None, None).unwrap();
        assert_eq!(code, payload);
        assert!(values.is_empty());
        assert!(UserConfig::from_manifest(&json!({ "id": "x" })).is_none());

        // What is saved for the user: known fields only, as text, for the current version.
        let entered: HashMap<String, serde_json::Value> = HashMap::from([
            ("domain".to_string(), json!("https://new.example")),
            ("dub".to_string(), json!(true)),
            ("count".to_string(), json!(12)),
            ("server".to_string(), serde_json::Value::Null),
            ("nonsense".to_string(), json!("x")),
        ]);
        assert_eq!(config.saved_from(&entered), saved(2, &[("domain", "https://new.example"), ("dub", "true"), ("count", "12")]));
    }

    const CONFIGURED_PLUGIN: &str = r#"
class Provider {
  getSettings() { return { episodeServers: ['{{mode}}-server', $getUserPreference('mode') + '-pref', String({{count}} + 1)], supportsDub: {{dub}} }; }
  async search() { return []; }
  async findEpisodes() { return []; }
  async findEpisodeServer() { return null; }
}
"#;

    #[tokio::test(flavor = "multi_thread")]
    async fn settings_are_stored_applied_and_reload_the_plugin() {
        let version = Arc::new(AtomicUsize::new(1));
        let served = version.clone();
        let site = Site::start(move |request: &Req| match request.route() {
            "/index.json" => Resp::json(json!([
                { "id": "configured", "name": "Configured", "version": "1", "type": "onlinestream-provider", "language": "javascript",
                  "payload": CONFIGURED_PLUGIN,
                  "userConfig": { "version": served.load(Ordering::SeqCst), "requiresConfig": false, "fields": [
                      { "type": "select", "name": "mode", "label": "Mode", "default": "a",
                        "options": [{ "value": "a", "label": "Alpha" }, { "value": "b", "label": "Beta" }] },
                      { "type": "text", "name": "count", "label": "Count", "default": "3" },
                      { "type": "switch", "name": "dub", "label": "Dub", "default": "false" },
                  ] } },
                { "id": "needy", "name": "Needy", "version": "1", "type": "onlinestream-provider", "language": "javascript",
                  "payload": "class Provider { getSettings() { return { episodeServers: ['{{where}}'], supportsDub: false }; } }",
                  "userConfig": { "version": 1, "requiresConfig": true, "fields": [
                      { "type": "text", "name": "where", "label": "Where", "default": "somewhere" } ] } },
                { "id": "plain", "name": "Plain", "version": "1", "type": "onlinestream-provider", "language": "javascript",
                  "payload": "class Provider { getSettings() { return { episodeServers: ['{{kept}}'], supportsDub: false }; } }" },
            ])),
            _ => Resp::status(404),
        })
        .await;
        let dir = scratch_dir("settings");
        let registry = open(&dir);
        registry.add_repo(&site.url("/index.json")).await.unwrap();
        let info = |registry: &PluginRegistry, id: &str| registry.list_plugins().into_iter().find(|p| p.id == id).unwrap();

        // Installed with nothing saved: the defaults are in the code and behind $getUserPreference.
        let configured = info(&registry, "configured");
        assert_eq!((configured.status.as_str(), configured.configurable), ("ready", true));
        assert_eq!(configured.servers, ["a-server", "a-pref", "4"]);
        assert!(!configured.supports_dub);
        // A plugin without settings keeps its braces and has nothing to configure.
        let plain = info(&registry, "plain");
        assert_eq!((plain.servers.as_slice(), plain.configurable), (&["{{kept}}".to_string()][..], false));
        // One that requires settings waits for them.
        let needy = info(&registry, "needy");
        assert_eq!(needy.status, "error");
        assert_eq!(needy.error.as_deref(), Some("This plugin has to be configured before it can be used."));
        assert!(!registry.providers().iter().any(|p| p.name() == "needy"));

        // What the settings form is given.
        assert_eq!(
            serde_json::to_value(registry.plugin_config("configured").unwrap()).unwrap(),
            json!({
                "fields": [
                    { "name": "mode", "label": "Mode", "type": "select", "default": "a", "value": "a",
                      "options": [{ "value": "a", "label": "Alpha" }, { "value": "b", "label": "Beta" }] },
                    { "name": "count", "label": "Count", "type": "text", "default": "3", "value": "3" },
                    { "name": "dub", "label": "Dub", "type": "switch", "default": "false", "value": "false" },
                ],
                "requires_config": false,
                "configured": false,
            })
        );
        assert_eq!(serde_json::to_value(registry.plugin_config("plain").unwrap()).unwrap(), json!({ "fields": [], "requires_config": false, "configured": false }));
        assert!(registry.plugin_config("absent").is_err());

        // Saving values starts the plugin again with them.
        let changes = Arc::new(AtomicUsize::new(0));
        let counter = changes.clone();
        registry.set_notifier(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let values = HashMap::from([("mode".to_string(), json!("b")), ("dub".to_string(), json!(true)), ("stray".to_string(), json!("x"))]);
        registry.set_plugin_config("configured", &values).await.unwrap();
        assert_eq!(changes.load(Ordering::SeqCst), 1);
        let configured = info(&registry, "configured");
        assert_eq!(configured.servers, ["b-server", "b-pref", "4"]);
        assert!(configured.supports_dub);
        let form = registry.plugin_config("configured").unwrap();
        assert!(form.configured);
        assert_eq!(form.fields.iter().map(|f| f.value.as_str()).collect::<Vec<_>>(), ["b", "3", "true"]);
        // The provider the manager uses is the reloaded one.
        let provider = registry.providers().into_iter().find(|p| p.name() == "configured").unwrap();
        assert!(provider.supports_dub());

        registry.set_plugin_config("needy", &HashMap::new()).await.unwrap();
        let needy = info(&registry, "needy");
        assert_eq!((needy.status.as_str(), needy.servers.as_slice()), ("ready", &["somewhere".to_string()][..]));
        assert_eq!(registry.set_plugin_config("plain", &HashMap::new()).await.unwrap_err(), "This plugin has no settings.");
        assert!(registry.set_plugin_config("absent", &HashMap::new()).await.is_err());

        // The values survive a restart and a refresh of the repository.
        let reopened = open(&dir);
        reopened.load_all().await;
        assert_eq!(info(&reopened, "configured").servers, ["b-server", "b-pref", "4"]);
        reopened.refresh_repos().await;
        assert_eq!(info(&reopened, "configured").servers, ["b-server", "b-pref", "4"]);
        assert_eq!(reopened.plugin_config("configured").unwrap().fields[0].value, "b");

        // The plugin's update changes its configuration version: the saved values are not used
        // and the plugin waits until its settings are saved again.
        version.store(2, Ordering::SeqCst);
        reopened.refresh_repos().await;
        let outdated = info(&reopened, "configured");
        assert_eq!(outdated.status, "error");
        assert_eq!(
            outdated.error.as_deref(),
            Some("This plugin's settings changed with its update; review and save them again.")
        );
        let form = reopened.plugin_config("configured").unwrap();
        assert!(!form.configured);
        assert_eq!(form.fields[0].value, "a");
        reopened.set_plugin_config("configured", &HashMap::from([("mode".to_string(), json!("b"))])).await.unwrap();
        assert_eq!(info(&reopened, "configured").servers, ["b-server", "b-pref", "4"]);
        assert_eq!(info(&reopened, "configured").status, "ready");
    }
}

// ---------------------------------------------------------------------------------------
// Repositories on Google Drive
// ---------------------------------------------------------------------------------------

const FILE_ID: &str = "1AbC_dEf-0123456789xyz";

fn drive_plugin(servers: &str) -> String {
    format!("class Provider {{ getSettings() {{ return {{ episodeServers: [{}], supportsDub: false }}; }} }}", servers)
}

/// A stand-in for Google Drive (share pages and the download host) and, on another port, a
/// website whose links redirect to it.
struct DriveFixture {
    drive: Site,
    website: Site,
    /// What the shared file holds.
    file: Arc<Mutex<String>>,
    /// The page Drive sends instead of the file, when set.
    interstitial: Arc<Mutex<Option<String>>>,
}

impl DriveFixture {
    fn endpoints(&self) -> DriveEndpoints {
        DriveEndpoints {
            share_hosts: vec![self.drive.addr.to_string()],
            download_base: self.drive.base(),
        }
    }

    fn registry(&self, dir: &PathBuf) -> PluginRegistry {
        PluginRegistry::open(
            dir.clone(),
            RegistryOptions { drive: self.endpoints(), ..options(Arc::new(NoSession)) },
        )
    }

    fn set_file(&self, value: serde_json::Value) {
        *self.file.lock().unwrap() = value.to_string();
    }

    fn downloads(&self) -> usize {
        self.drive.count("GET /download?")
    }
}

fn drive_index(name: Option<&str>, version: &str, servers: &str) -> serde_json::Value {
    let plugins = json!([
        { "id": "drive-one", "name": "Drive One", "version": version, "type": "onlinestream-provider", "language": "javascript", "payload": drive_plugin(servers) },
        { "id": "drive-two", "name": "Drive Two", "version": "1.0.0", "type": "onlinestream-provider", "language": "javascript", "payload": drive_plugin("'two'") }
    ]);
    match name {
        Some(name) => json!({ "name": name, "plugins": plugins }),
        None => plugins,
    }
}

async fn drive_fixture() -> DriveFixture {
    let file = Arc::new(Mutex::new(drive_index(Some("Anix extensions"), "1.0.0", "'a'").to_string()));
    let interstitial: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let (contents, page) = (file.clone(), interstitial.clone());
    let drive = Site::start(move |request: &Req| match request.route() {
        // The file itself, only for the request the app is meant to make.
        "/download" => {
            if request.query("id") != FILE_ID {
                return Resp::status(404);
            }
            if request.query("export") != "download" || request.query("confirm") != "t" {
                return Resp::html("<html><body>Google Drive can't scan this file for viruses. Download anyway?</body></html>");
            }
            match page.lock().unwrap().clone() {
                Some(html) => Resp::html(&html),
                // Drive serves every file as a download, whatever it holds.
                None => Resp::new(200, "application/octet-stream", contents.lock().unwrap().clone()),
            }
        }
        // Drive's pages about the file are web pages, never the file.
        route if route.starts_with("/file/d/") || route == "/open" || route == "/uc" => {
            Resp::html("<!DOCTYPE html><html><head><title>index.json - Google Drive</title></head><body>viewer</body></html>")
        }
        _ => Resp::status(404),
    })
    .await;

    let drive_base = drive.base();
    let website = Site::start(move |request: &Req| {
        let to = |path: &str| Resp::status(302).with_header("Location", &format!("{}{}", drive_base, path));
        match request.route() {
            "/plugins" => to(&format!("/file/d/{}/view?usp=sharing", FILE_ID)),
            "/plugins-direct" => to(&format!("/download?id={}&export=download&confirm=t", FILE_ID)),
            "/hop" => Resp::status(301).with_header("Location", "/plugins"),
            "/plugins-missing" => to("/file/d/nosuchfile000/view"),
            "/list.json" => Resp::json(drive_index(Some("  Hosted \n here  "), "1.0.0", "'a'")),
            _ => Resp::status(404),
        }
    })
    .await;

    DriveFixture { drive, website, file, interstitial }
}

#[test]
fn drive_share_links_resolve_to_a_direct_download() {
    let direct = "https://drive.usercontent.google.com/download?id=1AbC_dEf-0123456789xyz&export=download&confirm=t";
    for link in [
        "https://drive.google.com/file/d/1AbC_dEf-0123456789xyz/view?usp=sharing",
        "https://drive.google.com/file/d/1AbC_dEf-0123456789xyz/view",
        "https://drive.google.com/file/d/1AbC_dEf-0123456789xyz",
        "drive.google.com/file/d/1AbC_dEf-0123456789xyz/edit",
        "https://drive.google.com/file/u/0/d/1AbC_dEf-0123456789xyz/view".replace("/file/u/0/d/", "/u/0/file/d/").as_str(),
        "https://drive.google.com/open?id=1AbC_dEf-0123456789xyz",
        "https://drive.google.com/open?usp=drive_link&id=1AbC_dEf-0123456789xyz",
        "https://drive.google.com/uc?id=1AbC_dEf-0123456789xyz&export=download",
        "https://drive.google.com/uc?export=download&id=1AbC_dEf-0123456789xyz",
        "https://DRIVE.GOOGLE.COM/uc?id=1AbC_dEf-0123456789xyz",
        "https://docs.google.com/uc?id=1AbC_dEf-0123456789xyz",
        "https://drive.usercontent.google.com/download?id=1AbC_dEf-0123456789xyz",
        "https://drive.usercontent.google.com/download?id=1AbC_dEf-0123456789xyz&export=download&authuser=0",
        "https://drive.usercontent.google.com/u/0/uc?id=1AbC_dEf-0123456789xyz&export=download",
    ] {
        assert_eq!(index_candidates(link).unwrap(), vec![direct.to_string()], "{}", link);
    }

    // Other sites, Drive addresses that name no file, and ids that are not ids are left as they are.
    for link in [
        "https://example.org/file/d/1AbC_dEf-0123456789xyz/view",
        "https://example.org/uc?id=1AbC_dEf-0123456789xyz",
        "https://drive.google.com/drive/folders/1AbC_dEf-0123456789xyz",
        "https://drive.google.com/uc?id=../../etc",
        "https://drive.google.com/uc?id=",
        "https://notdrive.google.com.example.org/uc?id=1AbC_dEf-0123456789xyz",
    ] {
        let url = if link.contains("://") { link.to_string() } else { format!("https://{}", link) };
        assert_eq!(index_candidates(link).unwrap(), vec![url], "{}", link);
    }
    assert!(DriveEndpoints::default().file_id(&"https://drive.google.com/file/d/abc/view".parse().unwrap()).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_drive_share_link_is_downloaded_directly_and_kept_as_entered() {
    let fx = drive_fixture().await;
    let dir = scratch_dir("drive-share");
    let registry = fx.registry(&dir);

    let link = fx.drive.url(&format!("/file/d/{}/view?usp=sharing", FILE_ID));
    let repo = registry.add_repo(&link).await.unwrap();
    assert_eq!(repo.url, link, "the link the user entered is what is stored");
    assert_eq!(repo.name, "Anix extensions");
    assert_eq!((repo.plugin_count, repo.error.clone()), (2, None));
    // The share page itself was never asked for.
    assert_eq!(fx.drive.count("/file/d/"), 0, "{:?}", fx.drive.hits());
    assert_eq!(fx.drive.hits(), [format!("GET /download?id={}&export=download&confirm=t", FILE_ID)]);
    assert_eq!(manager(&registry).provider_names(), ["drive-one", "drive-two"]);
    assert!(dir.join("payloads/drive-one.js").is_file());

    // The other spellings of the same file are the same repository.
    for other in [format!("/open?id={}", FILE_ID), format!("/uc?id={}&export=download", FILE_ID), format!("/download?id={}", FILE_ID)] {
        let again = registry.add_repo(&fx.drive.url(&other)).await.unwrap_err();
        assert!(again.contains("already been added"), "{}", again);
    }

    // It is stored and refreshed under the entered link after a restart.
    drop(registry);
    let registry = fx.registry(&dir);
    assert_eq!(registry.list_repos()[0].url, link);
    let before = fx.downloads();
    let refreshed = registry.refresh_repos().await;
    assert_eq!(fx.downloads(), before + 1);
    assert_eq!((refreshed[0].plugin_count, refreshed[0].error.clone()), (2, None));

    registry.remove_repo(&fx.drive.url(&format!("/open?id={}", FILE_ID))).await.unwrap();
    assert!(registry.list_repos().is_empty() && registry.list_plugins().is_empty());

    // Each form of link works on its own, and an index without a name is called after Drive.
    fx.set_file(drive_index(None, "1.0.0", "'a'"));
    for form in [format!("/open?id={}", FILE_ID), format!("/uc?id={}&export=download", FILE_ID), format!("/download?id={}&authuser=0", FILE_ID)] {
        let link = fx.drive.url(&form);
        let repo = registry.add_repo(&link).await.unwrap_or_else(|e| panic!("{}: {}", form, e));
        assert_eq!((repo.url.as_str(), repo.name.as_str(), repo.plugin_count), (link.as_str(), "Google Drive", 2));
        registry.remove_repo(&link).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_website_link_that_redirects_to_drive_is_followed_to_the_file() {
    let fx = drive_fixture().await;
    let dir = scratch_dir("drive-redirect");
    let registry = fx.registry(&dir);

    // website -> website -> Drive's share page (a web page) -> the file, asked for directly.
    let link = fx.website.url("/hop");
    let repo = registry.add_repo(&link).await.unwrap();
    assert_eq!(repo.url, link);
    assert_eq!(repo.name, "Anix extensions");
    assert_eq!(repo.plugin_count, 2);
    assert_eq!(fx.website.hits(), ["GET /hop", "GET /plugins"]);
    assert_eq!(
        fx.drive.hits(),
        [
            format!("GET /file/d/{}/view?usp=sharing", FILE_ID),
            format!("GET /download?id={}&export=download&confirm=t", FILE_ID),
        ]
    );
    assert_eq!(registry.list_plugins().iter().map(|p| p.repo_url.clone()).collect::<Vec<_>>(), [link.clone(), link.clone()]);

    // A website that redirects straight to the download needs no second request.
    let dir2 = scratch_dir("drive-redirect-direct");
    let second = fx.registry(&dir2);
    let before = fx.drive.hits().len();
    let direct = second.add_repo(&fx.website.url("/plugins-direct")).await.unwrap();
    assert_eq!(direct.plugin_count, 2);
    assert_eq!(fx.drive.hits().len(), before + 1);

    // Without a name in the index the repository is called after the link the user entered.
    fx.set_file(drive_index(None, "1.0.0", "'a'"));
    let dir3 = scratch_dir("drive-redirect-unnamed");
    let unnamed = fx.registry(&dir3).add_repo(&fx.website.url("/plugins")).await.unwrap();
    assert_eq!(unnamed.name, format!("{}/plugins", fx.website.addr.ip()));

    // A name that arrives later is taken up on refresh.
    fx.set_file(drive_index(Some("Renamed list"), "1.0.0", "'a'"));
    assert_eq!(registry.refresh_repos().await[0].name, "Renamed list");

    // A redirect to a Drive file that does not exist is reported as Drive's failure.
    let missing = registry.add_repo(&fx.website.url("/plugins-missing")).await.unwrap_err();
    assert!(missing.contains("Google Drive did not return the file") && missing.contains("HTTP 404"), "{}", missing);

    // An index with a name works from any host, and the name is tidied.
    let dir4 = scratch_dir("named-index");
    let hosted = fx.registry(&dir4).add_repo(&fx.website.url("/list.json")).await.unwrap();
    assert_eq!((hosted.name.as_str(), hosted.plugin_count), ("Hosted here", 2));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_from_drive_instead_of_the_file_is_reported_and_plugins_stay() {
    let fx = drive_fixture().await;
    let dir = scratch_dir("drive-interstitial");
    let registry = fx.registry(&dir);
    let link = fx.drive.url(&format!("/file/d/{}/view", FILE_ID));

    let pages = [
        ("<html><body><p>Sorry, you can't view or download this file at this time.</p><p>Too many users have viewed or downloaded this file recently.</p></body></html>", "too many people"),
        ("<html><body>Google Drive can't scan this file for viruses. <a>Download anyway</a></body></html>", "viruses"),
        ("<html><head><title>Sign in - Google Accounts</title></head><body><form action=\"/ServiceLogin\"></form></body></html>", "sign in"),
        ("<html><body>You need access. Request access, or switch to an account with access.</body></html>", "Anyone with the link"),
        ("<!DOCTYPE html><html><body>Something else</body></html>", "web page instead of the plugin list"),
        ("this is not json", "not JSON"),
    ];

    // Adding: nothing is stored.
    for (page, expected) in pages {
        *fx.interstitial.lock().unwrap() = Some(page.to_string());
        let refused = registry.add_repo(&link).await.unwrap_err();
        assert!(refused.contains("Google Drive did not return the file: "), "{}", refused);
        assert!(refused.contains(expected), "{} / {}", expected, refused);
    }
    assert!(registry.list_repos().is_empty() && registry.list_plugins().is_empty());

    // Refreshing: the reason is on the repository and the installed plugins keep working.
    *fx.interstitial.lock().unwrap() = None;
    registry.add_repo(&link).await.unwrap();
    let installed = std::fs::read_to_string(dir.join("payloads/drive-one.js")).unwrap();
    for (page, expected) in pages {
        *fx.interstitial.lock().unwrap() = Some(page.to_string());
        let refreshed = registry.refresh_repos().await;
        let error = refreshed[0].error.clone().unwrap();
        assert!(error.starts_with("Google Drive did not return the file: ") && error.contains(expected), "{}", error);
        assert_eq!(refreshed[0].plugin_count, 2);
    }
    assert_eq!(manager(&registry).provider_names(), ["drive-one", "drive-two"]);
    assert_eq!(std::fs::read_to_string(dir.join("payloads/drive-one.js")).unwrap(), installed);
    assert!(registry.list_plugins().iter().all(|p| p.status == "ready"));

    // The same through the redirecting website.
    let dir2 = scratch_dir("drive-interstitial-site");
    let via_site = fx.registry(&dir2).add_repo(&fx.website.url("/plugins")).await.unwrap_err();
    assert!(via_site.contains("Google Drive did not return the file"), "{}", via_site);

    // Once Drive serves the file again the error clears.
    *fx.interstitial.lock().unwrap() = None;
    assert_eq!(registry.refresh_repos().await[0].error, None);

    // A page from a site that is not Drive keeps its own wording.
    let other = registry.add_repo(&fx.drive.url("/file/d/")).await.unwrap_err();
    assert!(other.contains("Google Drive did not return the file"), "{}", other);
    let plain = PluginRegistry::open(scratch_dir("drive-none"), options(Arc::new(NoSession)));
    let not_drive = plain.add_repo(&link).await.unwrap_err();
    assert!(!not_drive.contains("Google Drive") && not_drive.contains("not JSON"), "{}", not_drive);
}

#[tokio::test(flavor = "multi_thread")]
async fn inline_code_that_changes_under_the_same_version_is_installed() {
    let fx = drive_fixture().await;
    let dir = scratch_dir("drive-same-version");
    let registry = fx.registry(&dir);
    let link = fx.drive.url(&format!("/uc?id={}", FILE_ID));
    registry.add_repo(&link).await.unwrap();
    let server_list = |registry: &PluginRegistry, id: &str| registry.list_plugins().into_iter().find(|p| p.id == id).unwrap().servers;
    assert_eq!(server_list(&registry, "drive-one"), ["a"]);
    let two_before = std::fs::metadata(dir.join("payloads/drive-two.js")).unwrap().modified().unwrap();
    let provider_before = Arc::as_ptr(&registry.providers()[1]) as *const () as usize;

    // Nothing changed: nothing is rewritten.
    registry.refresh_repos().await;
    assert_eq!(server_list(&registry, "drive-one"), ["a"]);

    // The code of one plugin changes; every version number stays the same.
    fx.set_file(drive_index(Some("Anix extensions"), "1.0.0", "'a', 'b'"));
    let refreshed = registry.refresh_repos().await;
    assert_eq!(refreshed[0].error, None);
    let one = registry.list_plugins().into_iter().find(|p| p.id == "drive-one").unwrap();
    assert_eq!((one.version.as_str(), one.status.as_str()), ("1.0.0", "ready"));
    assert_eq!(one.servers, ["a", "b"], "the new code is what runs");
    assert!(std::fs::read_to_string(dir.join("payloads/drive-one.js")).unwrap().contains("'a', 'b'"));
    // The plugin whose code did not change was left alone.
    assert_eq!(std::fs::metadata(dir.join("payloads/drive-two.js")).unwrap().modified().unwrap(), two_before);
    assert_eq!(Arc::as_ptr(&registry.providers()[1]) as *const () as usize, provider_before);

    // A plugin dropped from the list goes away; a new one arrives.
    fx.set_file(json!({ "plugins": [
        { "id": "drive-three", "name": "Three", "version": "0.1.0", "type": "onlinestream-provider", "payload": drive_plugin("'c'") }
    ] }));
    let refreshed = registry.refresh_repos().await;
    assert_eq!(refreshed[0].name, "Anix extensions", "a list that stops naming itself keeps its name");
    assert_eq!(manager(&registry).provider_names(), ["drive-three"]);
    assert!(!dir.join("payloads/drive-one.js").exists());
}
