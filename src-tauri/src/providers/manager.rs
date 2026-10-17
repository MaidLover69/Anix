use crate::cache::L1Cache;
use crate::clients::anilist::{fetch_media_hint, MediaHint};
use crate::clients::http::{http_client, no_redirect_client};
use crate::error::AppError;
use crate::models::{AnimeQuery, Category, EpisodeItem, ProviderData, SourcesResult, StreamItem};
use crate::providers::common::{fmt_ep, is_hls_url, with_session_allowance, SESSION_LOOKUP_ALLOWANCE};
use crate::providers::languages::{plan_tiers, prefer_subtitles, served_language, LanguagePrefs, Languages, Tier};
use crate::providers::AnimeProvider;
use crate::web_session::{detect_barrier, SessionUse};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, LOCATION, ORIGIN, RANGE, REFERER};
use reqwest::{Client, Url};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::sync::OnceCell;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;

/// Limit for one provider's episode listing.
const EPISODES_TIMEOUT: Duration = Duration::from_secs(10);
/// Limit for one provider's whole attempt at a play request: episode lookup, source
/// resolution and stream verification.
const SOURCES_TIMEOUT: Duration = Duration::from_secs(18);
/// Limit for checking a single stream, and for each request made while checking it.
const VERIFY_STREAM_TIMEOUT: Duration = Duration::from_secs(7);
const VERIFY_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// How many of a provider's streams are checked (concurrently) before giving up.
const MAX_VERIFIED_STREAMS: usize = 8;
const MAX_PLAYLIST_BYTES: usize = 8 * 1024 * 1024;

const HINT_TIMEOUT: Duration = Duration::from_millis(2500);

const EPISODES_TTL_SECS: u64 = 15 * 60;
/// An empty listing is retried sooner than a real one expires.
const EMPTY_EPISODES_TTL: Duration = Duration::from_secs(120);
/// A cached listing that lacks the requested episode is refreshed once it is this old.
const STALE_LISTING_AGE: Duration = Duration::from_secs(60);

/// A site that could not be connected to is left alone for this long, doubling per consecutive
/// failure up to the maximum.
const COOLDOWN_BASE: Duration = Duration::from_secs(20);
const COOLDOWN_MAX: Duration = Duration::from_secs(120);

/// Request headers a stream must be fetched with, one value per header name. Custom headers win
/// over `referer`/`origin`; the Origin falls back to the Referer's origin.
pub fn stream_header_map(stream: &StreamItem) -> HeaderMap {
    let mut map = HeaderMap::new();
    if let Some(ref headers) = stream.headers {
        for (name, value) in headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                map.insert(name, value);
            }
        }
    }

    if !map.contains_key(REFERER) {
        if let Some(value) = stream
            .referer
            .as_deref()
            .and_then(|r| HeaderValue::from_str(r).ok())
        {
            map.insert(REFERER, value);
        }
    }

    if !map.contains_key(ORIGIN) {
        let origin = stream.origin.clone().or_else(|| {
            let referer = map.get(REFERER)?.to_str().ok()?;
            let parsed = Url::parse(referer).ok()?;
            parsed.host_str()?;
            Some(parsed.origin().ascii_serialization())
        });
        if let Some(value) = origin.and_then(|o| HeaderValue::from_str(&o).ok()) {
            map.insert(ORIGIN, value);
        }
    }
    map
}

pub fn apply_stream_headers(req: reqwest::RequestBuilder, stream: &StreamItem) -> reqwest::RequestBuilder {
    req.headers(stream_header_map(stream))
}

/// First entry of a playlist that is not a tag or a comment.
pub fn first_playlist_uri(playlist: &str) -> Option<&str> {
    playlist
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

/// Resolves a playlist entry against the playlist's own URL: absolute, protocol-relative,
/// root-relative and path-relative entries all follow the usual URL rules.
pub fn resolve_playlist_uri(playlist_url: &str, entry: &str) -> Option<String> {
    let base = Url::parse(playlist_url).ok()?;
    base.join(entry.trim()).ok().map(|u| u.to_string())
}

fn host_of(response: &reqwest::Response) -> String {
    response.url().host_str().unwrap_or("unknown host").to_string()
}

fn transport_reason(err: reqwest::Error) -> String {
    AppError::from(err).reason()
}

/// How much of an answer is read to tell what it is.
const SNIFF_BYTES: usize = 32 * 1024;

/// The start of an answer's body, as far as it arrives within the request's time limit.
async fn body_start(response: &mut reqwest::Response, limit: usize) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    while bytes.len() < limit {
        match response.chunk().await {
            Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
            _ => break,
        }
    }
    bytes.truncate(limit);
    bytes
}

fn lowered_headers(response: &reqwest::Response) -> HashMap<String, String> {
    response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

/// True for an answer whose body is a web page rather than media. The declared type alone does
/// not decide: some hosts send media under a text type.
pub fn looks_like_web_page(content_type: &str, body: &[u8]) -> bool {
    let body = body.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(body);
    let start = body.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(body.len());
    let head: Vec<u8> = body[start..].iter().take(15).map(u8::to_ascii_lowercase).collect();
    if head.starts_with(b"<!doctype html") || head.starts_with(b"<html") {
        return true;
    }
    let declared = content_type.trim().to_ascii_lowercase();
    (declared.starts_with("text/html") || declared.starts_with("application/xhtml")) && head.starts_with(b"<")
}

/// Why an answer cannot be the media that was asked for, or `None` when nothing speaks against
/// it. An anti-bot page is named as such, whatever status it came with.
async fn media_refusal(response: &mut reqwest::Response) -> Option<String> {
    let status = response.status();
    let host = host_of(response);
    let headers = lowered_headers(response);
    let content_type = headers.get("content-type").cloned().unwrap_or_default();
    let mitigated = headers.contains_key("cf-mitigated");
    let textual = content_type.to_ascii_lowercase().starts_with("text/");
    if status.is_success() && !mitigated && !textual {
        // Media: only its first bytes are looked at, for a page sent under a media type.
        let start = body_start(response, 512).await;
        return looks_like_web_page(&content_type, &start)
            .then(|| format!("{} answered with a web page instead of a video", host));
    }
    let start = body_start(response, SNIFF_BYTES).await;
    let text = String::from_utf8_lossy(&start);
    if detect_barrier(status.as_u16(), &headers, &text).is_some() {
        return Some(format!("{} answered with its anti-bot check", host));
    }
    if !status.is_success() {
        return Some(format!("HTTP {} from {}", status.as_u16(), host));
    }
    looks_like_web_page(&content_type, &start)
        .then(|| format!("{} answered with a web page instead of a video", host))
}

async fn fetch_playlist(client: &Client, url: &Url, stream: &StreamItem) -> Result<String, String> {
    let request = apply_stream_headers(client.get(url.clone()), stream).timeout(VERIFY_REQUEST_TIMEOUT);
    let mut response = request.send().await.map_err(transport_reason)?;
    let host = host_of(&response);
    let headers = lowered_headers(&response);
    let status = response.status();
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_reason)? {
        if bytes.len() + chunk.len() > MAX_PLAYLIST_BYTES {
            return Err(format!("{} answered with something too large to be a playlist", host));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if detect_barrier(status.as_u16(), &headers, head_of_text(&text)).is_some() {
        return Err(format!("{} answered with its anti-bot check", host));
    }
    if !status.is_success() {
        return Err(format!("HTTP {} from {}", status.as_u16(), host));
    }
    if !text.contains("#EXTM3U") {
        let content_type = headers.get("content-type").map(String::as_str).unwrap_or_default();
        return Err(if looks_like_web_page(content_type, &bytes) {
            format!("{} answered with a web page instead of a playlist", host)
        } else {
            "not an HLS playlist".to_string()
        });
    }
    Ok(text)
}

fn head_of_text(text: &str) -> &str {
    let mut end = text.len().min(SNIFF_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

async fn check_hls(stream: &StreamItem, url: Url) -> Result<(), String> {
    let client = http_client().map_err(|e| e.reason())?;

    let playlist = fetch_playlist(&client, &url, stream).await?;
    let first = first_playlist_uri(&playlist).ok_or("empty playlist")?;
    let mut target = url.join(first).map_err(|_| "playlist entry is not a URL")?;

    let is_master = playlist.contains("#EXT-X-STREAM-INF") || target.path().ends_with(".m3u8");
    if is_master {
        let media_playlist = fetch_playlist(&client, &target, stream).await?;
        let segment = first_playlist_uri(&media_playlist).ok_or("empty media playlist")?;
        target = target.join(segment).map_err(|_| "segment entry is not a URL")?;
    }

    let request = apply_stream_headers(client.get(target).header(RANGE, "bytes=0-2047"), stream)
        .timeout(VERIFY_REQUEST_TIMEOUT);
    let mut response = request.send().await.map_err(transport_reason)?;
    match media_refusal(&mut response).await {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

async fn check_file(stream: &StreamItem, url: Url) -> Result<(), String> {
    // Redirects are followed by hand so the stream's own headers go out on every hop.
    let client = no_redirect_client().map_err(|e| e.reason())?;
    let mut current = url;
    for _ in 0..6 {
        let request = apply_stream_headers(client.get(current.clone()).header(RANGE, "bytes=0-2047"), stream)
            .timeout(VERIFY_REQUEST_TIMEOUT);
        let mut response = request.send().await.map_err(transport_reason)?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|h| h.to_str().ok())
                .ok_or("redirect without a Location header")?;
            current = current.join(location).map_err(|_| "redirect to an invalid URL")?;
            continue;
        }
        return match media_refusal(&mut response).await {
            Some(reason) => Err(reason),
            None => Ok(()),
        };
    }
    Err("too many redirects".to_string())
}

async fn check_stream(stream: &StreamItem) -> Result<(), String> {
    let url = Url::parse(&stream.url).map_err(|_| "invalid stream URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("unsupported URL scheme".to_string());
    }
    let is_file = stream.stream_type.eq_ignore_ascii_case("mp4")
        || (!is_hls_url(&stream.url) && url.path().to_ascii_lowercase().contains(".mp4"));
    let check = async {
        if is_file {
            check_file(stream, url).await
        } else {
            check_hls(stream, url).await
        }
    };
    match timeout(VERIFY_STREAM_TIMEOUT, check).await {
        Ok(result) => result,
        Err(_) => Err("timed out".to_string()),
    }
}

/// True when the stream's playlist and first segment (or the file's first bytes) can be fetched
/// with the headers the stream asks for.
pub async fn verify_stream_segment(stream: &StreamItem) -> bool {
    check_stream(stream).await.is_ok()
}

/// Checks the streams concurrently and returns as soon as one is confirmed playable: that
/// stream first, then the others in their original order as unverified fallbacks. When none
/// passes, the error is the reason the first-choice stream failed.
pub async fn verify_streams(streams: Vec<StreamItem>) -> Result<Vec<StreamItem>, String> {
    if streams.is_empty() {
        return Err("no streams".to_string());
    }

    let mut order: Vec<usize> = (0..streams.len()).collect();
    order.sort_by_key(|&i| !streams[i].is_active);
    order.truncate(MAX_VERIFIED_STREAMS);

    let shared = Arc::new(streams);
    let mut checks = JoinSet::new();
    for &idx in &order {
        let shared = shared.clone();
        checks.spawn(async move { (idx, check_stream(&shared[idx]).await) });
    }

    let mut winner: Option<usize> = None;
    let mut reasons: HashMap<usize, String> = HashMap::new();
    while let Some(joined) = checks.join_next().await {
        match joined {
            Ok((idx, Ok(()))) => {
                winner = Some(idx);
                break;
            }
            Ok((idx, Err(reason))) => {
                reasons.insert(idx, reason);
            }
            Err(join_err) => tracing::error!("stream check task failed: {}", join_err),
        }
    }
    drop(checks);

    let streams = Arc::try_unwrap(shared).unwrap_or_else(|still_shared| (*still_shared).clone());
    match winner {
        Some(idx) => {
            let mut ordered = Vec::with_capacity(streams.len());
            let mut rest = Vec::with_capacity(streams.len());
            for (i, stream) in streams.into_iter().enumerate() {
                if i == idx {
                    ordered.push(stream);
                } else {
                    rest.push(stream);
                }
            }
            ordered.extend(rest);
            Ok(ordered)
        }
        None => Err(order
            .first()
            .and_then(|idx| reasons.remove(idx))
            .unwrap_or_else(|| "no stream could be checked".to_string())),
    }
}

/// Like `verify_streams`, but the caller's order is a preference: all streams are checked at
/// once and the earliest one that passes is put first, even when a later one answered sooner.
pub async fn verify_streams_in_order(streams: Vec<StreamItem>) -> Result<Vec<StreamItem>, String> {
    if streams.is_empty() {
        return Err("no streams".to_string());
    }
    let shared = Arc::new(streams);
    let checks: Vec<AbortOnDrop<Result<(), String>>> = (0..shared.len().min(MAX_VERIFIED_STREAMS))
        .map(|idx| {
            let shared = shared.clone();
            AbortOnDrop(tokio::spawn(async move { check_stream(&shared[idx]).await }))
        })
        .collect();

    let mut winner: Option<usize> = None;
    let mut first_reason: Option<String> = None;
    for (idx, check) in checks.into_iter().enumerate() {
        match check.await {
            Ok(Ok(())) => {
                winner = Some(idx);
                break;
            }
            Ok(Err(reason)) => {
                first_reason.get_or_insert(reason);
            }
            Err(join_err) => tracing::error!("stream check task failed: {}", join_err),
        }
    }
    let Some(winner) = winner else {
        return Err(first_reason.unwrap_or_else(|| "no stream could be checked".to_string()));
    };
    let mut streams = Arc::try_unwrap(shared).unwrap_or_else(|still_shared| (*still_shared).clone());
    let chosen = streams.remove(winner);
    streams.insert(0, chosen);
    Ok(streams)
}

/// Joins per-provider failure reasons into one line, listing providers that failed for the same
/// reason together: `"Miruro, AniZone: timed out; AniBD: HTTP 403 from example.org"`.
pub fn summarize_failures(failures: &[(String, String)]) -> String {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for (name, reason) in failures {
        match groups.iter_mut().find(|(r, _)| *r == reason.as_str()) {
            Some((_, names)) => names.push(name.as_str()),
            None => groups.push((reason.as_str(), vec![name.as_str()])),
        }
    }
    groups
        .iter()
        .map(|(reason, names)| format!("{}: {}", names.join(", "), reason))
        .collect::<Vec<_>>()
        .join("; ")
}

/// A spawned task that is cancelled when this handle is dropped.
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl<T> Future for AbortOnDrop<T> {
    type Output = Result<T, tokio::task::JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

#[derive(Clone)]
struct CachedEpisodes {
    data: Arc<ProviderData>,
    fetched_at: Instant,
}

struct Cooldown {
    until: Instant,
    strikes: u32,
}

struct ProviderFailure {
    index: usize,
    name: &'static str,
    reason: String,
    /// The site refused the plain client and the provider can ask through the in-app browser,
    /// which this attempt was not allowed to start.
    session_could_help: bool,
}

type SharedQuery = Arc<OnceCell<Arc<AnimeQuery>>>;

/// The providers one lookup works with, in priority order.
type Lineup = Arc<Vec<Arc<dyn AnimeProvider>>>;

/// Where the manager's providers come from: a fixed list, or a list asked for at the start of
/// every lookup (installed plugins, which come and go while the app runs).
pub type ProviderSource = Arc<dyn Fn() -> Vec<Arc<dyn AnimeProvider>> + Send + Sync>;

/// The user's language preferences, read at the start of every lookup.
pub type LanguageSource = Arc<dyn Fn() -> LanguagePrefs + Send + Sync>;

/// What to ask a provider for to answer `category`, or `None` when it has no such version. A
/// site whose only version is a dub serves it as its plain version.
fn asked_category(provider: &dyn AnimeProvider, category: &Category) -> Option<Category> {
    match category {
        Category::Sub => Some(Category::Sub),
        Category::Dub if provider.supports_dub() => Some(Category::Dub),
        Category::Dub if provider.languages().map_or(false, |l| l.dub_only(false)) => Some(Category::Sub),
        Category::Dub => None,
    }
}

/// Whether the provider's `category` list is offered under the preferences: always when
/// other languages are allowed, for the provider picked in settings and for providers that do
/// not say their languages; otherwise only when it serves a preferred language.
fn offers_preferred(provider: &dyn AnimeProvider, category: &Category, prefs: &LanguagePrefs) -> bool {
    if !prefs.strict || prefs.pinned.as_deref().map_or(false, |p| p.eq_ignore_ascii_case(provider.name())) {
        return true;
    }
    let Some(langs) = provider.languages() else {
        return true;
    };
    asked_category(provider, category).is_some()
        && langs
            .for_category(category)
            .iter()
            .any(|code| prefs.for_category(category).contains(code))
}

/// A provider's listing as it is offered: a dub-only site's episodes are its dub, and with
/// preferences, lists in languages the user did not ask for are left out.
fn shape_listing(provider: &dyn AnimeProvider, mut data: ProviderData, prefs: Option<&LanguagePrefs>) -> ProviderData {
    if provider.languages().map_or(false, |l| l.dub_only(provider.supports_dub())) && data.dub.is_empty() {
        data.dub = std::mem::take(&mut data.sub);
    }
    if let Some(prefs) = prefs {
        if !offers_preferred(provider, &Category::Sub, prefs) {
            data.sub.clear();
        }
        if !offers_preferred(provider, &Category::Dub, prefs) {
            data.dub.clear();
        }
    }
    data
}

fn no_language_error(prefs: &LanguagePrefs, category: &Category) -> AppError {
    AppError::SourcesUnavailable(format!(
        "No enabled plugin offers {}. Choose other languages, or allow plugins in other languages, in Settings › Sources.",
        prefs.describe(category)
    ))
}

/// What a lookup fails with when there is no provider at all.
pub const NO_PLUGIN_MESSAGE: &str =
    "No streaming plugin is installed or enabled. Add or enable one in Settings → Plugins.";

struct Inner {
    source: ProviderSource,
    /// `get_episodes` results per (provider cache key, AniList id).
    episodes: L1Cache<(String, i64), CachedEpisodes>,
    hints: L1Cache<i64, Arc<MediaHint>>,
    cooldowns: Mutex<HashMap<&'static str, Cooldown>>,
    /// Whether titles, year and episode count are looked up on AniList for title matching.
    resolve_hints: bool,
    /// Without it, every provider is asked whatever its languages.
    languages: Option<LanguageSource>,
}

impl Inner {
    fn prefs(&self) -> Option<LanguagePrefs> {
        self.languages.as_ref().map(|source| source())
    }

    fn lineup(&self) -> Lineup {
        Arc::new((self.source)())
    }

    fn is_cooling_down(&self, site: &'static str) -> bool {
        let guard = self.cooldowns.lock().unwrap_or_else(|p| p.into_inner());
        guard.get(site).map_or(false, |c| Instant::now() < c.until)
    }

    fn note_unreachable(&self, site: &'static str) {
        let mut guard = self.cooldowns.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let entry = guard.entry(site).or_insert(Cooldown { until: now, strikes: 0 });
        // Providers of one site fail together; count that as a single strike.
        if now < entry.until {
            return;
        }
        entry.strikes = (entry.strikes + 1).min(8);
        let length = (COOLDOWN_BASE * 2u32.pow(entry.strikes - 1)).min(COOLDOWN_MAX);
        entry.until = now + length;
    }

    fn note_reachable(&self, site: &'static str) {
        let mut guard = self.cooldowns.lock().unwrap_or_else(|p| p.into_inner());
        guard.remove(site);
    }

    fn note_outcome<T>(&self, site: &'static str, result: &Result<T, AppError>) {
        match result {
            Err(e) if e.is_connection_failure() => self.note_unreachable(site),
            _ => self.note_reachable(site),
        }
    }

    async fn enrich_query(&self, base: &AnimeQuery) -> AnimeQuery {
        let mut query = base.clone();
        if !self.resolve_hints || base.anilist_id <= 0 {
            return query;
        }
        let id = base.anilist_id;
        let lookup = self
            .hints
            .try_get_with(id, async move {
                let client = http_client()?;
                match timeout(HINT_TIMEOUT, fetch_media_hint(&client, id)).await {
                    Ok(result) => result.map(Arc::new),
                    Err(_) => Err(AppError::Network("AniList timed out".to_string())),
                }
            })
            .await;
        match lookup {
            Ok(hint) => {
                for title in &hint.titles {
                    query.add_title(title);
                }
                query.mal_id = query.mal_id.or(hint.id_mal);
                query.year = hint.year;
                query.episodes = hint.episodes;
                query.format = hint.format.clone();
                query.english = hint.english.clone();
            }
            Err(e) => tracing::warn!(
                "AniList lookup for {} failed ({}); matching by the given title only",
                id,
                e.reason()
            ),
        }
        query
    }

    async fn full_query(&self, base: &AnimeQuery, cell: &SharedQuery) -> Arc<AnimeQuery> {
        cell.get_or_init(|| async { Arc::new(self.enrich_query(base).await) })
            .await
            .clone()
    }

    /// The provider's episode listing, from cache when present. `refresh_if_older` forces a new
    /// fetch when the cached listing is older than the given age.
    async fn episodes(
        &self,
        provider: &Arc<dyn AnimeProvider>,
        query: &AnimeQuery,
        refresh_if_older: Option<Duration>,
        access: SessionUse,
    ) -> Result<CachedEpisodes, AppError> {
        let key = (provider.cache_key(), query.anilist_id);
        if let Some(hit) = self.episodes.get(&key).await {
            let age = hit.fetched_at.elapsed();
            let expired = hit.data.is_empty() && age > EMPTY_EPISODES_TTL;
            let too_old = refresh_if_older.map_or(false, |limit| age > limit);
            if !expired && !too_old {
                return Ok(hit);
            }
            self.episodes.invalidate(&key).await;
        }

        let provider = provider.clone();
        self.episodes
            .try_get_with(key, async move {
                let data = provider.get_episodes_with(query, access).await?;
                Ok::<_, AppError>(CachedEpisodes {
                    data: Arc::new(data),
                    fetched_at: Instant::now(),
                })
            })
            .await
            .map_err(|shared| (*shared).clone())
    }

    async fn find_episode(
        &self,
        provider: &Arc<dyn AnimeProvider>,
        query: &AnimeQuery,
        number: f64,
        category: &Category,
        access: SessionUse,
    ) -> Result<EpisodeItem, AppError> {
        let name = provider.name();
        let pick = |data: &ProviderData| -> Option<EpisodeItem> {
            let list = match category {
                Category::Sub => &data.sub,
                Category::Dub => &data.dub,
            };
            list.iter().find(|ep| (ep.number - number).abs() < 0.01).cloned()
        };

        let mut cached = self.episodes(provider, query, None, access).await?;
        if pick(&cached.data).is_none() && cached.fetched_at.elapsed() > STALE_LISTING_AGE {
            cached = self.episodes(provider, query, Some(STALE_LISTING_AGE), access).await?;
        }
        pick(&cached.data).ok_or_else(|| {
            let message = if cached.data.is_empty() {
                "anime not found".to_string()
            } else if *category == Category::Dub && cached.data.dub.is_empty() {
                "no dub".to_string()
            } else {
                format!("episode {} not listed", fmt_ep(number))
            };
            AppError::provider(name, message)
        })
    }

    async fn try_provider(
        self: Arc<Self>,
        provider: Arc<dyn AnimeProvider>,
        base: Arc<AnimeQuery>,
        cell: SharedQuery,
        number: f64,
        category: Category,
        access: SessionUse,
    ) -> Result<SourcesResult, AppError> {
        let name = provider.name();
        let Some(asked) = asked_category(provider.as_ref(), &category) else {
            return Err(AppError::provider(name, "no dub"));
        };

        let episode = match provider.episode_by_number(&base, number) {
            Some(episode) => episode,
            None => {
                let query = self.full_query(&base, &cell).await;
                let found = self.find_episode(&provider, &query, number, &asked, access).await;
                if matches!(&found, Err(e) if e.is_connection_failure()) {
                    self.note_unreachable(provider.site());
                }
                found?
            }
        };

        let sources = provider.get_sources_with(&episode, asked.clone(), access).await;
        self.note_outcome(provider.site(), &sources);
        let mut sources = sources?;
        if sources.streams.is_empty() {
            return Err(AppError::NoSourcesFound);
        }
        if asked != category {
            for stream in &mut sources.streams {
                stream.audio = Some(category.as_str().to_string());
            }
        }

        if !provider.verifies_streams() {
            sources.streams = verify_streams(sources.streams)
                .await
                .map_err(|reason| AppError::provider(name, format!("stream check failed: {}", reason)))?;
        }
        sources.provider = Some(name.to_string());
        Ok(sources)
    }

    /// Runs the given providers against each other. The first verified result wins and every
    /// other attempt is cancelled; failures are appended with their reasons. `access` says
    /// whether a provider may start an in-app browser session for its site.
    async fn race(
        self: &Arc<Self>,
        lineup: &Lineup,
        wave: &[usize],
        base: &Arc<AnimeQuery>,
        cell: &SharedQuery,
        number: f64,
        category: &Category,
        access: SessionUse,
        failures: &mut Vec<ProviderFailure>,
    ) -> Option<SourcesResult> {
        let mut attempts = JoinSet::new();
        for &idx in wave {
            let inner = self.clone();
            let provider = lineup[idx].clone();
            let (base, cell, category) = (base.clone(), cell.clone(), category.clone());
            attempts.spawn(async move {
                let name = provider.name();
                let attempt = AbortOnDrop(tokio::spawn(
                    inner.clone().try_provider(provider.clone(), base, cell, number, category, access),
                ));
                // Time spent starting an in-app browser session does not count against the
                // provider; the other providers of the race are not held up by it.
                let limited = with_session_allowance(attempt, SOURCES_TIMEOUT, SESSION_LOOKUP_ALLOWANCE, |since| {
                    provider.session_time_since(since)
                })
                .await;
                let outcome = match limited.ok_or(()) {
                    Err(_) => Err(("timed out".to_string(), false)),
                    Ok(Err(join_err)) => {
                        tracing::error!("provider {} task failed: {}", name, join_err);
                        Err(("internal error".to_string(), false))
                    }
                    Ok(Ok(Ok(sources))) => Ok(sources),
                    Ok(Ok(Err(err))) => {
                        let refused = matches!(
                            err,
                            AppError::CaptchaRequired | AppError::VerificationRequired { .. }
                        );
                        Err((err.reason(), refused))
                    }
                };
                (idx, outcome)
            });
        }

        while let Some(joined) = attempts.join_next().await {
            match joined {
                Ok((_, Ok(sources))) => {
                    attempts.abort_all();
                    return Some(sources);
                }
                Ok((idx, Err((reason, refused)))) => {
                    let provider = &lineup[idx];
                    let name = provider.name();
                    tracing::warn!("provider {} gave no source: {}", name, reason);
                    failures.push(ProviderFailure {
                        index: idx,
                        name,
                        reason,
                        session_could_help: refused
                            && access == SessionUse::Existing
                            && provider.may_use_session(),
                    });
                }
                Err(join_err) => tracing::error!("provider attempt task failed: {}", join_err),
            }
        }
        None
    }
}

pub struct ProviderManager {
    inner: Arc<Inner>,
}

impl Default for ProviderManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderManager {
    /// A manager without providers; see `with_source` for the one the app uses.
    pub fn new() -> Self {
        Self::build(Arc::new(Vec::new), true)
    }

    /// A manager over the given providers that never consults AniList, for tests and tools.
    pub fn with_providers(providers: Vec<Arc<dyn AnimeProvider>>) -> Self {
        Self::build(Arc::new(move || providers.clone()), false)
    }

    /// A manager whose providers are asked for at the start of every lookup, so a change in
    /// the set (a plugin installed, enabled or disabled) applies to the next one.
    pub fn with_source(source: ProviderSource) -> Self {
        Self::build(source, true)
    }

    /// `with_source` without the AniList lookup, for tests.
    pub fn with_source_offline(source: ProviderSource) -> Self {
        Self::build(source, false)
    }

    /// `with_source`, picking providers by the user's languages: see `get_sources_for_episode`.
    pub fn with_source_and_languages(source: ProviderSource, languages: LanguageSource) -> Self {
        Self::build_with(source, true, Some(languages))
    }

    /// `with_providers` under fixed language preferences, for tests.
    pub fn with_providers_and_languages(providers: Vec<Arc<dyn AnimeProvider>>, prefs: LanguagePrefs) -> Self {
        Self::build_with(
            Arc::new(move || providers.clone()),
            false,
            Some(Arc::new(move || prefs.clone())),
        )
    }

    fn build(source: ProviderSource, resolve_hints: bool) -> Self {
        Self::build_with(source, resolve_hints, None)
    }

    fn build_with(source: ProviderSource, resolve_hints: bool, languages: Option<LanguageSource>) -> Self {
        Self {
            inner: Arc::new(Inner {
                source,
                episodes: L1Cache::new(2000, EPISODES_TTL_SECS),
                hints: L1Cache::new(500, 3600),
                cooldowns: Mutex::new(HashMap::new()),
                resolve_hints,
                languages,
            }),
        }
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.inner.lineup().iter().map(|p| p.name()).collect()
    }

    /// Names of the providers served by the site with this key (see `AnimeProvider::site`).
    pub fn providers_for_site(&self, site: &str) -> Vec<&'static str> {
        self.inner
            .lineup()
            .iter()
            .filter(|p| p.site() == site)
            .map(|p| p.name())
            .collect()
    }

    /// True for providers that are asked by episode number and return no episode list.
    pub fn is_number_addressed(&self, name: &str) -> bool {
        let probe = AnimeQuery::new(1, None, None);
        self.inner
            .lineup()
            .iter()
            .find(|p| p.name().eq_ignore_ascii_case(name))
            .map_or(false, |p| p.episode_by_number(&probe, 1.0).is_some())
    }

    /// Episode listings of every provider that answered, in provider order. Each provider has
    /// its own time limit, so one slow site costs at most that; providers that failed or timed
    /// out are left out and logged. Listing never loads a site in the in-app browser that was not
    /// reached through it before, and a session that is slow to start is not waited for beyond
    /// the provider's limit (it keeps starting and answers the next listing).
    ///
    /// Under language preferences only providers that serve a preferred language are asked
    /// (unless other languages are allowed), and each listing keeps only the versions (sub,
    /// dub) in the preferred languages.
    pub async fn get_all_episodes(
        &self,
        anilist_id: i64,
        mal_id: Option<i64>,
        title_romaji: Option<String>,
    ) -> Result<Vec<ProviderData>, AppError> {
        let inner = &self.inner;
        let lineup = inner.lineup();
        let base = Arc::new(AnimeQuery::new(anilist_id, mal_id, title_romaji.as_deref()));
        let cell: SharedQuery = Arc::new(OnceCell::new());
        let prefs = inner.prefs();

        let mut results: Vec<(usize, ProviderData)> = Vec::new();
        let mut listings = JoinSet::new();
        for (idx, provider) in lineup.iter().enumerate() {
            if let Some(prefs) = &prefs {
                let wanted = offers_preferred(provider.as_ref(), &Category::Sub, prefs)
                    || offers_preferred(provider.as_ref(), &Category::Dub, prefs);
                if !wanted {
                    continue;
                }
            }
            if provider.episode_by_number(&base, 1.0).is_some() {
                results.push((idx, ProviderData::empty(provider.name())));
                continue;
            }
            if inner.is_cooling_down(provider.site()) {
                tracing::warn!(
                    "provider {} skipped for the episode list: site was unreachable moments ago",
                    provider.name()
                );
                continue;
            }
            let (inner, base, cell, provider) = (inner.clone(), base.clone(), cell.clone(), provider.clone());
            listings.spawn(async move {
                let site = provider.site();
                let listing = AbortOnDrop(tokio::spawn({
                    let inner = inner.clone();
                    async move {
                        let query = inner.full_query(&base, &cell).await;
                        inner.episodes(&provider, &query, None, SessionUse::Existing).await
                    }
                }));
                let outcome = match timeout(EPISODES_TIMEOUT, listing).await {
                    Err(_) => Err("timed out".to_string()),
                    Ok(Err(join_err)) => {
                        tracing::error!("episode listing task failed: {}", join_err);
                        Err("internal error".to_string())
                    }
                    Ok(Ok(result)) => {
                        inner.note_outcome(site, &result);
                        result.map_err(|e| e.reason())
                    }
                };
                (idx, outcome)
            });
        }

        while let Some(joined) = listings.join_next().await {
            match joined {
                Ok((idx, Ok(cached))) => {
                    let data = shape_listing(lineup[idx].as_ref(), (*cached.data).clone(), prefs.as_ref());
                    results.push((idx, data));
                }
                Ok((idx, Err(reason))) => tracing::warn!(
                    "provider {} returned no episode list: {}",
                    lineup[idx].name(),
                    reason
                ),
                Err(join_err) => tracing::error!("episode listing task failed: {}", join_err),
            }
        }

        results.sort_by_key(|(idx, _)| *idx);
        Ok(results.into_iter().map(|(_, data)| data).collect())
    }

    /// Finds a playable source for one episode. All candidate providers run at once and the
    /// first to produce a verified stream wins, wherever it sits in the list; the rest are
    /// cancelled. `preferred_provider` (anything but `None`, `""` or `"auto"`) restricts the
    /// attempt to that provider. On failure the error names the reason per provider.
    ///
    /// Under language preferences, an automatic lookup races only the providers serving the
    /// most wanted language, then those serving the next one, and so on (see `plan_tiers`);
    /// providers in other languages come last, and only when the preferences allow them. The
    /// result says which language it is in, and its subtitle tracks in the preferred languages
    /// come first.
    ///
    /// A provider whose site refuses the plain client may load that site in a hidden in-app
    /// browser only when it was asked for by name, or as a last resort once every provider has
    /// failed; a session that answers, or answered in an earlier run, is used either way. The
    /// time a provider's requests wait for a session to start is added to its limit.
    pub async fn get_sources_for_episode(
        &self,
        anilist_id: i64,
        mal_id: Option<i64>,
        episode_number: f64,
        category_str: &str,
        title_romaji: Option<String>,
        preferred_provider: Option<&str>,
    ) -> Result<SourcesResult, AppError> {
        let inner = &self.inner;
        let lineup = inner.lineup();
        if lineup.is_empty() {
            return Err(AppError::SourcesUnavailable(NO_PLUGIN_MESSAGE.to_string()));
        }
        let category = Category::from(category_str);
        let base = Arc::new(AnimeQuery::new(anilist_id, mal_id, title_romaji.as_deref()));
        let cell: SharedQuery = Arc::new(OnceCell::new());

        let preferred = preferred_provider
            .map(str::trim)
            .filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("auto"));
        let prefs = inner.prefs();

        // A provider asked for by name is asked whatever its languages. Otherwise the
        // preferred languages are tried in order, each in a race of its own.
        let tiers: Vec<Tier> = match (preferred, &prefs) {
            (Some(wanted), _) => {
                let idx = lineup
                    .iter()
                    .position(|p| p.name() == wanted)
                    .or_else(|| lineup.iter().position(|p| p.name().eq_ignore_ascii_case(wanted)))
                    .ok_or_else(|| {
                        AppError::SourcesUnavailable(format!(
                            "No source found. Unknown provider '{}'",
                            wanted
                        ))
                    })?;
                vec![Tier { language: None, members: vec![idx] }]
            }
            (None, None) => vec![Tier { language: None, members: (0..lineup.len()).collect() }],
            (None, Some(prefs)) => {
                let described: Vec<(Option<Languages>, bool)> = lineup
                    .iter()
                    .map(|p| (p.languages(), asked_category(p.as_ref(), &category).is_some()))
                    .collect();
                let tiers = plan_tiers(&described, &category, prefs);
                if tiers.is_empty() {
                    return Err(no_language_error(prefs, &category));
                }
                tiers
            }
        };
        let left_out = tiers.iter().map(|t| t.members.len()).sum::<usize>() < lineup.len();

        let finish = |mut sources: SourcesResult| -> SourcesResult {
            let Some(prefs) = &prefs else {
                return sources;
            };
            let langs = sources
                .provider
                .as_deref()
                .and_then(|name| lineup.iter().find(|p| p.name() == name))
                .and_then(|p| p.languages());
            sources.language = served_language(langs.as_ref(), &category, prefs.for_category(&category));
            // Subtitle files, also offered with a dub, follow the subtitle preference.
            prefer_subtitles(&mut sources.subtitles, &prefs.sub);
            sources
        };

        let access = match preferred {
            Some(_) => SessionUse::Create,
            None => SessionUse::Existing,
        };
        let mut failures: Vec<ProviderFailure> = Vec::new();
        for tier in &tiers {
            // Sites that just proved unreachable go last within their tier, but are still
            // tried when nothing else in it delivers, so a retry never runs out of providers.
            let (first_wave, second_wave): (Vec<usize>, Vec<usize>) = match preferred {
                Some(_) => (tier.members.clone(), Vec::new()),
                None => tier
                    .members
                    .iter()
                    .partition(|&&idx| !inner.is_cooling_down(lineup[idx].site())),
            };
            for wave in [first_wave, second_wave] {
                if wave.is_empty() {
                    continue;
                }
                if let Some(sources) = inner
                    .race(&lineup, &wave, &base, &cell, episode_number, &category, access, &mut failures)
                    .await
                {
                    return Ok(finish(sources));
                }
            }
        }

        // Nothing delivered: the providers that were only held back from their site's in-app
        // browser session get to use it now, tier by tier.
        for tier in &tiers {
            let held_back = |f: &ProviderFailure| f.session_could_help && tier.members.contains(&f.index);
            let last_resort: Vec<usize> = failures.iter().filter(|f| held_back(f)).map(|f| f.index).collect();
            if last_resort.is_empty() {
                continue;
            }
            failures.retain(|f| !held_back(f));
            let allowed = SessionUse::Create;
            if let Some(sources) = inner
                .race(&lineup, &last_resort, &base, &cell, episode_number, &category, allowed, &mut failures)
                .await
            {
                return Ok(finish(sources));
            }
        }

        failures.sort_by_key(|f| f.index);
        let listed: Vec<(String, String)> = failures
            .into_iter()
            .map(|f| (f.name.to_string(), f.reason))
            .collect();
        let mut message = format!("No source found. {}", summarize_failures(&listed));
        if let (None, Some(prefs), true) = (preferred, &prefs, left_out) {
            message.push_str(&format!(
                ". Only plugins with {} were asked; Settings › Sources can allow other languages.",
                prefs.describe(&category)
            ));
        }
        tracing::warn!(
            "no source for anime {} episode {} ({}): {}",
            anilist_id,
            fmt_ep(episode_number),
            category.as_str(),
            message
        );
        Err(AppError::SourcesUnavailable(message))
    }
}
