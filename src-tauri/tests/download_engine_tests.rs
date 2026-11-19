//! Issue #8 (download engine), RED step: written before the code. Every test names one item of
//! the issue; the coder builds `anix_lib::downloads::engine` (see `download_engine_API.md`) until
//! all of them pass. Each test starts its own loopback server; nothing here touches the network.

mod engine_support;

use anix_lib::clients::app_settings::DohMode;
use anix_lib::downloads::engine::{
    self, backoff_delay, check_aes_key, check_segment, doh_switch_decision, download_direct, fetch_segment,
    fetch_segments, host_key, is_retryable, parallel_downloads, segment_path, speed_limit_bps,
    verify_segments_before_mux, DirectOptions, FetchError, HostLimits, RateLimiter, RenewFn, RetryPolicy,
    SegmentOptions, SpeedMeter, Validator,
};
use engine_support::{fmp4_segment, ts_segment, Reply, TestServer};
use reqwest::Client;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-engine-{}-{}-{}",
            name,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn client() -> Client {
    Client::builder().build().unwrap()
}

fn fast_retry() -> RetryPolicy {
    RetryPolicy { max_tries: 4, base: Duration::from_millis(20), jitter: false }
}

fn options(limits: Arc<HostLimits>) -> SegmentOptions {
    SegmentOptions { headers: vec![], retry: fast_retry(), limits, renew: None }
}

fn segment_urls(server: &TestServer, prefix: &str, count: usize) -> Vec<String> {
    (0..count).map(|i| server.url(&format!("/{}/{}.ts", prefix, i))).collect()
}

fn index_of(path: &str) -> usize {
    path.trim_end_matches(".ts").rsplit('/').next().unwrap().parse().unwrap()
}

// ---- 1. Parallel HLS segments --------------------------------------------------------------

#[tokio::test]
async fn hls_segments_are_fetched_in_parallel_between_two_and_eight_at_once() {
    let server = TestServer::start(|r| Reply::ok(ts_segment(index_of(&r.path))).delay(60));
    let dir = TempDir::new("parallel");
    let urls = segment_urls(&server, "p", 32);

    let report = fetch_segments(&client(), &urls, &dir.0, &options(Arc::new(HostLimits::new(4, 8))))
        .await
        .expect("fetch");

    assert_eq!(report.fetched, 32);
    let peak = server.max_concurrent();
    assert!(peak > 1, "segments were fetched one at a time (peak {})", peak);
    assert!(peak <= 8, "more than 8 requests at once (peak {})", peak);
    for i in 0..32 {
        assert!(segment_path(&dir.0, i).exists(), "segment {} not saved", i);
    }
}

// ---- 2. Per-segment retry ------------------------------------------------------------------

#[tokio::test]
async fn one_503_then_ok_does_not_fail_the_episode_and_keeps_saved_segments() {
    let server = TestServer::start(|r| {
        if r.path.ends_with("/3.ts") && r.nth == 1 {
            return Reply::status(503);
        }
        Reply::ok(ts_segment(index_of(&r.path)))
    });
    let dir = TempDir::new("retry503");
    let urls = segment_urls(&server, "r", 8);

    let report = fetch_segments(&client(), &urls, &dir.0, &options(Arc::new(HostLimits::new(2, 8))))
        .await
        .expect("a single 503 must be retried");

    assert_eq!(report.fetched, 8);
    let reqs = server.requests();
    for i in [0usize, 1, 2, 4, 5, 6, 7] {
        let n = reqs.iter().filter(|r| r.path == format!("/r/{}.ts", i)).count();
        assert_eq!(n, 1, "segment {} was fetched {} times", i, n);
    }
    assert_eq!(reqs.iter().filter(|r| r.path == "/r/3.ts").count(), 2);
}

#[tokio::test]
async fn retry_after_header_is_honoured_on_429() {
    let server = TestServer::start(|r| {
        if r.nth == 1 {
            Reply::status(429).header("Retry-After", "1")
        } else {
            Reply::ok(ts_segment(0))
        }
    });
    let start = Instant::now();

    let body = fetch_segment(&client(), &server.url("/ra.ts"), &[], &fast_retry()).await.expect("fetch");

    assert!(start.elapsed() >= Duration::from_millis(950), "waited only {:?}", start.elapsed());
    assert_eq!(body, ts_segment(0));
}

#[tokio::test]
async fn forbidden_and_not_found_are_not_retried_and_report_an_expired_link() {
    for code in [403u16, 404] {
        let server = TestServer::start(move |_| Reply::status(code));
        let err = fetch_segment(&client(), &server.url("/gone.ts"), &[], &fast_retry())
            .await
            .expect_err("must fail");
        assert!(matches!(err, FetchError::Status { code: c, .. } if c == code), "{:?}", err);
        assert_eq!(server.requests().len(), 1, "{} was retried", code);
    }
}

#[tokio::test]
async fn gives_up_after_max_tries_on_a_host_that_keeps_failing() {
    let server = TestServer::start(|_| Reply::status(503));
    let policy = RetryPolicy { max_tries: 3, base: Duration::from_millis(10), jitter: false };

    let err = fetch_segment(&client(), &server.url("/down.ts"), &[], &policy).await.expect_err("fails");

    assert!(is_retryable(&err));
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn retryable_means_timeouts_connection_drops_408_429_and_5xx_only() {
    let status = |code| FetchError::Status { code, retry_after: None };
    for code in [408u16, 429, 500, 502, 503, 504] {
        assert!(is_retryable(&status(code)), "{}", code);
    }
    for code in [400u16, 401, 403, 404, 410] {
        assert!(!is_retryable(&status(code)), "{}", code);
    }
    assert!(is_retryable(&FetchError::Timeout));
    assert!(is_retryable(&FetchError::Connect("reset".into())));
}

#[test]
fn backoff_doubles_from_the_base_and_never_undercuts_retry_after() {
    let policy = RetryPolicy { max_tries: 5, base: Duration::from_secs(1), jitter: false };
    assert_eq!(backoff_delay(&policy, 0, None, 0), Duration::from_secs(1));
    assert_eq!(backoff_delay(&policy, 1, None, 0), Duration::from_secs(2));
    assert_eq!(backoff_delay(&policy, 2, None, 0), Duration::from_secs(4));
    assert_eq!(backoff_delay(&policy, 0, Some(Duration::from_secs(7)), 0), Duration::from_secs(7));
    // `jitter_ms` is added on top (the caller picks it at random, up to a few hundred ms).
    assert_eq!(backoff_delay(&policy, 1, None, 250), Duration::from_millis(2250));
}

// ---- 3. Adaptive connections per host ------------------------------------------------------

#[test]
fn host_limit_grows_one_step_after_a_run_of_successes_up_to_the_maximum() {
    let limits = HostLimits::new(2, 4);
    assert_eq!(limits.limit("h"), 2);
    for _ in 0..engine::SUCCESSES_PER_STEP {
        limits.record_success("h");
    }
    assert_eq!(limits.limit("h"), 3);
    for _ in 0..engine::SUCCESSES_PER_STEP * 10 {
        limits.record_success("h");
    }
    assert_eq!(limits.limit("h"), 4, "never above the maximum");
}

#[test]
fn host_limit_halves_on_pressure_and_never_climbs_back_past_the_refusal() {
    let limits = HostLimits::new(4, 8);
    for _ in 0..engine::SUCCESSES_PER_STEP {
        limits.record_success("h");
    }
    assert_eq!(limits.limit("h"), 5);

    limits.record_pressure("h"); // 429, 503 or a timeout at 5 connections

    assert_eq!(limits.limit("h"), 2, "halved (rounded down)");
    assert_eq!(limits.ceiling("h"), Some(4), "the host refused 5, so 4 is the most to try");
    for _ in 0..engine::SUCCESSES_PER_STEP * 20 {
        limits.record_success("h");
    }
    assert_eq!(limits.limit("h"), 4);
    // Other hosts are unaffected, and the limit never drops below one connection.
    assert_eq!(limits.limit("other"), 4);
    for _ in 0..10 {
        limits.record_pressure("h");
    }
    assert_eq!(limits.limit("h"), 1);
}

#[tokio::test]
async fn a_host_that_answers_429_above_three_connections_ends_at_three_or_fewer() {
    let server = TestServer::start(|r| {
        if r.active > 3 {
            return Reply::status(429);
        }
        Reply::ok(ts_segment(index_of(&r.path))).delay(25)
    });
    let dir = TempDir::new("adaptive");
    let urls = segment_urls(&server, "a", 60);
    let limits = Arc::new(HostLimits::new(6, 8));

    let report = fetch_segments(&client(), &urls, &dir.0, &options(limits.clone())).await.expect("fetch");

    assert_eq!(report.fetched, 60, "429 is retried, not fatal");
    let host = host_key(&urls[0]).expect("host");
    assert!(limits.limit(&host) <= 3, "limit stayed at {}", limits.limit(&host));
    assert!(limits.ceiling(&host).map_or(false, |c| c <= 3), "ceiling {:?}", limits.ceiling(&host));
}

// ---- 4. Link renewal -----------------------------------------------------------------------

#[tokio::test]
async fn a_403_mid_download_renews_the_playlist_and_saved_segments_are_not_fetched_again() {
    let server = TestServer::start(|r| {
        let index = index_of(&r.path);
        if r.path.starts_with("/old/") && index >= 4 {
            return Reply::status(403); // the signed link expired
        }
        Reply::ok(ts_segment(index)).delay(20)
    });
    let dir = TempDir::new("renew");
    let old = segment_urls(&server, "old", 10);
    let new = segment_urls(&server, "new", 10);
    let renewed = Arc::new(AtomicU32::new(0));
    let (counter, fresh) = (renewed.clone(), new.clone());
    let renew: RenewFn = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        let fresh = fresh.clone();
        Box::pin(async move { Ok(fresh) })
    });
    // One connection at a time: segments 0..3 are saved before segment 4 meets the 403.
    let mut opts = options(Arc::new(HostLimits::new(1, 1)));
    opts.renew = Some(renew);

    let report = fetch_segments(&client(), &old, &dir.0, &opts).await.expect("fetch with renewal");

    assert_eq!(renewed.load(Ordering::SeqCst), 1, "renew called once");
    assert_eq!(report.renewals, 1);
    for i in 0..10 {
        assert!(segment_path(&dir.0, i).exists(), "segment {} missing", i);
    }
    // Segments 0..3 were saved from the old link; the re-planned download must not fetch them again.
    let reqs = server.requests();
    for i in 0..4 {
        let again = reqs.iter().filter(|r| r.path == format!("/new/{}.ts", i)).count();
        assert_eq!(again, 0, "segment {} was saved before the renewal but fetched again", i);
    }
}

#[tokio::test]
async fn a_403_without_a_renew_callback_fails_with_the_expired_link_error() {
    let server = TestServer::start(|_| Reply::status(403));
    let dir = TempDir::new("norenew");
    let urls = segment_urls(&server, "x", 3);

    let err = fetch_segments(&client(), &urls, &dir.0, &options(Arc::new(HostLimits::new(2, 8))))
        .await
        .expect_err("fails");

    assert!(matches!(err, FetchError::Status { code: 403, .. }), "{:?}", err);
}

// ---- 5. Downloads at once and speed-limit settings -----------------------------------------

#[test]
fn parallel_downloads_defaults_to_one_and_is_clamped_to_one_through_three() {
    assert_eq!(parallel_downloads(&json!({})), 1);
    assert_eq!(parallel_downloads(&json!({ "max_parallel_downloads": 2 })), 2);
    assert_eq!(parallel_downloads(&json!({ "max_parallel_downloads": 3 })), 3);
    assert_eq!(parallel_downloads(&json!({ "max_parallel_downloads": 0 })), 1);
    assert_eq!(parallel_downloads(&json!({ "max_parallel_downloads": 9 })), 3);
    assert_eq!(parallel_downloads(&json!({ "max_parallel_downloads": "two" })), 1);
}

#[test]
fn speed_limit_is_off_by_default_and_read_in_kilobytes_per_second() {
    assert_eq!(speed_limit_bps(&json!({})), None);
    assert_eq!(speed_limit_bps(&json!({ "download_speed_limit_kbps": 0 })), None);
    assert_eq!(speed_limit_bps(&json!({ "download_speed_limit_kbps": 500 })), Some(500 * 1024));
    assert_eq!(speed_limit_bps(&json!({ "download_speed_limit_kbps": -5 })), None);
}

// ---- 6. Integrity checks -------------------------------------------------------------------

#[test]
fn accepts_ts_and_fmp4_segments() {
    assert!(check_segment(&ts_segment(1), Some(188 * 6), Some("video/mp2t")).is_ok());
    assert!(check_segment(&ts_segment(1), None, None).is_ok());
    assert!(check_segment(&fmp4_segment(), None, Some("video/mp4")).is_ok());
    for kind in [b"ftyp", b"styp", b"moof"] {
        let mut data = vec![0, 0, 0, 20];
        data.extend_from_slice(kind);
        data.extend_from_slice(&[0u8; 40]);
        assert!(check_segment(&data, None, None).is_ok(), "{:?}", kind);
    }
}

#[test]
fn rejects_html_and_json_bodies_in_place_of_a_segment() {
    assert!(check_segment(b"<!DOCTYPE html><html>Access denied</html>", None, None).is_err());
    assert!(check_segment(b"  <html><body>blocked</body></html>", None, None).is_err());
    assert!(check_segment(br#"{"error":"token expired"}"#, None, None).is_err());
    assert!(check_segment(br#"[{"error":1}]"#, None, None).is_err());
    // A text/html or JSON content type is enough, even when the bytes look like TS.
    assert!(check_segment(&ts_segment(1), None, Some("text/html; charset=utf-8")).is_err());
    assert!(check_segment(&ts_segment(1), None, Some("application/json")).is_err());
}

#[test]
fn rejects_a_body_whose_size_differs_from_content_length() {
    let body = ts_segment(2);
    assert!(check_segment(&body, Some(body.len() as u64 + 1), None).is_err());
    assert!(check_segment(&body, Some(body.len() as u64 - 1), None).is_err());
}

#[test]
fn rejects_a_segment_with_neither_ts_sync_byte_nor_an_fmp4_box() {
    assert!(check_segment(&[0u8; 400], None, None).is_err());
    assert!(check_segment(b"hello this is not video at all, sorry", None, None).is_err());
    assert!(check_segment(&[], None, None).is_err(), "an empty segment is not a segment");
}

#[test]
fn an_aes_key_must_be_exactly_sixteen_bytes() {
    assert!(check_aes_key(&[7u8; 16]).is_ok());
    assert!(check_aes_key(&[7u8; 15]).is_err());
    assert!(check_aes_key(&[7u8; 32]).is_err());
    assert!(check_aes_key(b"<html>forbidden</html>").is_err());
    assert!(check_aes_key(&[]).is_err());
}

#[tokio::test]
async fn an_html_error_page_served_with_200_is_refused_and_nothing_is_saved() {
    let server = TestServer::start(|_| Reply::ok(b"<html>captcha</html>".to_vec()).header("Content-Type", "text/html"));
    let dir = TempDir::new("html");
    let urls = segment_urls(&server, "h", 3);

    let err = fetch_segments(&client(), &urls, &dir.0, &options(Arc::new(HostLimits::new(2, 8))))
        .await
        .expect_err("must refuse");

    assert!(matches!(err, FetchError::Integrity(_)), "{:?}", err);
    for i in 0..3 {
        assert!(!segment_path(&dir.0, i).exists(), "bad segment {} was kept", i);
    }
}

#[test]
fn every_segment_is_checked_before_the_mux() {
    let dir = TempDir::new("verify");
    let paths: Vec<PathBuf> = (0..3).map(|i| segment_path(&dir.0, i)).collect();
    for (i, p) in paths.iter().enumerate() {
        std::fs::write(p, ts_segment(i)).unwrap();
    }
    assert!(verify_segments_before_mux(&paths).is_ok());

    std::fs::write(&paths[1], b"<html>oops</html>").unwrap();
    let err = verify_segments_before_mux(&paths).expect_err("a bad segment blocks the mux");
    assert!(err.contains('1'), "the message names the segment: {}", err);

    std::fs::write(&paths[1], ts_segment(1)).unwrap();
    std::fs::remove_file(&paths[2]).unwrap();
    assert!(verify_segments_before_mux(&paths).is_err(), "a missing segment blocks the mux");
}

// ---- 7. Byte ranges for big direct files ---------------------------------------------------

fn big_file(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// A server that honours `Range: bytes=a-b` (and `bytes=a-`) over `data`.
fn range_server(data: Vec<u8>, etag: &'static str) -> TestServer {
    TestServer::start(move |r| {
        let total = data.len();
        let Some(range) = r.range.as_deref().and_then(|v| v.strip_prefix("bytes=")) else {
            return Reply::ok(data.clone()).header("Accept-Ranges", "bytes").header("ETag", etag);
        };
        let (a, b) = range.split_once('-').unwrap();
        let start: usize = a.parse().unwrap();
        let end: usize = b.parse().map(|e: usize| e.min(total - 1)).unwrap_or(total - 1);
        Reply {
            status: 206,
            headers: vec![
                ("Content-Range".into(), format!("bytes {}-{}/{}", start, end, total)),
                ("Accept-Ranges".into(), "bytes".into()),
                ("ETag".into(), etag.into()),
            ],
            body: data[start..=end].to_vec(),
            delay_ms: 0,
        }
    })
}

fn small_parts() -> DirectOptions {
    DirectOptions { max_connections: 4, min_part_bytes: 256 * 1024, retry: fast_retry(), headers: vec![] }
}

#[test]
fn direct_options_default_to_four_connections_of_at_least_16_mb() {
    let d = DirectOptions::default();
    assert_eq!(d.max_connections, 4);
    assert_eq!(d.min_part_bytes, 16 * 1024 * 1024);
}

#[tokio::test]
async fn a_big_direct_file_is_split_into_several_range_requests_and_reassembled() {
    let data = big_file(1024 * 1024);
    let server = range_server(data.clone(), "\"v1\"");
    let dir = TempDir::new("ranges");
    let target = dir.0.join("movie.mp4");

    let report = download_direct(&client(), &server.url("/f.mp4"), &target, &small_parts()).await.expect("download");

    assert_eq!(std::fs::read(&target).unwrap(), data);
    assert!(report.connections >= 2 && report.connections <= 4, "{} connections", report.connections);
    let ranges: std::collections::HashSet<String> =
        server.requests().into_iter().filter_map(|r| r.range).collect();
    assert!(ranges.len() >= 2, "expected several different Range requests, got {:?}", ranges);
}

#[tokio::test]
async fn a_server_that_ignores_range_gets_one_connection_and_a_complete_file() {
    let data = big_file(600 * 1024);
    let copy = data.clone();
    let server = TestServer::start(move |_| Reply::ok(copy.clone())); // never 206, no Accept-Ranges
    let dir = TempDir::new("noranges");
    let target = dir.0.join("movie.mp4");

    let report = download_direct(&client(), &server.url("/f.mp4"), &target, &small_parts()).await.expect("download");

    assert_eq!(std::fs::read(&target).unwrap(), data);
    assert_eq!(report.connections, 1);
}

#[tokio::test]
async fn a_file_smaller_than_two_parts_uses_one_connection() {
    let data = big_file(100 * 1024);
    let server = range_server(data.clone(), "\"v1\"");
    let dir = TempDir::new("small");
    let target = dir.0.join("small.mp4");

    let report = download_direct(&client(), &server.url("/s.mp4"), &target, &small_parts()).await.expect("download");

    assert_eq!(std::fs::read(&target).unwrap(), data);
    assert_eq!(report.connections, 1);
}

// ---- 8. Safer resume -----------------------------------------------------------------------

fn partial_path(target: &std::path::Path) -> PathBuf {
    engine::partial_path(target)
}

#[tokio::test]
async fn resume_sends_range_and_if_range_with_the_stored_etag_and_appends_on_206() {
    let data = big_file(300 * 1024);
    let server = range_server(data.clone(), "\"v1\"");
    let dir = TempDir::new("resume");
    let target = dir.0.join("ep.mp4");
    std::fs::write(partial_path(&target), &data[..100_000]).unwrap();
    engine::write_validator(&target, &Validator { etag: Some("\"v1\"".into()), last_modified: None }).unwrap();

    let report = download_direct(&client(), &server.url("/ep.mp4"), &target, &small_parts()).await.expect("resume");

    assert_eq!(std::fs::read(&target).unwrap(), data);
    assert_eq!(report.resumed_from, 100_000);
    let resumed = server.requests().into_iter().find(|r| r.range.as_deref() == Some("bytes=100000-")).expect("range request");
    assert_eq!(resumed.if_range.as_deref(), Some("\"v1\""));
}

#[tokio::test]
async fn a_changed_file_on_the_server_restarts_from_zero_instead_of_gluing_two_versions() {
    let new_data = big_file(300 * 1024).into_iter().map(|b| b.wrapping_add(1)).collect::<Vec<u8>>();
    let copy = new_data.clone();
    // A real server answers 200 with the whole new file when If-Range does not match.
    let server = TestServer::start(move |_| Reply::ok(copy.clone()).header("ETag", "\"v2\""));
    let dir = TempDir::new("changed");
    let target = dir.0.join("ep.mp4");
    std::fs::write(partial_path(&target), vec![9u8; 100_000]).unwrap();
    engine::write_validator(&target, &Validator { etag: Some("\"v1\"".into()), last_modified: None }).unwrap();

    let report = download_direct(&client(), &server.url("/ep.mp4"), &target, &small_parts()).await.expect("restart");

    assert_eq!(std::fs::read(&target).unwrap(), new_data);
    assert_eq!(report.resumed_from, 0);
}

#[tokio::test]
async fn last_modified_is_used_when_there_is_no_etag() {
    let data = big_file(200 * 1024);
    let server = range_server(data.clone(), "\"unused\"");
    let dir = TempDir::new("lastmod");
    let target = dir.0.join("ep.mp4");
    std::fs::write(partial_path(&target), &data[..50_000]).unwrap();
    let stamp = "Wed, 21 Oct 2026 07:28:00 GMT";
    engine::write_validator(&target, &Validator { etag: None, last_modified: Some(stamp.into()) }).unwrap();

    download_direct(&client(), &server.url("/ep.mp4"), &target, &small_parts()).await.expect("resume");

    let resumed = server.requests().into_iter().find(|r| r.range.as_deref() == Some("bytes=50000-")).expect("range");
    assert_eq!(resumed.if_range.as_deref(), Some(stamp));
}

#[tokio::test]
async fn a_partial_file_from_1_4_x_with_no_stored_validator_still_resumes() {
    let data = big_file(300 * 1024);
    let server = range_server(data.clone(), "\"v1\"");
    let dir = TempDir::new("legacy");
    let target = dir.0.join("ep.mp4");
    std::fs::write(partial_path(&target), &data[..120_000]).unwrap(); // no validator file beside it

    let report = download_direct(&client(), &server.url("/ep.mp4"), &target, &small_parts()).await.expect("resume");

    assert_eq!(std::fs::read(&target).unwrap(), data);
    assert_eq!(report.resumed_from, 120_000);
    let resumed = server.requests().into_iter().find(|r| r.range.as_deref() == Some("bytes=120000-")).expect("range");
    assert_eq!(resumed.if_range, None, "nothing to validate against, so no If-Range");
}

#[tokio::test]
async fn the_validator_is_stored_beside_the_partial_file_when_a_download_starts() {
    let data = big_file(100 * 1024);
    let slow = data.clone();
    let server = TestServer::start(move |_| Reply::ok(slow.clone()).header("ETag", "\"abc\"").delay(0));
    let dir = TempDir::new("store");
    let target = dir.0.join("ep.mp4");

    download_direct(&client(), &server.url("/ep.mp4"), &target, &small_parts()).await.expect("download");

    // After completion the sidecar is gone with the partial file; the read API reports no validator.
    assert!(engine::read_validator(&target).is_none());
    assert!(!partial_path(&target).exists());
}

// ---- 9. Speed, time left, speed limit ------------------------------------------------------

#[test]
fn speed_meter_sums_every_parallel_request_over_a_sliding_window() {
    let meter = SpeedMeter::new(Duration::from_secs(4));
    let t0 = Instant::now();
    let a = meter.clone();
    let b = meter.clone(); // two connections feeding one meter
    for s in 0..4u64 {
        a.record(50_000, t0 + Duration::from_secs(s));
        b.record(50_000, t0 + Duration::from_secs(s));
    }
    let bps = meter.bytes_per_sec(t0 + Duration::from_secs(4));
    assert!((90_000.0..=110_000.0).contains(&bps), "expected about 100 kB/s, got {}", bps);
    // Idle for longer than the window: the speed falls to zero.
    assert_eq!(meter.bytes_per_sec(t0 + Duration::from_secs(60)), 0.0);
}

#[test]
fn time_left_is_remaining_bytes_over_speed_and_unknown_when_stalled() {
    assert_eq!(engine::eta_seconds(1_000_000, 100_000.0), Some(10));
    assert_eq!(engine::eta_seconds(0, 100_000.0), Some(0));
    assert_eq!(engine::eta_seconds(1_000_000, 0.0), None);
}

#[tokio::test]
async fn the_rate_limiter_keeps_the_measured_rate_under_the_limit() {
    let limit = 200_000u64; // bytes per second
    let limiter = RateLimiter::new(limit);
    let start = Instant::now();
    for _ in 0..6 {
        limiter.throttle(50_000).await; // 300 kB in total
    }
    let secs = start.elapsed().as_secs_f64();
    let rate = 300_000.0 / secs;
    assert!(secs >= 1.2, "finished in {:.2}s, so the limit was ignored", secs);
    assert!(rate <= limit as f64 * 1.25, "measured {:.0} B/s against a {} B/s limit", rate, limit);
}

#[tokio::test]
async fn no_limit_means_no_waiting() {
    let limiter = RateLimiter::unlimited();
    let start = Instant::now();
    limiter.throttle(500_000_000).await;
    assert!(start.elapsed() < Duration::from_millis(100));
}

// ---- 10. DoH fallback decision (no network) ------------------------------------------------

#[test]
fn automatic_mode_switches_a_host_to_doh_only_when_it_cannot_be_connected_to() {
    let status = |code| FetchError::Status { code, retry_after: None };
    assert!(doh_switch_decision(DohMode::Auto, &FetchError::Connect("connection refused".into())));
    assert!(doh_switch_decision(DohMode::Auto, &FetchError::Timeout));
    // The host answered, so DNS is not the problem.
    assert!(!doh_switch_decision(DohMode::Auto, &status(403)));
    assert!(!doh_switch_decision(DohMode::Auto, &status(503)));
    assert!(!doh_switch_decision(DohMode::Auto, &FetchError::Integrity("html".into())));
}

#[test]
fn doh_on_and_off_modes_never_trigger_the_automatic_switch() {
    let refused = FetchError::Connect("connection refused".into());
    assert!(!doh_switch_decision(DohMode::On, &refused), "On already uses DoH for everything");
    assert!(!doh_switch_decision(DohMode::Off, &refused), "Off must stay off");
}
