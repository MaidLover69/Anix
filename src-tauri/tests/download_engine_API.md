# #8 download engine: API the tests call (`anix_lib::downloads::engine`, new module, `pub mod engine;` in downloads/mod.rs)

Tests: `src-tauri/tests/download_engine_tests.rs` (+ `tests/engine_support/mod.rs`, a loopback tiny_http server).
Use `crate::clients::http` (shared client, `note_unreachable_host`, `doh_first`); no new dependencies.

## Types
- `FetchError { Connect(String), Timeout, Status { code: u16, retry_after: Option<Duration> }, Integrity(String), Io(String) }` (Debug): why a request failed.
- `RetryPolicy { max_tries: u32, base: Duration, jitter: bool }`: tries include the first; app default 4 tries, base 1 s, jitter on.
- `HostLimits` (`new(start, max)`, shared via `Arc`, interior mutability): per-host connection level.
- `SegmentOptions { headers: Vec<(String,String)>, retry: RetryPolicy, limits: Arc<HostLimits>, renew: Option<RenewFn> }`.
- `RenewFn = Arc<dyn Fn() -> RenewFuture + Send + Sync>`, `RenewFuture = Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + Send>>`: asks the extension again, returns the new segment URLs.
- `SegmentReport { fetched: usize, renewals: u32, .. }`.
- `DirectOptions { max_connections: usize, min_part_bytes: u64, retry: RetryPolicy, headers }` + `Default` (4, 16 MiB).
- `DirectReport { connections: usize, resumed_from: u64 }`.
- `Validator { etag: Option<String>, last_modified: Option<String> }`: stored beside a partial file.
- `SpeedMeter` (`Clone`, shared state, `new(window)`), `RateLimiter` (`new(bytes_per_sec)`, `unlimited()`).

## Functions
- `fetch_segment(&Client, url, &[(String,String)], &RetryPolicy) -> Result<Vec<u8>, FetchError>`: one segment with retry/backoff/Retry-After; runs `check_segment`; 403/404 returned at once as `Status`.
- `fetch_segments(&Client, &[String], dir: &Path, &SegmentOptions) -> Result<SegmentReport, FetchError>`: parallel (limit from `HostLimits`, never above 8), saves `segment_path(dir, i)`, skips ones already saved and valid, on 403 calls `renew` once and re-plans keeping saved segments; failed integrity leaves no file; 429/503/timeout call `HostLimits::record_pressure`, success `record_success`.
- `segment_path(dir, index) -> PathBuf`; `host_key(url) -> Option<String>` (host[:port]).
- `is_retryable(&FetchError) -> bool` (timeout, connect, 408, 429, 5xx); `backoff_delay(&RetryPolicy, attempt0, retry_after: Option<Duration>, jitter_ms: u64) -> Duration` (base * 2^attempt, at least Retry-After, plus jitter).
- `HostLimits`: `limit(host)`, `ceiling(host) -> Option<usize>`, `record_success(host)` (+1 per `SUCCESSES_PER_STEP`, cap max and ceiling), `record_pressure(host, started_limit)` (skipped when started above the current limit; halve, min 1, ceiling = refused level - 1); `const SUCCESSES_PER_STEP: u32` (tests loop on it).
- `check_segment(body, content_length: Option<u64>, content_type: Option<&str>) -> Result<(), String>`: rejects empty, size mismatch, HTML/JSON (sniffed or by content type), no TS `0x47` / fMP4 `ftyp|styp|moof` at bytes 4..8.
- `check_aes_key(&[u8]) -> Result<(), String>` (exactly 16 bytes); `verify_segments_before_mux(&[PathBuf]) -> Result<(), String>` (each present and valid; message names the index).
- `download_direct(&Client, url, target: &Path, &DirectOptions) -> Result<DirectReport, FetchError>`: probes with Range, splits into up to 4 parts of at least `min_part_bytes`, falls back to one connection when Range is ignored; resumes `partial_path(target)` with `Range: bytes=N-` plus `If-Range` from the stored validator (none stored = no If-Range, old 1.4.x partial files still resume); a 200 answer restarts from 0; on success moves the partial to `target` and removes the validator.
- `partial_path(target) -> PathBuf` (the existing `.part` name), `write_validator(target, &Validator) -> io::Result<()>`, `read_validator(target) -> Option<Validator>`.
- `SpeedMeter::record(bytes, at: Instant)`, `bytes_per_sec(at) -> f64` (sliding window, 0 when idle); `eta_seconds(remaining: u64, bps: f64) -> Option<u64>`; `RateLimiter::throttle(bytes).await` (async wait, about no burst).
- `parallel_downloads(&Value) -> usize` (`max_parallel_downloads`, default 1, clamp 1..=3); `speed_limit_bps(&Value) -> Option<u64>` (`download_speed_limit_kbps` x 1024; absent, 0 or negative = off).
- `doh_switch_decision(DohMode, &FetchError) -> bool`: true only in Automatic mode for `Connect` and `Timeout`.
