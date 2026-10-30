//! End-to-end tests of the download path against an HTTP server on 127.0.0.1.
//!
//! They use an in-memory database and a fresh temp directory, never the user's database,
//! settings or download folder, and nothing outside the loopback interface. The tests that
//! need ffmpeg print a note and pass when it is not installed.

use anix_lib::db;
use anix_lib::downloads::{self, work_file_path, work_tag, DownloadOutcome, DownloadSource, PART_SUFFIX, RESUME_ROLES};
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
    /// More header lines, such as `Content-Range`.
    extra: Vec<String>,
}

impl Response {
    fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self { status: 200, content_type, body: body.into(), announced_len: None, hang: false, extra: Vec::new() }
    }

    fn status(status: u16) -> Self {
        Self { status, content_type: "text/plain", body: b"denied".to_vec(), announced_len: None, hang: false, extra: Vec::new() }
    }
}

/// Answers like a host that supports byte ranges: `Range: bytes=N-` gets a 206 with the rest
/// of `body`; without one, the whole body.
fn ranged(req: &Request, body: &[u8]) -> Response {
    let start = req
        .header("range")
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.strip_suffix('-'))
        .and_then(|n| n.parse::<usize>().ok());
    match start {
        Some(from) if from < body.len() => Response {
            status: 206,
            extra: vec![format!("Content-Range: bytes {}-{}/{}", from, body.len() - 1, body.len())],
            ..Response::ok("video/mp4", body[from..].to_vec())
        },
        Some(_) => Response::status(416),
        None => Response::ok("video/mp4", body.to_vec()),
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
                let extra: String = response.extra.iter().map(|line| format!("{}\r\n", line)).collect();
                let header = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                    response.status,
                    reason,
                    response.content_type,
                    response.announced_len.unwrap_or(response.body.len()),
                    extra
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
        audio_lang: None,
        sub_lang: None,
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
    assert_eq!(stored.audio_lang.as_deref(), Some("ja"), "a sub is Japanese audio");
    assert_eq!(stored.sub_lang, None, "no subtitle track was offered");

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
        extra: Vec::new(),
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
    // A network failure keeps the bytes so far and their total, so Try again can resume.
    let kept: Vec<String> = ["download", "resume"]
        .iter()
        .map(|role| work_file_path(tmp.path(), &rec, role).file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(tmp.file_names(), kept, "only the resumable files are left");

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
        extra: Vec::new(),
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
            assert!(message.contains("segment of the stream could not be fetched"), "{}: {}", id, message);
            assert!(message.contains("403"), "{}: the host's answer is passed on: {}", id, message);
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
        language: None,
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
    assert_eq!((stored.audio_lang.as_deref(), stored.sub_lang.as_deref()), (Some("ja"), Some("en")));
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

// ---------------------------------------------------------------------------------------
// Resume after the app was stopped
// ---------------------------------------------------------------------------------------

fn test_body(len: usize) -> Vec<u8> {
    (0..len as u32).map(|i| (i % 253) as u8).collect()
}

/// What a stopped run leaves: the first `have` bytes and the total they belong to.
fn leave_partial(dir: &Path, rec: &DownloadRecord, bytes: &[u8], total: usize) {
    std::fs::write(work_file_path(dir, rec, "download"), bytes).unwrap();
    std::fs::write(work_file_path(dir, rec, "resume"), total.to_string()).unwrap();
}

#[tokio::test]
async fn an_interrupted_direct_download_resumes_with_a_range_request() {
    let body = test_body(300_000);
    let served = body.clone();
    let (base, seen) = serve(move |req| ranged(req, &served)).await;
    let tmp = TempDir::new("resume");
    let rec = record("resume-me");
    let pool = db_with(&rec).await;
    leave_partial(tmp.path(), &rec, &body[..120_000], body.len());

    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/v.mp4", base)), tmp.path()).await;

    let path = tmp.path().join("Test Show - EP 03 [SUB 720p].mp4");
    assert_eq!(outcome, DownloadOutcome::Completed { path: path.clone(), size: body.len() as i64 });
    assert_eq!(std::fs::read(&path).unwrap(), body, "the saved bytes and the rest join up exactly");
    assert_eq!(tmp.file_names(), vec!["Test Show - EP 03 [SUB 720p].mp4"]);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].header("range"), Some("bytes=120000-"), "only the missing part is asked for");
}

#[tokio::test]
async fn a_host_that_ignores_the_range_gets_a_fresh_download() {
    let body = test_body(200_000);
    let served = body.clone();
    let (base, seen) = serve(move |_| Response::ok("video/mp4", served.clone())).await;
    let tmp = TempDir::new("resume-ignored");
    let rec = record("ignored");
    let pool = db_with(&rec).await;
    leave_partial(tmp.path(), &rec, &[9u8; 50_000], body.len());

    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/v.mp4", base)), tmp.path()).await;

    assert!(matches!(outcome, DownloadOutcome::Completed { .. }), "{:?}", outcome);
    let path = tmp.path().join("Test Show - EP 03 [SUB 720p].mp4");
    assert_eq!(std::fs::read(&path).unwrap(), body, "the old bytes were replaced, not kept in front");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_partial_file_of_another_stream_is_not_resumed() {
    // The saved bytes belong to a body of another size, so the 206 does not match them.
    let body = test_body(250_000);
    let served = body.clone();
    let (base, seen) = serve(move |req| ranged(req, &served)).await;
    let tmp = TempDir::new("resume-other");
    let rec = record("other");
    let pool = db_with(&rec).await;
    leave_partial(tmp.path(), &rec, &[1u8; 100_000], 999_999);

    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/v.mp4", base)), tmp.path()).await;

    assert!(matches!(outcome, DownloadOutcome::Completed { .. }), "{:?}", outcome);
    assert_eq!(std::fs::read(tmp.path().join("Test Show - EP 03 [SUB 720p].mp4")).unwrap(), body);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "asked once with a range, then for the whole file");
    assert_eq!(seen[0].header("range"), Some("bytes=100000-"));
    assert_eq!(seen[1].header("range"), None);
}

#[tokio::test]
async fn a_download_stopped_with_the_app_continues_from_its_last_byte_after_a_relaunch() {
    let body = test_body(3_000_000);
    let served = body.clone();
    let first = Arc::new(AtomicU32::new(0));
    let (base, seen) = serve(move |req| {
        if first.fetch_add(1, Ordering::SeqCst) == 0 {
            // The first run gets half of the file, then the connection hangs.
            return Response {
                announced_len: Some(served.len()),
                hang: true,
                ..Response::ok("video/mp4", served[..1_500_000].to_vec())
            };
        }
        ranged(req, &served)
    })
    .await;
    let tmp = TempDir::new("relaunch");
    let rec = record("relaunch");
    let pool = db_with(&rec).await;
    let partial = work_file_path(tmp.path(), &rec, "download");

    let task = {
        let (pool, rec, dir, src) = (pool.clone(), rec.clone(), tmp.path().to_path_buf(), source(format!("{}/v.mp4", base)));
        tokio::spawn(async move { downloads::download_into(&pool, &rec, src, &dir).await })
    };
    let mut on_disk = 0;
    for _ in 0..400 {
        on_disk = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
        if on_disk >= 512 * 1024 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(on_disk >= 512 * 1024, "the first run never wrote its first half megabyte");
    // Like killing the app: the work stops where it is and cleans nothing up.
    task.abort();
    let _ = task.await;
    let on_disk = std::fs::metadata(&partial).unwrap().len();

    // The relaunch: the row goes back to the queue and the startup cleanup keeps its bytes.
    assert_eq!(db::reconcile_stale_downloads(&pool).await.unwrap(), 1);
    downloads::remove_stale_part_files(tmp.path(), &[work_tag(&rec.id)]);
    assert_eq!(std::fs::metadata(&partial).unwrap().len(), on_disk);

    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/v.mp4", base)), tmp.path()).await;

    assert!(matches!(outcome, DownloadOutcome::Completed { .. }), "{:?}", outcome);
    assert_eq!(std::fs::read(tmp.path().join("Test Show - EP 03 [SUB 720p].mp4")).unwrap(), body);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].header("range").map(str::to_string), Some(format!("bytes={}-", on_disk)));
    assert_eq!(row(&pool, "relaunch").await.status, "COMPLETED");
}

#[tokio::test]
async fn an_hls_download_resumes_at_the_first_segment_it_does_not_have() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let media = TempDir::new("hls-media-resume");
    let Some(files) = make_hls(media.path()) else {
        eprintln!("skipped: this ffmpeg cannot generate the test stream");
        return;
    };
    let playlist = String::from_utf8(files["index.m3u8"].clone()).unwrap();
    let seg0 = files["seg0.jpg"].clone();
    let (base, seen) = serve(move |req| match files.get(req.path.trim_start_matches("/hls/")) {
        Some(body) => Response::ok("application/octet-stream", body.clone()),
        None => Response::status(404),
    })
    .await;

    let tmp = TempDir::new("hls-resume");
    let rec = record("hls-resume");
    let pool = db_with(&rec).await;
    let url = format!("{}/hls/index.m3u8", base);

    // What a stopped run leaves: its playlist and the first segment.
    let plan = downloads::plan_hls_download(&playlist, &url).expect("the fixture is plannable");
    assert!(plan.files.len() >= 2, "{:?}", plan.files);
    let folder = work_file_path(tmp.path(), &rec, "hls");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("index.m3u8"), &plan.playlist).unwrap();
    std::fs::write(folder.join("source.txt"), downloads::hls_source_stamp(&plan)).unwrap();
    std::fs::write(folder.join(&plan.files[0].1), &seg0).unwrap();
    std::fs::write(folder.join(format!("{}.tmp", plan.files[1].1)), b"cut off").unwrap();

    let outcome = downloads::download_into(&pool, &rec, source(url), tmp.path()).await;

    let DownloadOutcome::Completed { path, .. } = outcome else { panic!("{:?}", outcome) };
    assert!(stream_kinds(&path).contains("Video:"));
    assert_eq!(tmp.file_names(), vec!["Test Show - EP 03 [SUB 720p].mkv"], "the segment folder is gone");
    let seen = seen.lock().unwrap();
    assert!(!seen.iter().any(|r| r.path == "/hls/seg0.jpg"), "a segment already saved is not fetched again");
    assert!(seen.iter().any(|r| r.path == "/hls/seg1.jpg"), "the cut-off segment is fetched again");
}

#[tokio::test]
async fn a_failed_download_is_posted_to_the_notification_centre() {
    let (base, _) = serve(|_| Response::status(403)).await;
    let tmp = TempDir::new("notice");
    let rec = record("notice");
    let pool = db_with(&rec).await;

    let outcome = downloads::download_into(&pool, &rec, source(format!("{}/v.mp4", base)), tmp.path()).await;

    assert!(matches!(outcome, DownloadOutcome::Failed(_)));
    let notices = anix_lib::notices::list(&pool, Some(anix_lib::notices::DOWNLOAD)).await.unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].title, "Download failed: Test Show, episode 3");
    assert!(notices[0].body.contains("403"), "{}", notices[0].body);
    assert_eq!(notices[0].link.as_deref(), Some("/downloads"));
}

// ---------------------------------------------------------------------------------------
// Running jobs, Try again and Remove
// ---------------------------------------------------------------------------------------

/// A host that sends the first 1.5 MB of a 50 MB file and then stalls with the connection open.
fn stalling() -> Response {
    Response {
        status: 200,
        content_type: "video/mp4",
        body: vec![1u8; 1_500_000],
        announced_len: Some(50_000_000),
        hang: true,
        extra: Vec::new(),
    }
}

#[tokio::test]
async fn a_running_download_is_never_started_twice_and_cancel_stops_it() {
    let (base, seen) = serve(|_| stalling()).await;
    let tmp = TempDir::new("twice");
    // The running-jobs registry is shared by the whole test binary, so the id is unique.
    let rec = record("running-twice");
    let pool = db_with(&rec).await;
    let url = format!("{}/v.mp4", base);

    downloads::start_download_in(pool.clone(), rec.clone(), source(url.clone()), tmp.path().to_path_buf());
    let mut started = false;
    for _ in 0..200 {
        if row(&pool, &rec.id).await.file_size.is_some() {
            started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(started, "the download never started: {:?}", tmp.file_names());

    // A second start for the same row (a re-queue racing the worker) is refused.
    downloads::start_download_in(pool.clone(), rec.clone(), source(url), tmp.path().to_path_buf());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(seen.lock().unwrap().len(), 1, "only one job asked for the stream");

    // Try again on a running row does nothing.
    assert_eq!(downloads::retry_download(&pool, &rec.id).await, Ok(false));
    assert_eq!(row(&pool, &rec.id).await.status, "DOWNLOADING");

    // Cancel (what Remove does) stops the one job and removes its partial files.
    assert!(downloads::cancel_download(&rec.id).await, "the job was registered and cancelled");
    assert!(tmp.file_names().is_empty(), "partial files must be gone: {:?}", tmp.file_names());
    assert!(!downloads::cancel_download(&rec.id).await, "nothing is left running");
}

#[tokio::test]
async fn try_again_requeues_only_a_failed_download() {
    let pool = db::init_db_in_memory().await.unwrap();
    for (id, status) in [("r-queued", "QUEUED"), ("r-running", "DOWNLOADING"), ("r-done", "COMPLETED"), ("r-failed", "FAILED")] {
        let mut rec = record(id);
        rec.status = status.to_string();
        rec.progress = 0.4;
        rec.error_msg = Some("HTTP 503".to_string());
        db::create_download(&pool, &rec).await.unwrap();
    }

    for id in ["r-queued", "r-running", "r-done", "no-such-row"] {
        assert_eq!(downloads::retry_download(&pool, id).await, Ok(false), "{} is left alone", id);
    }
    for (id, status) in [("r-queued", "QUEUED"), ("r-running", "DOWNLOADING"), ("r-done", "COMPLETED")] {
        let stored = row(&pool, id).await;
        assert_eq!(stored.status, status);
        assert_eq!(stored.progress, 0.4);
    }

    assert_eq!(downloads::retry_download(&pool, "r-failed").await, Ok(true));
    let stored = row(&pool, "r-failed").await;
    assert_eq!(stored.status, "QUEUED");
    assert_eq!(stored.progress, 0.0);
    assert_eq!(stored.error_msg, None);
    assert_eq!(downloads::retry_download(&pool, "r-failed").await, Ok(false), "a double click queues it once");
}

#[test]
fn only_network_failures_keep_what_was_fetched_for_a_retry() {
    for kept in [
        "Could not reach the stream host: error sending request for url",
        "The stream host did not answer in time.",
        "A segment of the stream was interrupted: connection reset",
        "The stream stalled: no data for 60 seconds.",
        "A segment of the stream is incomplete: received 10 bytes.",
        "The download was interrupted after 50000 bytes: error decoding response body",
        "The download stalled after 50000 bytes: no data for 60 seconds.",
        "The download is incomplete: received 1 of 2 bytes.",
        "A segment of the stream could not be fetched: HTTP 503 Service Unavailable.",
        "The stream host answered HTTP 502 Bad Gateway.",
        "The stream host answered HTTP 429 Too Many Requests.",
    ] {
        assert!(downloads::keeps_partial_files_after(kept), "kept: {}", kept);
    }
    for dropped in [
        "A segment of the stream could not be fetched: HTTP 403 Forbidden.",
        "The stream host answered HTTP 404 Not Found.",
        "The stream host returned a web page instead of a video.",
        "Could not write to /x/a.download.anix-part: No space left on device (os error 28)",
        "Could not create /x/a.hls.anix-part: Permission denied (os error 13)",
        "The download produced an empty file.",
        "ffmpeg failed (exit code 1): Invalid data found when processing input",
        "No playable stream was found for this episode.",
    ] {
        assert!(!downloads::keeps_partial_files_after(dropped), "dropped: {}", dropped);
    }
}

#[tokio::test]
async fn try_again_after_a_network_failure_resumes_where_it_stopped() {
    let body = test_body(300_000);
    let served = body.clone();
    let first = Arc::new(AtomicU32::new(0));
    let (base, seen) = serve(move |req| {
        if first.fetch_add(1, Ordering::SeqCst) == 0 {
            // The first try gets 120 kB, then the connection closes early.
            return Response { announced_len: Some(served.len()), ..Response::ok("video/mp4", served[..120_000].to_vec()) };
        }
        ranged(req, &served)
    })
    .await;
    let tmp = TempDir::new("retry-resume");
    let rec = record("retry-resume");
    let pool = db_with(&rec).await;
    let url = format!("{}/v.mp4", base);

    let outcome = downloads::download_into(&pool, &rec, source(url.clone()), tmp.path()).await;
    assert!(matches!(outcome, DownloadOutcome::Failed(_)), "{:?}", outcome);
    assert_eq!(std::fs::metadata(work_file_path(tmp.path(), &rec, "download")).unwrap().len(), 120_000);

    assert_eq!(downloads::retry_download(&pool, &rec.id).await, Ok(true));
    let outcome = downloads::download_into(&pool, &rec, source(url), tmp.path()).await;

    assert!(matches!(outcome, DownloadOutcome::Completed { .. }), "{:?}", outcome);
    assert_eq!(std::fs::read(tmp.path().join("Test Show - EP 03 [SUB 720p].mp4")).unwrap(), body);
    assert_eq!(tmp.file_names(), vec!["Test Show - EP 03 [SUB 720p].mp4"], "the kept files are gone after success");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].header("range"), Some("bytes=120000-"), "only the missing part is fetched again");
}

#[tokio::test]
async fn an_hls_download_that_failed_on_a_segment_keeps_the_others_for_try_again() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let media = TempDir::new("hls-media-retry");
    let Some(files) = make_hls(media.path()) else {
        eprintln!("skipped: this ffmpeg cannot generate the test stream");
        return;
    };
    let playlist = String::from_utf8(files["index.m3u8"].clone()).unwrap();
    let refused_once = Arc::new(AtomicU32::new(0));
    let (base, seen) = serve(move |req| {
        let name = req.path.trim_start_matches("/hls/");
        if name == "seg1.jpg" && refused_once.fetch_add(1, Ordering::SeqCst) == 0 {
            return Response::status(503);
        }
        match files.get(name) {
            Some(body) => Response::ok("application/octet-stream", body.clone()),
            None => Response::status(404),
        }
    })
    .await;
    let tmp = TempDir::new("hls-retry");
    let rec = record("hls-retry");
    let pool = db_with(&rec).await;
    let url = format!("{}/hls/index.m3u8", base);

    let outcome = downloads::download_into(&pool, &rec, source(url.clone()), tmp.path()).await;
    let DownloadOutcome::Failed(message) = outcome else { panic!("{:?}", outcome) };
    assert!(message.contains("503"), "{}", message);
    let plan = downloads::plan_hls_download(&playlist, &url).expect("the fixture is plannable");
    let folder = work_file_path(tmp.path(), &rec, "hls");
    assert!(folder.join(&plan.files[0].1).is_file(), "the segment fetched before the failure is kept");

    let outcome = downloads::download_into(&pool, &rec, source(url), tmp.path()).await;

    assert!(matches!(outcome, DownloadOutcome::Completed { .. }), "{:?}", outcome);
    assert_eq!(tmp.file_names(), vec!["Test Show - EP 03 [SUB 720p].mkv"], "the segment folder is gone after success");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.iter().filter(|r| r.path == "/hls/seg0.jpg").count(), 1, "seg0 is fetched once");
}

#[test]
fn removing_a_download_deletes_only_its_own_kept_files() {
    let tmp = TempDir::new("forget");
    let gone = record("forget-me");
    let mut other = record("someone-else");
    other.episode_num = 4.0;
    for rec in [&gone, &other] {
        for role in RESUME_ROLES {
            let path = work_file_path(tmp.path(), rec, role);
            if role == "hls" {
                std::fs::create_dir_all(&path).unwrap();
                std::fs::write(path.join("seg0.ts"), b"segment").unwrap();
            } else {
                std::fs::write(&path, b"partial").unwrap();
            }
        }
    }
    let finished = tmp.path().join("Test Show - EP 03 [SUB 720p].mkv");
    std::fs::write(&finished, b"a finished video").unwrap();

    assert_eq!(downloads::remove_resume_files(tmp.path(), &gone), RESUME_ROLES.len());

    assert!(finished.is_file(), "a finished video is never touched");
    for role in RESUME_ROLES {
        assert!(!work_file_path(tmp.path(), &gone, role).exists(), "{} of the removed row is gone", role);
        assert!(work_file_path(tmp.path(), &other, role).exists(), "{} of another row stays", role);
    }
}
