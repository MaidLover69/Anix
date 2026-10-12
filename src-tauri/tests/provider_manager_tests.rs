//! Provider manager behaviour, exercised with in-process providers and a loopback HTTP server
//! that plays the part of a stream host. No outside network is involved.

use anix_lib::error::AppError;
use anix_lib::models::{AnimeQuery, Category, EpisodeItem, ProviderData, SourcesResult, StreamItem};
use anix_lib::providers::manager::{
    first_playlist_uri, resolve_playlist_uri, summarize_failures, verify_stream_segment,
    verify_streams, ProviderManager,
};
use anix_lib::providers::AnimeProvider;
use anix_lib::web_session::{SessionUse, SiteSpec};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------- loopback stream host

#[derive(Clone)]
struct Route {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: String,
}

fn ok(body: &str) -> Route {
    Route { status: 200, headers: Vec::new(), body: body.to_string() }
}

fn status(code: u16) -> Route {
    Route { status: code, headers: Vec::new(), body: String::new() }
}

fn redirect(location: &str) -> Route {
    Route { status: 302, headers: vec![("Location", location.to_string())], body: String::new() }
}

/// One request the host received: the request target and its header lines, lower-cased.
type Seen = Arc<Mutex<Vec<(String, Vec<String>)>>>;

struct StreamHost {
    base: String,
    seen: Seen,
}

impl StreamHost {
    /// Serves `routes` (keyed by request target, query included) on 127.0.0.1.
    async fn start(routes: Vec<(&'static str, Route)>) -> StreamHost {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: Arc<HashMap<String, Route>> =
            Arc::new(routes.into_iter().map(|(path, route)| (path.to_string(), route)).collect());
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));

        let seen_by_server = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let (routes, seen) = (routes.clone(), seen_by_server.clone());
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0u8; 2048];
                    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buffer).to_string();
                    let mut lines = head.split("\r\n");
                    let target = lines.next().and_then(|l| l.split(' ').nth(1)).unwrap_or("/").to_string();
                    let headers: Vec<String> = lines
                        .take_while(|l| !l.is_empty())
                        .map(|l| l.to_ascii_lowercase())
                        .collect();
                    seen.lock().unwrap().push((target.clone(), headers));

                    let route = routes.get(&target).cloned().unwrap_or_else(|| status(404));
                    let reason = if route.status == 200 { "OK" } else { "Status" };
                    let mut response = format!(
                        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                        route.status,
                        reason,
                        route.body.len()
                    );
                    for (name, value) in &route.headers {
                        response.push_str(&format!("{}: {}\r\n", name, value));
                    }
                    response.push_str("\r\n");
                    response.push_str(&route.body);
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        StreamHost { base, seen }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().map(|(target, _)| target.clone()).collect()
    }

    fn headers_for(&self, target: &str) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .find(|(seen, _)| seen == target)
            .map(|(_, headers)| headers.clone())
            .unwrap_or_default()
    }
}

const MEDIA_PLAYLIST: &str = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts\n#EXTINF:6.0,\nseg1.ts\n#EXT-X-ENDLIST\n";

/// A host with one working HLS stream at `/hls/master.m3u8`.
async fn host_with_good_stream() -> StreamHost {
    StreamHost::start(vec![
        ("/hls/master.m3u8", ok("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000\n720/index.m3u8\n")),
        ("/hls/720/index.m3u8", ok(MEDIA_PLAYLIST)),
        ("/hls/720/seg0.ts", ok("segment-bytes")),
    ])
    .await
}

fn hls(url: String) -> StreamItem {
    StreamItem::new(url, "hls".to_string(), Some("auto".to_string()), Some("sub".to_string()), None, true)
}

// ---------------------------------------------------------------- in-process providers

#[derive(Clone)]
enum Listing {
    Episodes(Vec<f64>),
    Fail(AppError),
}

struct MockProvider {
    name: &'static str,
    dub: bool,
    by_number: bool,
    listing: Listing,
    sources: Result<Vec<StreamItem>, AppError>,
    delay: Duration,
    list_calls: AtomicUsize,
    source_calls: AtomicUsize,
}

impl MockProvider {
    fn new(name: &'static str, sources: Result<Vec<StreamItem>, AppError>) -> MockProvider {
        MockProvider {
            name,
            dub: false,
            by_number: false,
            listing: Listing::Episodes(vec![1.0, 2.0, 3.0]),
            sources,
            delay: Duration::ZERO,
            list_calls: AtomicUsize::new(0),
            source_calls: AtomicUsize::new(0),
        }
    }

    fn list_calls(&self) -> usize {
        self.list_calls.load(Ordering::SeqCst)
    }

    fn source_calls(&self) -> usize {
        self.source_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AnimeProvider for MockProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn supports_dub(&self) -> bool {
        self.dub
    }

    fn episode_by_number(&self, query: &AnimeQuery, number: f64) -> Option<EpisodeItem> {
        self.by_number.then(|| {
            EpisodeItem::new(format!("{}-{}", query.anilist_id, number), number, None, None, false)
        })
    }

    async fn get_episodes(&self, _query: &AnimeQuery) -> Result<ProviderData, AppError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        match &self.listing {
            Listing::Fail(err) => Err(err.clone()),
            Listing::Episodes(numbers) => {
                let list: Vec<EpisodeItem> = numbers
                    .iter()
                    .map(|n| EpisodeItem::new(format!("{}|{}", self.name, n), *n, None, None, false))
                    .collect();
                Ok(ProviderData {
                    name: self.name.to_string(),
                    dub: if self.dub { list.clone() } else { Vec::new() },
                    sub: list,
                })
            }
        }
    }

    async fn get_sources(&self, _episode: &EpisodeItem, _category: Category) -> Result<SourcesResult, AppError> {
        self.source_calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.sources.clone().map(SourcesResult::new)
    }
}

fn manager_of(providers: &[Arc<MockProvider>]) -> ProviderManager {
    ProviderManager::with_providers(
        providers.iter().map(|p| p.clone() as Arc<dyn AnimeProvider>).collect(),
    )
}

async fn sources(
    manager: &ProviderManager,
    episode: f64,
    category: &str,
    preferred: Option<&str>,
) -> Result<SourcesResult, AppError> {
    manager
        .get_sources_for_episode(21, Some(21), episode, category, Some("One Piece".to_string()), preferred)
        .await
}

fn failure_message(result: Result<SourcesResult, AppError>) -> String {
    match result {
        Err(AppError::SourcesUnavailable(message)) => message,
        other => panic!("expected a per-provider failure message, got {:?}", other),
    }
}

// ---------------------------------------------------------------- playlist URL handling

#[test]
fn playlist_entries_follow_url_joining_rules() {
    let master = "https://cdn.example/a/b/master.m3u8?token=x/y";
    assert_eq!(
        resolve_playlist_uri(master, "720/index.m3u8").as_deref(),
        Some("https://cdn.example/a/b/720/index.m3u8"),
        "a '/' in the playlist's query must not change the base path"
    );
    assert_eq!(
        resolve_playlist_uri(master, "/m3u8-proxy?url=abc").as_deref(),
        Some("https://cdn.example/m3u8-proxy?url=abc")
    );
    assert_eq!(
        resolve_playlist_uri(master, "//other.example/v/seg.ts").as_deref(),
        Some("https://other.example/v/seg.ts")
    );
    assert_eq!(
        resolve_playlist_uri(master, "http://abs.example/seg.ts").as_deref(),
        Some("http://abs.example/seg.ts")
    );
    assert_eq!(
        resolve_playlist_uri(master, "../c/seg.ts").as_deref(),
        Some("https://cdn.example/a/c/seg.ts")
    );
    assert_eq!(resolve_playlist_uri("not a url", "seg.ts"), None);

    assert_eq!(first_playlist_uri("#EXTM3U\n\n#EXTINF:6,\n  seg0.ts  \nseg1.ts\n"), Some("seg0.ts"));
    assert_eq!(first_playlist_uri("#EXTM3U\n#EXT-X-ENDLIST\n"), None);
}

#[test]
fn failures_are_grouped_by_reason() {
    let failures = vec![
        ("bonk".to_string(), "timed out".to_string()),
        ("AniBD".to_string(), "HTTP 403 from example.org".to_string()),
        ("kiwi".to_string(), "timed out".to_string()),
    ];
    assert_eq!(
        summarize_failures(&failures),
        "bonk, kiwi: timed out; AniBD: HTTP 403 from example.org"
    );
    assert_eq!(summarize_failures(&[]), "");
}

// ---------------------------------------------------------------- stream verification

#[tokio::test]
async fn hls_with_root_relative_and_query_urls_verifies() {
    let host = StreamHost::start(vec![
        ("/a/b/master.m3u8?token=x/y", ok("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n/proxy/variant.m3u8?id=7\n")),
        ("/proxy/variant.m3u8?id=7", ok(MEDIA_PLAYLIST)),
        ("/proxy/seg0.ts", ok("bytes")),
    ])
    .await;

    assert!(verify_stream_segment(&hls(host.url("/a/b/master.m3u8?token=x/y"))).await);
    assert_eq!(
        host.requests(),
        vec!["/a/b/master.m3u8?token=x/y", "/proxy/variant.m3u8?id=7", "/proxy/seg0.ts"]
    );
}

#[tokio::test]
async fn media_playlist_without_master_verifies_and_dead_segments_fail() {
    let host = StreamHost::start(vec![
        ("/live/index.m3u8", ok(MEDIA_PLAYLIST)),
        ("/live/seg0.ts", ok("bytes")),
        ("/dead/index.m3u8", ok(MEDIA_PLAYLIST)),
        ("/embed/page", ok("<html>an embed page, not a playlist</html>")),
    ])
    .await;

    assert!(verify_stream_segment(&hls(host.url("/live/index.m3u8"))).await);
    assert!(!verify_stream_segment(&hls(host.url("/dead/index.m3u8"))).await, "segment is a 404");
    assert!(!verify_stream_segment(&hls(host.url("/embed/page"))).await, "HTML is not a playlist");
    assert!(!verify_stream_segment(&hls(host.url("/missing.m3u8"))).await);
    assert!(!verify_stream_segment(&hls("ftp://example/x.m3u8".to_string())).await);
}

#[tokio::test]
async fn mp4_check_follows_relative_redirects_with_one_referer() {
    let host = StreamHost::start(vec![
        ("/play/1/video.mp4", redirect("/files/real.mp4")),
        ("/files/real.mp4", redirect("final.mp4")),
        ("/files/final.mp4", ok("bytes")),
    ])
    .await;

    let mut stream = StreamItem::new(
        host.url("/play/1/video.mp4"),
        "mp4".to_string(),
        None,
        None,
        Some("https://site.example/watch".to_string()),
        true,
    );
    let mut custom = HashMap::new();
    custom.insert("Referer".to_string(), "https://site.example/watch".to_string());
    stream.headers = Some(custom);
    stream.origin = Some("https://origin.example".to_string());

    assert!(verify_stream_segment(&stream).await);
    assert_eq!(host.requests(), vec!["/play/1/video.mp4", "/files/real.mp4", "/files/final.mp4"]);

    let headers = host.headers_for("/files/final.mp4");
    let referers: Vec<&String> = headers.iter().filter(|h| h.starts_with("referer:")).collect();
    assert_eq!(referers, vec!["referer: https://site.example/watch"], "exactly one Referer per request");
    assert!(headers.contains(&"origin: https://origin.example".to_string()));
    assert!(headers.iter().any(|h| h.starts_with("range: bytes=0-")));
}

#[tokio::test]
async fn verification_puts_the_working_stream_first_and_keeps_the_rest() {
    let host = host_with_good_stream().await;
    let broken = hls(host.url("/gone/master.m3u8"));
    let working = hls(host.url("/hls/master.m3u8"));
    let other = hls(host.url("/also-gone.m3u8"));

    let ordered = verify_streams(vec![broken.clone(), working.clone(), other.clone()]).await.unwrap();
    assert_eq!(ordered, vec![working, broken.clone(), other.clone()]);

    let reason = verify_streams(vec![broken, other]).await.unwrap_err();
    assert!(reason.starts_with("HTTP 404"), "unexpected reason: {}", reason);
    assert_eq!(verify_streams(Vec::new()).await.unwrap_err(), "no streams");
}

fn file(url: String) -> StreamItem {
    StreamItem::new(url, "mp4".to_string(), None, None, None, true)
}

const CHECK_PAGE: &str = "<!DOCTYPE html><html><head><title>Just a moment...</title></head><body></body></html>";

#[tokio::test]
async fn a_stream_host_that_answers_with_its_anti_bot_page_is_named_as_such() {
    let check = Route { status: 403, headers: vec![("Content-Type", "text/html".to_string())], body: CHECK_PAGE.to_string() };
    let mitigated = Route {
        status: 200,
        headers: vec![("cf-mitigated", "challenge".to_string()), ("Content-Type", "text/html".to_string())],
        body: "<html></html>".to_string(),
    };
    let host = StreamHost::start(vec![
        ("/guarded/video.mp4", check.clone()),
        ("/guarded/master.m3u8", check.clone()),
        ("/half/index.m3u8", ok(MEDIA_PLAYLIST)),
        ("/half/seg0.ts", check),
        ("/soft/video.mp4", mitigated),
    ])
    .await;

    for stream in [
        file(host.url("/guarded/video.mp4")),
        hls(host.url("/guarded/master.m3u8")),
        hls(host.url("/half/index.m3u8")),
        file(host.url("/soft/video.mp4")),
    ] {
        let reason = verify_streams(vec![stream.clone()]).await.unwrap_err();
        assert_eq!(reason, "127.0.0.1 answered with its anti-bot check", "{}", stream.url);
    }
    // An ordinary refusal keeps its status.
    let reason = verify_streams(vec![file(host.url("/nowhere.mp4"))]).await.unwrap_err();
    assert_eq!(reason, "HTTP 404 from 127.0.0.1");
}

#[tokio::test]
async fn a_web_page_is_not_accepted_as_a_video() {
    let page = |content_type: &str| Route {
        status: 200,
        headers: vec![("Content-Type", content_type.to_string())],
        body: "\n <!DOCTYPE html><html><body>Download will start soon</body></html>".to_string(),
    };
    let disguised_segment = Route {
        status: 200,
        headers: vec![("Content-Type", "text/html".to_string())],
        body: "G@\u{11}\u{10} transport stream bytes".to_string(),
    };
    let host = StreamHost::start(vec![
        ("/page/video.mp4", page("text/html; charset=utf-8")),
        ("/mislabelled/video.mp4", page("video/mp4")),
        ("/real/video.mp4", Route { status: 206, headers: vec![("Content-Type", "video/mp4".to_string())], body: "\0\0\0 ftypisom".to_string() }),
        ("/page/index.m3u8", ok(MEDIA_PLAYLIST)),
        ("/page/seg0.ts", page("text/html")),
        ("/odd/index.m3u8", ok(MEDIA_PLAYLIST)),
        ("/odd/seg0.ts", disguised_segment),
        ("/embed.m3u8", page("text/html")),
    ])
    .await;

    for path in ["/page/video.mp4", "/mislabelled/video.mp4"] {
        let reason = verify_streams(vec![file(host.url(path))]).await.unwrap_err();
        assert_eq!(reason, "127.0.0.1 answered with a web page instead of a video", "{}", path);
    }
    let reason = verify_streams(vec![hls(host.url("/page/index.m3u8"))]).await.unwrap_err();
    assert_eq!(reason, "127.0.0.1 answered with a web page instead of a video");
    let reason = verify_streams(vec![hls(host.url("/embed.m3u8"))]).await.unwrap_err();
    assert_eq!(reason, "127.0.0.1 answered with a web page instead of a playlist");

    assert!(verify_stream_segment(&file(host.url("/real/video.mp4"))).await);
    // Media sent under a text type is still media.
    assert!(verify_stream_segment(&hls(host.url("/odd/index.m3u8"))).await);
}

// ---------------------------------------------------------------- racing and reporting

#[tokio::test]
async fn first_verified_provider_wins_regardless_of_position() {
    let host = host_with_good_stream().await;
    let good = vec![hls(host.url("/hls/master.m3u8"))];

    let mut slow = MockProvider::new("slow-first", Ok(good.clone()));
    slow.delay = Duration::from_secs(6);
    let slow = Arc::new(slow);
    let failing = Arc::new(MockProvider::new("failing", Err(AppError::Network("HTTP 500 from x".into()))));
    let fast = Arc::new(MockProvider::new("fast-last", Ok(good)));
    let manager = manager_of(&[slow.clone(), failing.clone(), fast.clone()]);

    let started = Instant::now();
    let result = sources(&manager, 2.0, "sub", None).await.expect("a source");
    assert!(started.elapsed() < Duration::from_secs(4), "the slow provider held the result back");
    assert_eq!(result.provider.as_deref(), Some("fast-last"));
    assert_eq!(result.streams.len(), 1);

    // "auto" and an empty preference mean the same as no preference.
    assert_eq!(sources(&manager, 2.0, "sub", Some("auto")).await.unwrap().provider.as_deref(), Some("fast-last"));
    assert_eq!(sources(&manager, 2.0, "sub", Some("")).await.unwrap().provider.as_deref(), Some("fast-last"));
}

#[tokio::test]
async fn preferred_provider_is_the_only_one_tried() {
    let host = host_with_good_stream().await;
    let good = vec![hls(host.url("/hls/master.m3u8"))];
    let first = Arc::new(MockProvider::new("first", Ok(good.clone())));
    let second = Arc::new(MockProvider::new("second", Ok(good)));
    let manager = manager_of(&[first.clone(), second.clone()]);

    let result = sources(&manager, 1.0, "sub", Some("SECOND")).await.expect("a source");
    assert_eq!(result.provider.as_deref(), Some("second"));
    assert_eq!(first.list_calls() + first.source_calls(), 0, "the other provider must stay untouched");

    let message = failure_message(sources(&manager, 1.0, "sub", Some("nope")).await);
    assert!(message.contains("Unknown provider 'nope'"), "{}", message);
}

#[tokio::test]
async fn every_failure_is_reported_with_its_reason() {
    let host = host_with_good_stream().await;
    let unplayable = Arc::new(MockProvider::new("unplayable", Ok(vec![hls(host.url("/gone.m3u8"))])));
    let blocked = Arc::new(MockProvider::new("blocked", Err(AppError::CaptchaRequired)));
    let waiting = Arc::new(MockProvider::new(
        "waiting",
        Err(AppError::VerificationRequired {
            site: "site".into(),
            origin: "https://www.site.example".into(),
        }),
    ));
    let erroring = Arc::new(MockProvider::new("erroring", Err(AppError::Network("HTTP 403 from cdn.example".into()))));
    let erroring_too = Arc::new(MockProvider::new("erroring-too", Err(AppError::Network("HTTP 403 from cdn.example".into()))));
    let mut missing = MockProvider::new("missing", Ok(Vec::new()));
    missing.listing = Listing::Episodes(Vec::new());
    let missing = Arc::new(missing);
    let manager = manager_of(&[unplayable, blocked, waiting, erroring, erroring_too, missing]);

    let message = failure_message(sources(&manager, 2.0, "sub", None).await);
    assert!(message.starts_with("No source found. "), "{}", message);
    assert!(message.contains("unplayable: stream check failed: HTTP 404"), "{}", message);
    assert!(message.contains("blocked: blocked by anti-bot check"), "{}", message);
    assert!(message.contains("waiting: needs verification"), "{}", message);
    assert!(message.contains("erroring, erroring-too: HTTP 403 from cdn.example"), "{}", message);
    assert!(message.contains("missing: anime not found"), "{}", message);

    let message = failure_message(sources(&manager, 99.0, "sub", Some("erroring")).await);
    assert_eq!(message, "No source found. erroring: episode 99 not listed");
}

#[tokio::test]
async fn dub_requests_only_reach_providers_with_a_dub() {
    let host = host_with_good_stream().await;
    let good = vec![hls(host.url("/hls/master.m3u8"))];
    let sub_only = Arc::new(MockProvider::new("sub-only", Ok(good.clone())));
    let mut dubbed = MockProvider::new("dubbed", Ok(good));
    dubbed.dub = true;
    let dubbed = Arc::new(dubbed);

    let manager = manager_of(&[sub_only.clone(), dubbed.clone()]);
    let result = sources(&manager, 1.0, "dub", None).await.expect("a dub source");
    assert_eq!(result.provider.as_deref(), Some("dubbed"));
    assert_eq!(sub_only.list_calls() + sub_only.source_calls(), 0);

    let only_sub = manager_of(&[sub_only.clone()]);
    let message = failure_message(sources(&only_sub, 1.0, "dub", None).await);
    assert_eq!(message, "No source found. sub-only: no dub");
}

#[tokio::test]
async fn episode_lists_are_cached_per_provider_and_anime() {
    let host = host_with_good_stream().await;
    let provider = Arc::new(MockProvider::new("cached", Ok(vec![hls(host.url("/hls/master.m3u8"))])));
    let manager = manager_of(&[provider.clone()]);

    let listed = manager.get_all_episodes(21, Some(21), Some("One Piece".to_string())).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].sub.len(), 3);

    sources(&manager, 1.0, "sub", None).await.expect("first play");
    sources(&manager, 2.0, "sub", None).await.expect("second play");
    assert_eq!(provider.list_calls(), 1, "a play request must reuse the cached episode list");
    assert_eq!(provider.source_calls(), 2);

    // Another anime is a different cache entry.
    manager.get_all_episodes(22, None, Some("Other".to_string())).await.unwrap();
    assert_eq!(provider.list_calls(), 2);
}

#[tokio::test]
async fn number_addressed_providers_skip_the_episode_list() {
    let host = host_with_good_stream().await;
    let mut by_number = MockProvider::new("by-number", Ok(vec![hls(host.url("/hls/master.m3u8"))]));
    by_number.by_number = true;
    let by_number = Arc::new(by_number);
    let manager = manager_of(&[by_number.clone()]);

    assert!(manager.is_number_addressed("by-number"));
    let result = sources(&manager, 1234.0, "sub", None).await.expect("a source");
    assert_eq!(result.provider.as_deref(), Some("by-number"));

    let listed = manager.get_all_episodes(21, None, None).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].sub.is_empty() && listed[0].dub.is_empty());
    assert_eq!(by_number.list_calls(), 0);
}

#[tokio::test]
async fn an_unreachable_site_is_deprioritised_but_never_locked_out() {
    let host = host_with_good_stream().await;
    let mut down = MockProvider::new("down", Ok(Vec::new()));
    down.listing = Listing::Fail(AppError::Unreachable("down.example: connection refused".into()));
    let down = Arc::new(down);
    let manager = manager_of(&[down.clone()]);

    let first = failure_message(sources(&manager, 1.0, "sub", None).await);
    assert!(first.contains("down: connection failed (down.example: connection refused)"), "{}", first);
    assert_eq!(down.list_calls(), 1);

    // Still cooling down, but it is the only provider, so the retry must try it again.
    let second = failure_message(sources(&manager, 1.0, "sub", None).await);
    assert!(second.contains("down: connection failed"), "{}", second);
    assert_eq!(down.list_calls(), 2);

    // The episode list leaves a cooling-down provider out instead of waiting on it.
    let up = Arc::new(MockProvider::new("up", Ok(vec![hls(host.url("/hls/master.m3u8"))])));
    let mixed = manager_of(&[down.clone(), up.clone()]);
    failure_message(sources(&mixed, 99.0, "sub", None).await);
    let calls_before = down.list_calls();
    let listed = mixed.get_all_episodes(21, None, Some("One Piece".to_string())).await.unwrap();
    assert_eq!(listed.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["up"]);
    assert_eq!(down.list_calls(), calls_before);

    // A failure that is not connection-level never triggers the cooldown.
    let mut flaky = MockProvider::new("flaky", Ok(Vec::new()));
    flaky.listing = Listing::Fail(AppError::Network("HTTP 503 from flaky.example".into()));
    let flaky = Arc::new(flaky);
    let flaky_manager = manager_of(&[flaky.clone(), up.clone()]);
    for _ in 0..2 {
        let listed = flaky_manager.get_all_episodes(21, None, Some("One Piece".to_string())).await.unwrap();
        assert_eq!(listed.len(), 1);
    }
    assert_eq!(flaky.list_calls(), 2);
}

// ---------------------------------------------------------------- in-app browser sessions

static GUARDED_SITE: SiteSpec = SiteSpec {
    key: "guarded",
    label: "Guarded",
    origins: &["https://guarded.example"],
};

/// A provider whose site refuses the plain client: it lists and delivers only when it may
/// start an in-app browser session, and records how far each call was allowed to go.
struct GuardedProvider {
    streams: Vec<StreamItem>,
    /// What it answers once a session may be started.
    with_session: Result<(), AppError>,
    calls: Mutex<Vec<(&'static str, SessionUse)>>,
}

impl GuardedProvider {
    fn new(streams: Vec<StreamItem>, with_session: Result<(), AppError>) -> Arc<GuardedProvider> {
        Arc::new(GuardedProvider { streams, with_session, calls: Mutex::new(Vec::new()) })
    }

    fn calls(&self) -> Vec<(&'static str, SessionUse)> {
        self.calls.lock().unwrap().clone()
    }

    fn answer(&self, call: &'static str, access: SessionUse) -> Result<(), AppError> {
        self.calls.lock().unwrap().push((call, access));
        match access {
            SessionUse::Existing => Err(AppError::CaptchaRequired),
            SessionUse::Create => self.with_session.clone(),
        }
    }
}

#[async_trait]
impl AnimeProvider for GuardedProvider {
    fn name(&self) -> &'static str {
        "guarded"
    }

    fn session_site(&self) -> Option<&'static SiteSpec> {
        Some(&GUARDED_SITE)
    }

    async fn get_episodes(&self, query: &AnimeQuery) -> Result<ProviderData, AppError> {
        self.get_episodes_with(query, SessionUse::Existing).await
    }

    async fn get_sources(&self, episode: &EpisodeItem, category: Category) -> Result<SourcesResult, AppError> {
        self.get_sources_with(episode, category, SessionUse::Existing).await
    }

    async fn get_episodes_with(&self, _query: &AnimeQuery, access: SessionUse) -> Result<ProviderData, AppError> {
        self.answer("episodes", access)?;
        Ok(ProviderData {
            name: "guarded".to_string(),
            sub: vec![EpisodeItem::new("guarded|1".to_string(), 1.0, None, None, false)],
            dub: Vec::new(),
        })
    }

    async fn get_sources_with(
        &self,
        _episode: &EpisodeItem,
        _category: Category,
        access: SessionUse,
    ) -> Result<SourcesResult, AppError> {
        self.answer("sources", access)?;
        Ok(SourcesResult::new(self.streams.clone()))
    }
}

fn with_guarded(guarded: &Arc<GuardedProvider>, others: &[Arc<MockProvider>]) -> ProviderManager {
    let mut providers: Vec<Arc<dyn AnimeProvider>> = vec![guarded.clone()];
    providers.extend(others.iter().map(|p| p.clone() as Arc<dyn AnimeProvider>));
    ProviderManager::with_providers(providers)
}

#[tokio::test]
async fn a_provider_picked_by_name_may_start_its_session() {
    let host = host_with_good_stream().await;
    let guarded = GuardedProvider::new(vec![hls(host.url("/hls/master.m3u8"))], Ok(()));
    let manager = with_guarded(&guarded, &[]);

    let result = sources(&manager, 1.0, "sub", Some("guarded")).await.expect("a source");
    assert_eq!(result.provider.as_deref(), Some("guarded"));
    assert_eq!(
        guarded.calls(),
        vec![("episodes", SessionUse::Create), ("sources", SessionUse::Create)]
    );
}

#[tokio::test]
async fn a_background_lookup_that_finds_a_source_starts_no_session() {
    let host = host_with_good_stream().await;
    let stream = vec![hls(host.url("/hls/master.m3u8"))];
    let guarded = GuardedProvider::new(stream.clone(), Ok(()));
    let open = Arc::new(MockProvider::new("open", Ok(stream)));
    let manager = with_guarded(&guarded, &[open]);

    let result = sources(&manager, 1.0, "sub", None).await.expect("a source");
    assert_eq!(result.provider.as_deref(), Some("open"));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(guarded.calls().iter().all(|(_, access)| *access == SessionUse::Existing), "{:?}", guarded.calls());
}

#[tokio::test]
async fn a_session_is_started_as_the_last_resort_when_nothing_else_delivered() {
    let host = host_with_good_stream().await;
    let guarded = GuardedProvider::new(vec![hls(host.url("/hls/master.m3u8"))], Ok(()));
    // Refused as well, but with no session to fall back on: asked once and not again.
    let blocked = Arc::new(MockProvider::new("blocked", Err(AppError::CaptchaRequired)));
    let erroring = Arc::new(MockProvider::new("erroring", Err(AppError::Network("HTTP 500 from x".into()))));
    let manager = with_guarded(&guarded, &[blocked.clone(), erroring.clone()]);

    let result = sources(&manager, 1.0, "sub", None).await.expect("a source from the last resort");
    assert_eq!(result.provider.as_deref(), Some("guarded"));
    assert_eq!(
        guarded.calls(),
        vec![
            ("episodes", SessionUse::Existing),
            ("episodes", SessionUse::Create),
            ("sources", SessionUse::Create)
        ]
    );
    assert_eq!((blocked.source_calls(), erroring.source_calls()), (1, 1));
}

#[tokio::test]
async fn a_failed_last_resort_reports_what_the_session_found() {
    let waiting = AppError::VerificationRequired {
        site: "guarded".into(),
        origin: "https://guarded.example".into(),
    };
    let guarded = GuardedProvider::new(Vec::new(), Err(waiting));
    let blocked = Arc::new(MockProvider::new("blocked", Err(AppError::CaptchaRequired)));
    let manager = with_guarded(&guarded, &[blocked]);

    let message = failure_message(sources(&manager, 1.0, "sub", None).await);
    assert_eq!(
        message,
        "No source found. guarded: needs verification; blocked: blocked by anti-bot check"
    );
    // The last resort ran once; it is not repeated for an answer that is itself a refusal.
    assert_eq!(
        guarded.calls(),
        vec![("episodes", SessionUse::Existing), ("episodes", SessionUse::Create)]
    );
}

#[tokio::test]
async fn listing_episodes_never_starts_a_session() {
    let guarded = GuardedProvider::new(Vec::new(), Ok(()));
    let open = Arc::new(MockProvider::new("open", Ok(Vec::new())));
    let manager = with_guarded(&guarded, &[open]);

    let listed = manager.get_all_episodes(21, Some(21), Some("One Piece".to_string())).await.unwrap();
    let names: Vec<&str> = listed.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["open"]);
    assert_eq!(guarded.calls(), vec![("episodes", SessionUse::Existing)]);
}
