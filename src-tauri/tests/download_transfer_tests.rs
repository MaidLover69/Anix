//! End-to-end tests of the download path against an HTTP server on 127.0.0.1.
//!
//! They use an in-memory database and a fresh temp directory, never the user's database,
//! settings or download folder, and nothing outside the loopback interface. The tests that
//! need ffmpeg print a note and pass when it is not installed.

use anix_lib::db;
use anix_lib::downloads::{self, DownloadOutcome, DownloadSource, PART_SUFFIX};
use anix_lib::models::{DownloadRecord, SubtitleItem};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-dl-test-{}-{}-{}",
            name,
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn file_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Request {
    path: String,
    headers: HashMap<String, String>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    /// `Content-Length` to announce when it should differ from the body sent.
    announced_len: Option<usize>,
    /// Keep the connection open after the body instead of closing it.
    hang: bool,
}

impl Response {
    fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self { status: 200, content_type, body: body.into(), announced_len: None, hang: false }
    }

    fn status(status: u16) -> Self {
        Self { status, content_type: "text/plain", body: b"denied".to_vec(), announced_len: None, hang: false }
    }
}

/// Serves every request with `handler` and records the requests it saw. Returns the base
/// URL (`http://127.0.0.1:<port>`).
async fn serve<F>(handler: F) -> (String, Arc<Mutex<Vec<Request>>>)
where
    F: Fn(&Request) -> Response + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Request>>> = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);

    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let handler = handler.clone();
            let log = log.clone();
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
                let mut lines = text.split("\r\n");
                let path = lines.next().unwrap_or("").split(' ').nth(1).unwrap_or("/").to_string();
                let headers = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                    .collect();
                let request = Request { path, headers };

                let response = handler(&request);
                log.lock().unwrap().push(request);

                let reason = if response.status == 200 { "OK" } else { "Error" };
                let header = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    reason,
                    response.content_type,
                    response.announced_len.unwrap_or(response.body.len())
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&response.body).await;
                let _ = socket.flush().await;
                if response.hang {
                    tokio::time::sleep(Duration::from_secs(120)).await;
                }
                let _ = socket.shutdown().await;
            });
        }
    });

    (base, seen)
}

fn record(id: &str) -> DownloadRecord {
    DownloadRecord {
        id: id.to_string(),
        anilist_id: 1,
        episode_num: 3.0,
        episode_title: Some("Episode 3".to_string()),
        series_title: "Test Show".to_string(),
        series_cover: None,
        provider: "auto".to_string(),
        category: "sub".to_string(),
        quality: "720p".to_string(),
        status: "DOWNLOADING".to_string(),
        progress: 0.0,
        file_path: None,
        file_size: None,
        duration_s: Some(4.0),
        error_msg: None,
        created_at: 0,
        updated_at: 0,
    }
}

async fn db_with(record: &DownloadRecord) -> SqlitePool {
    let pool = db::init_db_in_memory().await.expect("in-memory database");
    db::create_download(&pool, record).await.expect("insert row");
    pool
}

fn source(url: String) -> DownloadSource {
    DownloadSource { url, ..DownloadSource::default() }
}

async fn row(pool: &SqlitePool, id: &str) -> DownloadRecord {
    db::get_download_by_id(pool, id).await.unwrap().expect("row exists")
}

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Encodes four seconds of test video into an HLS playlist whose segments are named
/// `.jpg`, the way stream hosts disguise them. Returns the files by name.
fn make_hls(dir: &Path) -> Option<HashMap<String, Vec<u8>>> {
    let status = std::process::Command::new("ffmpeg")
        .current_dir(dir)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=duration=4:size=160x120:rate=10",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
            "-c:v", "mpeg2video", "-c:a", "mp2",
            "-f", "hls", "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", "seg%d.jpg", "index.m3u8",
        ])
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }

    let mut files = HashMap::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        files.insert(name, std::fs::read(entry.path()).ok()?);
    }
    files.contains_key("index.m3u8").then_some(files)
}

/// The stream lines ffmpeg reports for `file` ("Video: ...", "Subtitle: ...").
fn stream_kinds(file: &Path) -> String {
    let output = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-i"])
        .arg(file)
        .output()
        .expect("run ffmpeg");
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|l| l.contains("Stream #"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------------------
// Direct files
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn direct_file_is_saved_completely_and_marked_completed() {
    // Not a real video, so the optional remux cannot succeed and the bytes must be kept
    // exactly as served, whether or not ffmpeg is installed.
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let served = body.clone();
    let (base, seen) = serve(move |_| Response::ok("video/mp4", served.clone())).await;

    let tmp = TempDir::new("direct");
    let rec = record("direct-ok");
    let pool = db_with(&rec).await;
    let mut src = source(format!("{}/video/ep3.mp4", base));
    src.referer = Some("https://player.example/embed/9".to_string());
    src.headers.insert("X-Token".to_string(), "abc".to_string());

    let outcome = downloads::download_into(&pool, &rec, src, tmp.path()).await;

    let expected_path = tmp.path().join("Test Show - EP 03 [SUB 720p].mp4");
    assert_eq!(
        outcome,
        DownloadOutcome::Completed { path: expected_path.clone(), size: body.len() as i64 }
    );
    assert_eq!(std::fs::read(&expected_path).unwrap(), body);
    assert_eq!(tmp.file_names(), vec!["Test Show - EP 03 [SUB 720p].mp4"], "no working files are left");

    let stored = row(&pool, "direct-ok").await;
    assert_eq!(stored.status, "COMPLETED");
    assert_eq!(stored.progress, 1.0);
    assert_eq!(stored.file_size, Some(body.len() as i64));
    assert_eq!(stored.file_path.as_deref(), Some(expected_path.to_str().unwrap()));
    assert_eq!(stored.error_msg, None);

    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, "/video/ep3.mp4");
    assert_eq!(seen[0].header("referer"), Some("https://player.example/embed/9"));
    assert_eq!(seen[0].header("origin"), Some("https://player.example"));
    assert_eq!(seen[0].header("x-token"), Some("abc"));
    assert!(seen[0].header("user-agent").unwrap().starts_with("Mozilla/5.0"));
}

#[tokio::test]
async fn a_second_download_of_the_same_episode_does_not_overwrite_the_first() {
    let (base, _) = serve(|req| Response::ok("video/mp4", req.path.repeat(5000).into_bytes())).await;
    let tmp = TempDir::new("twice");

    let first = record("first");
    let second = record("second");
    let pool = db_with(&first).await;
    db::create_download(&pool, &second).await.unwrap();

    let a = downloads::download_into(&pool, &first, source(format!("{}/a.mp4", base)), tmp.path()).await;
    let b = downloads::download_into(&pool, &second, source(format!("{}/b.mp4", base)), tmp.path()).await;

    assert!(matches!(a, DownloadOutcome::Completed { .. }), "{:?}", a);
    assert!(matches!(b, DownloadOutcome::Completed { .. }), "{:?}", b);
    assert_eq!(
        tmp.file_names(),
        vec!["Test Show - EP 03 [SUB 720p] (2).mp4", "Test Show - EP 03 [SUB 720p].mp4"]
    );
    let first_file = std::fs::read(tmp.path().join("Test Show - EP 03 [SUB 720p].mp4")).unwrap();
    assert!(first_file.starts_with(b"/a.mp4"), "the first file still holds the first download");
}

#[tokio::test]
async fn truncated_body_is_a_failure_not_a_completed_download() {
    let (base, _) = serve(|_| Response {
        status: 200,
        content_type: "video/mp4",
        body: vec![7u8; 50_000],
        announced_len: Some(200_000),
        hang: false,
    })
    .await;

    let tmp = TempDir::new("truncated");
    let rec = record("truncated");
    let pool = db_with(&rec).await;

    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/v.mp4", base)), tmp.path()).await;

    let DownloadOutcome::Failed(message) = outcome else {
        panic!("a body cut short must fail, got {:?}", outcome);
    };
    assert!(
        message.contains("interrupted") || message.contains("incomplete"),
        "message was: {}",
        message
    );
    assert!(tmp.file_names().is_empty(), "the partial file is removed: {:?}", tmp.file_names());

    let stored = row(&pool, "truncated").await;
    assert_eq!(stored.status, "FAILED");
    assert_eq!(stored.error_msg.as_deref(), Some(message.as_str()));
    assert_eq!(stored.file_path, None);
}

#[tokio::test]
async fn http_error_and_html_page_fail_with_a_reason() {
    let (base, _) = serve(|req| {
        if req.path == "/page.mp4" {
            Response::ok("text/html", "<!DOCTYPE html><html><body>Just a moment...</body></html>")
        } else {
            Response::status(403)
        }
    })
    .await;
    let tmp = TempDir::new("errors");

    let rec = record("forbidden");
    let pool = db_with(&rec).await;
    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/v.mp4", base)), tmp.path()).await;
    let DownloadOutcome::Failed(message) = outcome else { panic!("{:?}", outcome) };
    assert!(message.contains("403"), "message was: {}", message);
    assert_eq!(row(&pool, "forbidden").await.status, "FAILED");

    let rec = record("html");
    db::create_download(&pool, &rec).await.unwrap();
    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/page.mp4", base)), tmp.path()).await;
    let DownloadOutcome::Failed(message) = outcome else { panic!("{:?}", outcome) };
    assert!(message.contains("web page"), "message was: {}", message);
    assert_eq!(row(&pool, "html").await.error_msg.as_deref(), Some(message.as_str()));

    assert!(tmp.file_names().is_empty(), "{:?}", tmp.file_names());
}

#[tokio::test]
async fn non_http_urls_never_start_a_transfer() {
    let tmp = TempDir::new("scheme");
    let rec = record("scheme");
    let pool = db_with(&rec).await;

    for url in ["file:///etc/hostname", "concat:/etc/hostname", "-version"] {
        let outcome = downloads::download_into(&pool, &rec, source(url.to_string()), tmp.path()).await;
        assert!(matches!(outcome, DownloadOutcome::Failed(_)), "{} gave {:?}", url, outcome);
    }
    assert_eq!(row(&pool, "scheme").await.status, "FAILED");
    assert!(tmp.file_names().is_empty());
}

#[tokio::test]
async fn cancelling_stops_the_transfer_and_removes_the_partial_file() {
    // Sends the first megabyte and then stalls with the connection open.
    let (base, _) = serve(|_| Response {
        status: 200,
        content_type: "video/mp4",
        body: vec![1u8; 1_500_000],
        announced_len: Some(50_000_000),
        hang: true,
    })
    .await;

    let tmp = TempDir::new("cancel");
    let rec = record("cancel-me");
    let pool = db_with(&rec).await;
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let task = {
        let pool = pool.clone();
        let rec = rec.clone();
        let dir = tmp.path().to_path_buf();
        let src = source(format!("{}/v.mp4", base));
        tokio::spawn(async move {
            downloads::download_into_cancellable(&pool, &rec, src, &dir, cancel_rx).await
        })
    };

    // Wait until the transfer is really under way.
    let mut started = false;
    for _ in 0..200 {
        if tmp.file_names().iter().any(|n| n.ends_with(PART_SUFFIX)) && row(&pool, "cancel-me").await.file_size.is_some() {
            started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(started, "the download never started: {:?}", tmp.file_names());

    cancel_tx.send(true).unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("a cancelled download stops promptly")
        .unwrap();

    assert_eq!(outcome, DownloadOutcome::Cancelled);
    assert!(tmp.file_names().is_empty(), "partial files must be gone: {:?}", tmp.file_names());
    assert_eq!(row(&pool, "cancel-me").await.status, "DOWNLOADING", "the row is left for the caller to delete");
}

#[tokio::test]
async fn a_download_whose_row_was_deleted_does_not_run() {
    let (base, seen) = serve(|_| Response::ok("video/mp4", vec![1u8; 1000])).await;
    let tmp = TempDir::new("deleted");
    let rec = record("gone");
    let pool = db_with(&rec).await;
    db::delete_download(&pool, "gone").await.unwrap();

    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/v.mp4", base)), tmp.path()).await;

    assert_eq!(outcome, DownloadOutcome::Cancelled);
    assert!(tmp.file_names().is_empty());
    assert!(seen.lock().unwrap().is_empty(), "nothing was requested");
}

// ---------------------------------------------------------------------------------------
// HLS
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn hls_failure_marks_the_row_failed_and_never_saves_the_playlist() {
    // The playlist is served, but every segment is refused.
    let (base, _) = serve(|req| {
        if req.path.ends_with(".m3u8") || req.path == "/stream" {
            Response::ok(
                "application/vnd.apple.mpegurl",
                "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:2\n#EXTINF:2.0,\nseg0.ts\n#EXT-X-ENDLIST\n",
            )
        } else {
            Response::status(403)
        }
    })
    .await;
    let tmp = TempDir::new("hls-fail");
    let pool = db::init_db_in_memory().await.unwrap();

    // Once by its `.m3u8` name, once behind a URL that does not look like a playlist.
    for (id, path) in [("hls-named", "/master.m3u8"), ("hls-unnamed", "/stream")] {
        let rec = record(id);
        db::create_download(&pool, &rec).await.unwrap();

        let outcome = downloads::download_into(&pool, &rec, source(format!("{}{}", base, path)), tmp.path()).await;

        let DownloadOutcome::Failed(message) = outcome else {
            panic!("{}: an HLS stream that cannot be fetched must fail, got {:?}", id, outcome);
        };
        if ffmpeg_available() {
            assert!(message.starts_with("ffmpeg failed (exit code"), "{}: {}", id, message);
            assert!(message.contains("403"), "{}: ffmpeg's own error is passed on: {}", id, message);
        } else {
            assert!(message.contains("ffmpeg is not installed"), "{}: {}", id, message);
        }

        let stored = row(&pool, id).await;
        assert_eq!(stored.status, "FAILED");
        assert_eq!(stored.error_msg.as_deref(), Some(message.as_str()));
        assert_eq!(stored.file_path, None);
        assert!(tmp.file_names().is_empty(), "{}: nothing may be left on disk: {:?}", id, tmp.file_names());
    }
}

#[tokio::test]
async fn hls_stream_is_muxed_with_headers_and_survives_a_bad_subtitle() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let media = TempDir::new("hls-media");
    let Some(files) = make_hls(media.path()) else {
        eprintln!("skipped: this ffmpeg cannot generate the test stream");
        return;
    };

    const REFERER: &str = "https://player.example/embed/42";
    let (base, seen) = serve(move |req| {
        // Like a real host: nothing without the right Referer, token and a browser UA.
        let allowed = req.header("referer") == Some(REFERER)
            && req.header("origin") == Some("https://player.example")
            && req.header("x-token") == Some("s3cret")
            && req.header("user-agent").is_some_and(|ua| ua.starts_with("Mozilla/5.0"));
        if !allowed {
            return Response::status(403);
        }
        match req.path.as_str() {
            "/subs/blocked.vtt" => Response::ok("text/html", "<html><body>Access denied</body></html>"),
            "/subs/missing.vtt" => Response::status(404),
            "/subs/en.vtt" => Response::ok(
                "text/vtt",
                "WEBVTT\n\n00:00:00.500 --> 00:00:02.000\nHello there\n\n00:00:02.500 --> 00:00:03.500\nGoodbye\n",
            ),
            "/subs/es.srt" => Response::ok("text/plain", "1\n00:00:00,500 --> 00:00:02,000\nHola\n"),
            path => match files.get(path.trim_start_matches("/hls/")) {
                Some(body) => Response::ok("application/octet-stream", body.clone()),
                None => Response::status(404),
            },
        }
    })
    .await;

    let tmp = TempDir::new("hls-ok");
    let rec = record("hls-ok");
    let pool = db_with(&rec).await;

    let subtitle = |path: &str, language: &str, is_default: bool| SubtitleItem {
        url: format!("{}{}", base, path),
        language: language.to_string(),
        format: "vtt".to_string(),
        is_default: Some(is_default),
    };
    let src = DownloadSource {
        url: format!("{}/hls/index.m3u8", base),
        stream_type: Some("hls".to_string()),
        referer: Some(REFERER.to_string()),
        origin: None,
        headers: HashMap::from([("X-Token".to_string(), "s3cret".to_string())]),
        subtitles: vec![
            subtitle("/subs/blocked.vtt", "German", false),
            subtitle("/subs/missing.vtt", "French", false),
            subtitle("/subs/en.vtt", "English", true),
            subtitle("/subs/es.srt", "Spanish", false),
        ],
    };

    let outcome = downloads::download_into(&pool, &rec, src, tmp.path()).await;

    let DownloadOutcome::Completed { path, size } = outcome else {
        panic!("the stream must download, got {:?}", outcome);
    };
    assert_eq!(path, tmp.path().join("Test Show - EP 03 [SUB 720p].mkv"));
    assert!(size > 100_000, "size was {}", size);
    assert_eq!(std::fs::metadata(&path).unwrap().len() as i64, size);

    // Video, audio and the default subtitle are inside; the other good subtitle is kept
    // next to it; the two bad ones left nothing behind.
    let kinds = stream_kinds(&path);
    assert!(kinds.contains("Video:"), "{}", kinds);
    assert!(kinds.contains("Audio:"), "{}", kinds);
    assert!(kinds.contains("Subtitle:"), "{}", kinds);
    assert_eq!(
        tmp.file_names(),
        vec!["Test Show - EP 03 [SUB 720p].mkv", "Test Show - EP 03 [SUB 720p].spanish.srt"]
    );

    let stored = row(&pool, "hls-ok").await;
    assert_eq!(stored.status, "COMPLETED");
    assert_eq!(stored.progress, 1.0);
    assert_eq!(stored.file_size, Some(size));
    assert_eq!(stored.file_path.as_deref(), Some(path.to_str().unwrap()));
    assert_eq!(stored.error_msg, None);

    // Every request, including the ones ffmpeg made for the playlist and the disguised
    // segments, carried the stream's headers.
    let seen = seen.lock().unwrap();
    for wanted in ["/hls/index.m3u8", "/hls/seg0.jpg", "/hls/seg1.jpg", "/subs/en.vtt", "/subs/blocked.vtt"] {
        assert!(seen.iter().any(|r| r.path == wanted), "{} was never requested", wanted);
    }
    for request in seen.iter() {
        assert_eq!(request.header("referer"), Some(REFERER), "{}", request.path);
        assert_eq!(request.header("x-token"), Some("s3cret"), "{}", request.path);
    }
}

#[tokio::test]
async fn hls_stream_still_downloads_when_ffmpeg_rejects_the_subtitle() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let media = TempDir::new("hls-media-2");
    let Some(files) = make_hls(media.path()) else {
        eprintln!("skipped: this ffmpeg cannot generate the test stream");
        return;
    };

    let (base, _) = serve(move |req| match req.path.as_str() {
        // A well-formed SRT in a legacy encoding: it is a subtitle file, but ffmpeg refuses
        // to convert text that is not UTF-8.
        "/subs/legacy.srt" => Response::ok(
            "text/plain",
            b"1\n00:00:00,500 --> 00:00:02,000\nCaf\xe9 \xff\xfe\n".to_vec(),
        ),
        path => match files.get(path.trim_start_matches("/hls/")) {
            Some(body) => Response::ok("application/octet-stream", body.clone()),
            None => Response::status(404),
        },
    })
    .await;

    let tmp = TempDir::new("hls-badsub");
    let rec = record("hls-badsub");
    let pool = db_with(&rec).await;
    let src = DownloadSource {
        url: format!("{}/hls/index.m3u8", base),
        subtitles: vec![SubtitleItem {
            url: format!("{}/subs/legacy.srt", base),
            language: "English".to_string(),
            format: "srt".to_string(),
            is_default: Some(true),
        }],
        ..DownloadSource::default()
    };

    let outcome = downloads::download_into(&pool, &rec, src, tmp.path()).await;

    assert!(matches!(outcome, DownloadOutcome::Completed { .. }), "{:?}", outcome);
    let video = tmp.path().join("Test Show - EP 03 [SUB 720p].mkv");
    let kinds = stream_kinds(&video);
    assert!(kinds.contains("Video:"), "{}", kinds);
    assert_eq!(row(&pool, "hls-badsub").await.status, "COMPLETED");

    // The subtitle is not lost either: where ffmpeg would not take it, it is kept beside
    // the video.
    let side_file = tmp.path().join("Test Show - EP 03 [SUB 720p].english.srt");
    assert!(
        kinds.contains("Subtitle:") || side_file.exists(),
        "streams: {}\nfiles: {:?}",
        kinds,
        tmp.file_names()
    );
    println!("subtitle embedded: {}, kept as side file: {}", kinds.contains("Subtitle:"), side_file.exists());
    assert!(
        tmp.file_names().iter().all(|n| !n.ends_with(PART_SUFFIX)),
        "{:?}",
        tmp.file_names()
    );
}
