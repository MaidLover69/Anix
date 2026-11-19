//! A queued download that fails is tried again on its own with another stream: the other
//! streams of the same extension first, then the next extension, never the other category.
//!
//! The queue runs against an in-memory database, a temp folder, in-process providers and HTTP
//! servers on 127.0.0.1. Nothing reaches the user's data or the outside network.

mod english_ui;

use anix_lib::db;
use anix_lib::downloads::{self, DownloadSource};
use anix_lib::error::AppError;
use anix_lib::models::{AnimeQuery, Category, DownloadRecord, EpisodeItem, ProviderData, SourcesResult, StreamItem};
use anix_lib::providers::manager::{ProviderManager, NO_OTHER_PROVIDER_MESSAGE, NO_PLUGIN_MESSAGE};
use anix_lib::providers::AnimeProvider;
use async_trait::async_trait;
use english_ui::english_ui;
use sqlx::SqlitePool;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Every test of this binary uses the same wait, so tests running at once agree on it.
const TEST_WAIT: Duration = Duration::from_millis(800);
/// How long a test waits for a row to finish.
const FINISH_WITHIN: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("anix-retry-test-{}-{}-{}", name, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the loopback host answers for one request: a status, a body, and the length it
/// announces (more than the body cuts the connection short).
struct Reply {
    status: u16,
    body: Vec<u8>,
    announced: Option<usize>,
    range_start: Option<usize>,
}

impl Reply {
    fn file(body: &[u8]) -> Self {
        Reply { status: 200, body: body.to_vec(), announced: None, range_start: None }
    }

    fn status(code: u16) -> Self {
        Reply { status: code, body: b"gone".to_vec(), announced: None, range_start: None }
    }

    /// The whole file announced, only its first `sent` bytes sent.
    fn cut(body: &[u8], sent: usize) -> Self {
        Reply { status: 200, body: body[..sent].to_vec(), announced: Some(body.len()), range_start: None }
    }

    /// Answers `Range: bytes=N-` with a 206 of the rest, like a host that supports ranges.
    fn ranged(body: &[u8], range: Option<usize>) -> Self {
        match range {
            Some(from) if from < body.len() => Reply { status: 206, body: body[from..].to_vec(), announced: None, range_start: Some(from) }
                .with_total(body.len()),
            _ => Reply::file(body),
        }
    }

    fn with_total(mut self, total: usize) -> Self {
        // Kept in `announced` only for 206 answers, as the total of the Content-Range.
        self.announced = Some(total);
        self
    }
}

/// Requests the host saw, as (path, range start).
type Seen = Arc<Mutex<Vec<(String, Option<usize>)>>>;

/// Serves every request with `handler`, which gets the path and the `Range` start. Returns
/// the base URL, which downloads are allowed to reach.
async fn serve<F>(handler: F) -> (String, Seen)
where
    F: Fn(&str, Option<usize>) -> Reply + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let base = format!("http://{}", listener.local_addr().unwrap());
    downloads::allow_private_origin_for_tests(&base);
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let (handler, log) = (handler.clone(), log.clone());
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 2048];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&head).to_string();
                let path = text.split(' ').nth(1).unwrap_or("/").to_string();
                let range = text
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("range:").map(|v| v.trim().to_string()))
                    .and_then(|v| v.strip_prefix("bytes=").and_then(|r| r.split('-').next()).and_then(|n| n.parse().ok()));
                log.lock().unwrap().push((path.clone(), range));
                let reply = handler(&path, range);
                let (length, extra) = match reply.range_start {
                    Some(from) => {
                        let total = reply.announced.unwrap_or(reply.body.len());
                        (reply.body.len(), format!("Content-Range: bytes {}-{}/{}\r\n", from, total - 1, total))
                    }
                    None => (reply.announced.unwrap_or(reply.body.len()), String::new()),
                };
                let header = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n{}Connection: close\r\n\r\n",
                    reply.status, length, extra
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&reply.body).await;
                let _ = socket.flush().await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (base, seen)
}

fn requests_to(seen: &Seen, path: &str) -> usize {
    seen.lock().unwrap().iter().filter(|(p, _)| p == path).count()
}

fn media(seed: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i as u32 * 7 + seed as u32) % 251) as u8).collect()
}

fn mp4(url: String, quality: &str, audio: &str) -> StreamItem {
    StreamItem::new(url, "mp4".to_string(), Some(quality.to_string()), Some(audio.to_string()), None, true)
}

/// An extension that answers each source request with the next of `answers` (the last one
/// repeats), with its streams taken as already checked.
struct FakeExtension {
    name: &'static str,
    dub: bool,
    answers: Mutex<VecDeque<Result<Vec<StreamItem>, AppError>>>,
    calls: AtomicUsize,
}

impl FakeExtension {
    fn new(name: &'static str, dub: bool, answers: Vec<Result<Vec<StreamItem>, AppError>>) -> Arc<Self> {
        Arc::new(FakeExtension { name, dub, answers: Mutex::new(answers.into()), calls: AtomicUsize::new(0) })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AnimeProvider for FakeExtension {
    fn name(&self) -> &'static str {
        self.name
    }

    fn supports_dub(&self) -> bool {
        self.dub
    }

    fn allows_private_network(&self) -> bool {
        true // its streams are served on loopback
    }

    fn verifies_streams(&self) -> bool {
        true // so a stream that fails only when downloaded is still handed out
    }

    fn episode_by_number(&self, query: &AnimeQuery, number: f64) -> Option<EpisodeItem> {
        Some(EpisodeItem::new(format!("{}-{}", query.anilist_id, number), number, None, None, false))
    }

    async fn get_episodes(&self, _query: &AnimeQuery) -> Result<ProviderData, AppError> {
        Ok(ProviderData { name: self.name.to_string(), sub: Vec::new(), dub: Vec::new() })
    }

    async fn get_sources(&self, _episode: &EpisodeItem, _category: Category) -> Result<SourcesResult, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut answers = self.answers.lock().unwrap();
        let answer = if answers.len() > 1 { answers.pop_front().unwrap() } else { answers.front().cloned().unwrap() };
        answer.map(SourcesResult::new)
    }
}

fn record(id: &str, category: &str) -> DownloadRecord {
    DownloadRecord {
        id: id.to_string(),
        anilist_id: 1,
        episode_num: 3.0,
        episode_title: Some("Episode 3".to_string()),
        series_title: "Test Show".to_string(),
        series_cover: None,
        provider: "auto".to_string(),
        category: category.to_string(),
        quality: "720p".to_string(),
        status: db::STATUS_QUEUED.to_string(),
        progress: 0.0,
        file_path: None,
        file_size: None,
        duration_s: Some(4.0),
        error_msg: None,
        created_at: 0,
        updated_at: 0,
        audio_lang: None,
        sub_lang: None,
    }
}

/// Starts a queue over `extensions` for one new row, saving under `root`.
async fn queue(rec: &DownloadRecord, extensions: &[Arc<FakeExtension>], root: &Path) -> SqlitePool {
    english_ui();
    downloads::set_retry_wait_for_tests(TEST_WAIT);
    let pool = db::init_db_in_memory().await.expect("in-memory database");
    db::create_download(&pool, rec).await.expect("insert row");
    let manager = ProviderManager::with_providers(extensions.iter().map(|e| e.clone() as Arc<dyn AnimeProvider>).collect());
    downloads::spawn_queue_under(pool.clone(), downloads::lookup_with(Arc::new(manager)), root.to_path_buf());
    pool
}

async fn row(pool: &SqlitePool, id: &str) -> DownloadRecord {
    db::get_download_by_id(pool, id).await.unwrap().expect("row exists")
}

/// Waits until the row is `COMPLETED` or `FAILED`.
async fn finished(pool: &SqlitePool, id: &str) -> DownloadRecord {
    let start = Instant::now();
    loop {
        let rec = row(pool, id).await;
        if rec.status == db::STATUS_COMPLETED || rec.status == db::STATUS_FAILED {
            return rec;
        }
        assert!(start.elapsed() < FINISH_WITHIN, "still {} after {:?}: {:?}", rec.status, FINISH_WITHIN, rec.error_msg);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn saved_bytes(rec: &DownloadRecord) -> Vec<u8> {
    assert_eq!(rec.status, db::STATUS_COMPLETED, "{:?}", rec.error_msg);
    tokio::fs::read(rec.file_path.as_deref().expect("file path")).await.expect("saved file")
}

async fn failure_notices(pool: &SqlitePool) -> usize {
    anix_lib::notices::list(pool, Some(anix_lib::notices::DOWNLOAD)).await.unwrap().len()
}

// ---------------------------------------------------------------------------------------
// When a try is worth it
// ---------------------------------------------------------------------------------------

#[test]
fn only_failures_another_stream_could_fix_are_tried_again() {
    english_ui();
    let cases: [(&str, bool); 18] = [
        ("The sites are too slow to answer right now.\nNo source found. a: timed out", true),
        ("The stream host answered HTTP 404.", true),
        ("A segment of the stream could not be fetched: HTTP 403.", true),
        ("The download was interrupted after 1000 bytes: connection reset", true),
        ("The body is a hls playlist.", true),
        ("The download produced an empty file.", true),
        ("ffmpeg failed (exit code 1): Invalid data found when processing input", true),
        ("Download refused: the stream is on a private address", true),
        (NO_PLUGIN_MESSAGE, false),
        (NO_OTHER_PROVIDER_MESSAGE, false),
        ("Other error: Could not create the download folder /mnt/x: Permission denied (os error 13)", false),
        ("Could not create /d/Show/Episode 03: Not a directory (os error 20)", false),
        ("Could not write /d/x.download.anix-part: No space left on device (os error 28)", false),
        ("ffmpeg failed (exit code 1): /d/x.mkv: No space left on device", false),
        ("ffmpeg failed (exit code 1): /d/x.mkv: Read-only file system", false),
        ("ffmpeg is not installed. Install the ffmpeg package to download this stream.", false),
        ("Anix cannot save downloads in /x: it is not a full path. Choose another folder.", false),
        (
            "No enabled plugin offers dub in German. Choose other languages, or allow plugins in other languages, in Settings › Sources.",
            false,
        ),
    ];
    for (message, again) in cases {
        assert_eq!(downloads::worth_another_try(message), again, "{}", message);
    }
    assert_eq!(downloads::DOWNLOAD_TRIES, 3);
}

#[tokio::test]
async fn a_lookup_that_excludes_every_provider_says_none_is_left() {
    english_ui();
    let x = FakeExtension::new("x", false, vec![Ok(vec![mp4("https://example.org/a.mp4".into(), "720p", "sub")])]);
    let manager = ProviderManager::with_providers(vec![x.clone() as Arc<dyn AnimeProvider>]);
    let err = manager
        .get_sources_for_episode_excluding(1, None, 3.0, "sub", Some("Test Show".into()), None, &["x".to_string()])
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), NO_OTHER_PROVIDER_MESSAGE);
    assert_eq!(x.calls(), 0, "an excluded provider is never asked");
}

// ---------------------------------------------------------------------------------------
// Tries through the queue
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_lookup_that_fails_once_then_succeeds_completes_without_a_manual_retry() {
    let body = media(1, 50_000);
    let served = body.clone();
    let (base, _) = serve(move |_, _| Reply::file(&served)).await;
    let x = FakeExtension::new(
        "x",
        false,
        vec![Err(AppError::Network("timed out".into())), Ok(vec![mp4(format!("{}/a.mp4", base), "720p", "sub")])],
    );
    let tmp = TempDir::new("lookup");
    let rec = record("retry-lookup", "sub");
    let pool = queue(&rec, &[x.clone()], tmp.path()).await;

    let done = finished(&pool, &rec.id).await;

    assert!(saved_bytes(&done).await == body, "the saved file is the served one");
    assert_eq!(x.calls(), 2);
    assert_eq!(failure_notices(&pool).await, 0, "a try that is followed by another is no failure");
}

#[tokio::test]
async fn a_failing_stream_is_replaced_by_another_stream_of_the_same_extension() {
    let body = media(2, 50_000);
    let served = body.clone();
    let (base, seen) = serve(move |path, _| if path == "/a.mp4" { Reply::status(404) } else { Reply::file(&served) }).await;
    let streams = vec![mp4(format!("{}/a.mp4", base), "720p", "sub"), mp4(format!("{}/b.mp4", base), "480p", "sub")];
    let x = FakeExtension::new("x", false, vec![Ok(streams)]);
    let tmp = TempDir::new("same-extension");
    let rec = record("retry-stream", "sub");
    let pool = queue(&rec, &[x.clone()], tmp.path()).await;

    let done = finished(&pool, &rec.id).await;

    assert!(saved_bytes(&done).await == body, "the saved file is the served one");
    assert!(requests_to(&seen, "/a.mp4") >= 1, "the 720p stream is tried first");
    assert_eq!(x.calls(), 2, "the second try asks the same extension again");
}

#[tokio::test]
async fn when_every_stream_of_an_extension_fails_the_next_extension_serves_the_same_category() {
    let body = media(3, 50_000);
    let served = body.clone();
    let (base, seen) = serve(move |path, _| if path == "/y.mp4" { Reply::file(&served) } else { Reply::status(404) }).await;
    let x = FakeExtension::new("x", true, vec![Ok(vec![mp4(format!("{}/x.mp4", base), "720p", "dub")])]);
    let y = FakeExtension::new("y", true, vec![Ok(vec![mp4(format!("{}/y.mp4", base), "720p", "dub")])]);
    let sub_only = FakeExtension::new("subonly", false, vec![Ok(vec![mp4(format!("{}/s.mp4", base), "720p", "sub")])]);
    let tmp = TempDir::new("next-extension");
    let rec = record("retry-extension", "dub");
    let pool = queue(&rec, &[x.clone(), y.clone(), sub_only.clone()], tmp.path()).await;

    let done = finished(&pool, &rec.id).await;

    assert!(saved_bytes(&done).await == body, "the saved file is the served one");
    assert!(done.file_path.as_deref().unwrap().contains("DUB"), "{:?}", done.file_path);
    assert!(requests_to(&seen, "/x.mp4") >= 1);
    assert_eq!(requests_to(&seen, "/s.mp4"), 0, "a dub never switches to a sub");
    assert_eq!(sub_only.calls(), 0);
}

#[tokio::test]
async fn a_failed_player_stream_is_followed_by_another_stream_from_the_extensions() {
    let body = media(4, 50_000);
    let served = body.clone();
    let (base, seen) = serve(move |path, _| if path == "/player.mp4" { Reply::status(404) } else { Reply::file(&served) }).await;
    let player = format!("{}/player.mp4", base);
    // The extension still lists the player's stream first; it is not tried a second time.
    let x = FakeExtension::new(
        "x",
        false,
        vec![Ok(vec![mp4(player.clone(), "720p", "sub"), mp4(format!("{}/b.mp4", base), "480p", "sub")])],
    );
    let tmp = TempDir::new("player");
    let rec = record("retry-player", "sub");
    downloads::queue_with_source(&rec.id, DownloadSource { url: player, ..DownloadSource::default() });
    let pool = queue(&rec, &[x.clone()], tmp.path()).await;

    let done = finished(&pool, &rec.id).await;

    assert!(saved_bytes(&done).await == body, "the saved file is the served one");
    let player_requests = requests_to(&seen, "/player.mp4");
    assert!(player_requests >= 1);
    assert_eq!(x.calls(), 1, "only the second try asks the extensions");
    assert_eq!(requests_to(&seen, "/player.mp4"), player_requests);
}

#[tokio::test]
async fn a_local_failure_is_not_tried_again() {
    let (base, _) = serve(|_, _| Reply::file(b"bytes")).await;
    let x = FakeExtension::new("x", false, vec![Ok(vec![mp4(format!("{}/a.mp4", base), "720p", "sub")])]);
    let tmp = TempDir::new("local");
    // A file where the download folder should be: no folder can be made in it.
    let blocked = tmp.path().join("not-a-folder");
    std::fs::write(&blocked, b"x").unwrap();
    let rec = record("retry-local", "sub");
    let pool = queue(&rec, &[x.clone()], &blocked).await;

    let done = finished(&pool, &rec.id).await;

    assert_eq!(done.status, db::STATUS_FAILED);
    assert!(done.error_msg.as_deref().unwrap_or("").contains("Could not create"), "{:?}", done.error_msg);
    tokio::time::sleep(TEST_WAIT * 2).await;
    assert_eq!(x.calls(), 1, "no second try");
    assert_eq!(failure_notices(&pool).await, 1);
}

#[tokio::test]
async fn cancelling_during_the_wait_stops_the_download_with_no_further_request() {
    let (base, seen) = serve(|_, _| Reply::status(404)).await;
    let x = FakeExtension::new(
        "x",
        false,
        vec![Ok(vec![mp4(format!("{}/a.mp4", base), "720p", "sub"), mp4(format!("{}/b.mp4", base), "480p", "sub")])],
    );
    let tmp = TempDir::new("cancel");
    let rec = record("retry-cancel", "sub");
    let pool = queue(&rec, &[x.clone()], tmp.path()).await;

    // Waits for the note that a next try is coming, then cancels at once.
    let start = Instant::now();
    loop {
        let now = row(&pool, &rec.id).await;
        if now.error_msg.as_deref().is_some_and(|m| m.starts_with("Trying another stream (2 of 3)")) {
            assert_ne!(now.status, db::STATUS_FAILED, "a row waiting for its next try is not failed");
            break;
        }
        assert!(start.elapsed() < FINISH_WITHIN, "no retry note: {:?}", now);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(downloads::cancel_download(&rec.id).await, "the waiting try can be cancelled");
    let requests = seen.lock().unwrap().len();

    tokio::time::sleep(TEST_WAIT * 3 + Duration::from_secs(6)).await;
    assert_eq!(seen.lock().unwrap().len(), requests, "no request after the cancel");
    assert_eq!(requests_to(&seen, "/b.mp4"), 0);
    assert_eq!(x.calls(), 1);
    assert_ne!(row(&pool, &rec.id).await.status, db::STATUS_FAILED);
    assert_eq!(failure_notices(&pool).await, 0);
}

#[tokio::test]
async fn when_every_try_fails_the_row_fails_once_with_the_last_message() {
    let (base, seen) = serve(|path, _| if path == "/c.mp4" { Reply::status(410) } else { Reply::status(404) }).await;
    let streams = vec![
        mp4(format!("{}/a.mp4", base), "720p", "sub"),
        mp4(format!("{}/b.mp4", base), "480p", "sub"),
        mp4(format!("{}/c.mp4", base), "360p", "sub"),
        mp4(format!("{}/d.mp4", base), "240p", "sub"),
    ];
    let x = FakeExtension::new("x", false, vec![Ok(streams)]);
    let tmp = TempDir::new("all-fail");
    let rec = record("retry-all", "sub");
    let pool = queue(&rec, &[x.clone()], tmp.path()).await;

    let done = finished(&pool, &rec.id).await;

    assert_eq!(done.status, db::STATUS_FAILED);
    assert!(done.error_msg.as_deref().unwrap_or("").contains("410"), "the last try's reason: {:?}", done.error_msg);
    assert_eq!(failure_notices(&pool).await, 1, "one notice, not one per try");
    tokio::time::sleep(TEST_WAIT * 2).await;
    assert_eq!(x.calls(), 3);
    assert_eq!(requests_to(&seen, "/d.mp4"), 0, "three tries in all");

    // Try again starts a fresh round.
    assert!(downloads::retry_download(&pool, &rec.id).await.unwrap());
    let again = finished(&pool, &rec.id).await;
    assert_eq!(again.status, db::STATUS_FAILED);
    assert_eq!(x.calls(), 6);
    assert_eq!(failure_notices(&pool).await, 2);
}

#[tokio::test]
async fn the_saved_part_of_one_stream_is_never_joined_with_another_stream() {
    let a = media(5, 400_000);
    let b = media(6, 400_000);
    let (a_served, b_served) = (a.clone(), b.clone());
    let a_asked = AtomicUsize::new(0);
    let (base, seen) = serve(move |path, range| match path {
        // The first answer of stream A (the whole file, range or not) breaks off half-way;
        // asked again it is gone. The half it sent stays saved for a resume.
        "/a.mp4" if a_asked.fetch_add(1, Ordering::SeqCst) == 0 => Reply::cut(&a_served, 200_000),
        "/a.mp4" => Reply::status(404),
        _ => Reply::ranged(&b_served, range),
    })
    .await;
    let x = FakeExtension::new(
        "x",
        false,
        vec![Ok(vec![mp4(format!("{}/a.mp4", base), "720p", "sub"), mp4(format!("{}/b.mp4", base), "480p", "sub")])],
    );
    let tmp = TempDir::new("no-join");
    let rec = record("retry-join", "sub");
    let pool = queue(&rec, &[x.clone()], tmp.path()).await;

    let done = finished(&pool, &rec.id).await;

    assert!(saved_bytes(&done).await == b, "stream B saved whole, with nothing of stream A");
    assert!(requests_to(&seen, "/a.mp4") >= 1);
    let b_ranges: Vec<Option<usize>> =
        seen.lock().unwrap().iter().filter(|(p, _)| p == "/b.mp4").map(|(_, r)| *r).collect();
    assert!(b_ranges.iter().all(|r| r.unwrap_or(0) == 0), "B is never asked from the middle: {:?}", b_ranges);
    let leftovers: Vec<String> = walk(tmp.path()).into_iter().filter(|n| n.ends_with(downloads::PART_SUFFIX)).collect();
    assert!(leftovers.is_empty(), "{:?}", leftovers);
}

fn walk(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        names.push(path.file_name().unwrap().to_string_lossy().to_string());
        if path.is_dir() {
            names.extend(walk(&path));
        }
    }
    names
}
