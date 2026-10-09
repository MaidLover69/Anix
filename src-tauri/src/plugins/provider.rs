//! A loaded plugin as an `AnimeProvider`, so the provider manager's racing, time budgets, stream
//! verification, caching and failure reporting apply to plugins unchanged.
//!
//! The plugin side is Seanime's protocol: `search` → `findEpisodes` → `findEpisodeServer`. What
//! the app adds around it is choosing the search hit by title, year and episode count, trying
//! the plugin's servers side by side and checking that a stream really plays.
//!
//! Optional fields a plugin may add for this app (Seanime ignores them):
//! - search result: `altTitles?: string[]`, `year?: number`, `episodeCount?: number`, more
//!   facts for matching;
//! - episode: `image?: string`, `synopsis?: string`, `filler?: boolean`;
//! - subtitle: `format?: "vtt" | "srt" | "ass" | "ssa"`, for URLs whose extension misleads;
//! - episode server: `skip: { introStart, introEnd, outroStart, outroEnd }`, in seconds; a
//!   window counts when both of its ends are given.

use crate::error::AppError;
use crate::models::{
    AnimeQuery, Category, EpisodeItem, ProviderData, SkipTimes, SourcesResult, StreamItem, SubtitleItem,
};
use crate::plugins::engine::{PluginError, PluginRuntime};
use crate::plugins::intern;
use crate::providers::common::{fix_url, fmt_ep, is_hls_url, json_id, json_number, subtitle_format};
use crate::providers::manager::{summarize_failures, verify_streams_in_order};
use crate::providers::matching::{pick_best, Candidate};
use crate::providers::AnimeProvider;
use crate::web_session::SessionUse;
use async_trait::async_trait;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;

const SETTINGS_TIMEOUT: Duration = Duration::from_secs(5);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(9);
const EPISODES_TIMEOUT: Duration = Duration::from_secs(9);
const SERVER_TIMEOUT: Duration = Duration::from_secs(14);
/// Servers of one plugin asked at the same time.
const SERVER_CONCURRENCY: usize = 3;
/// Titles a search is repeated with before the anime counts as not found.
const MAX_SEARCH_TITLES: usize = 3;
/// The server name used when a plugin lists none.
pub const DEFAULT_SERVER: &str = "default";

/// What `getSettings()` returned.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProviderSettings {
    pub episode_servers: Vec<String>,
    pub supports_dub: bool,
}

impl ProviderSettings {
    pub fn from_json(value: &Value) -> Self {
        let mut episode_servers: Vec<String> = Vec::new();
        if let Some(list) = value.get("episodeServers").and_then(Value::as_array) {
            for name in list.iter().filter_map(Value::as_str).map(str::trim) {
                if !name.is_empty() && !episode_servers.iter().any(|known| known == name) {
                    episode_servers.push(name.to_string());
                }
            }
        }
        Self {
            episode_servers,
            supports_dub: value.get("supportsDub").and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

/// Asks a plugin for its settings; an object is required.
pub async fn read_settings(runtime: &PluginRuntime) -> Result<ProviderSettings, String> {
    let value = runtime
        .call("getSettings", &[], SETTINGS_TIMEOUT, SessionUse::Existing)
        .await
        .map_err(|e| format!("getSettings failed: {}", e.reason()))?;
    if !value.is_object() {
        return Err("getSettings did not return an object".to_string());
    }
    Ok(ProviderSettings::from_json(&value))
}

/// The servers to ask for a category. Plugins that tell sub and dub apart by server name
/// ("GG-SUB", "GG-DUB") get only the matching ones; names without such a marker always count.
pub fn servers_for(servers: &[String], category: &Category) -> Vec<String> {
    let says = |name: &str, word: &str| {
        name.to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|part| part == word || part == format!("{}bed", word))
    };
    let (wanted, other) = match category {
        Category::Sub => ("sub", "dub"),
        Category::Dub => ("dub", "sub"),
    };
    let matching: Vec<String> = servers.iter().filter(|name| says(name, wanted)).cloned().collect();
    let chosen = if !matching.is_empty() {
        let neutral = servers.iter().filter(|name| !says(name, wanted) && !says(name, other)).cloned();
        matching.into_iter().chain(neutral).collect()
    } else {
        let without_other: Vec<String> = servers.iter().filter(|name| !says(name, other)).cloned().collect();
        if without_other.is_empty() {
            servers.to_vec()
        } else {
            without_other
        }
    };
    if chosen.is_empty() {
        return vec![DEFAULT_SERVER.to_string()];
    }
    // A plugin that lists "default" picks its own best server for it; that goes first.
    let (mut ordered, rest): (Vec<String>, Vec<String>) = chosen
        .into_iter()
        .partition(|name| name.eq_ignore_ascii_case(DEFAULT_SERVER));
    ordered.extend(rest);
    ordered
}

#[derive(Debug, Clone)]
struct SearchHit {
    id: String,
    audio: String,
}

fn text_of(value: &Value) -> Option<String> {
    value.as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from)
}

fn parse_search(value: &Value) -> Result<Vec<Candidate<SearchHit>>, String> {
    let Some(list) = value.as_array() else {
        return Err("search did not return a list".to_string());
    };
    let mut candidates = Vec::new();
    for entry in list {
        let (Some(id), Some(title)) = (entry.get("id").and_then(json_id), entry.get("title").and_then(text_of)) else {
            continue;
        };
        let mut titles = vec![title];
        if let Some(more) = entry.get("altTitles").and_then(Value::as_array) {
            for title in more.iter().filter_map(text_of) {
                if !titles.contains(&title) {
                    titles.push(title);
                }
            }
        }
        let number = |name: &str| {
            entry
                .get(name)
                .and_then(json_number)
                .filter(|n| n.is_finite() && *n > 0.0)
                .map(|n| n as i32)
        };
        candidates.push(Candidate {
            item: SearchHit {
                id,
                audio: entry
                    .get("subOrDub")
                    .and_then(Value::as_str)
                    .unwrap_or("sub")
                    .to_ascii_lowercase(),
            },
            titles,
            year: number("year"),
            episodes: number("episodeCount"),
        });
    }
    Ok(candidates)
}

fn parse_episodes(value: &Value) -> Result<Vec<EpisodeItem>, String> {
    let Some(list) = value.as_array() else {
        return Err("findEpisodes did not return a list".to_string());
    };
    let mut episodes: Vec<EpisodeItem> = Vec::new();
    for entry in list {
        let Some(id) = entry.get("id").and_then(json_id) else {
            continue;
        };
        let Some(number) = entry
            .get("number")
            .and_then(json_number)
            .filter(|n| n.is_finite() && *n >= 0.0)
        else {
            continue;
        };
        if episodes.iter().any(|known| (known.number - number).abs() < 0.001) {
            continue;
        }
        let title = entry.get("title").and_then(text_of);
        let url = entry.get("url").and_then(text_of).unwrap_or_default();
        let mut details = Map::new();
        details.insert("id".to_string(), Value::String(id));
        details.insert("number".to_string(), entry.get("number").cloned().unwrap_or(json!(number)));
        details.insert("url".to_string(), Value::String(url));
        if let Some(title) = &title {
            details.insert("title".to_string(), Value::String(title.clone()));
        }
        episodes.push(EpisodeItem {
            pipe_id: Value::Object(details).to_string(),
            number,
            title: title.or_else(|| Some(format!("Episode {}", fmt_ep(number)))),
            image: entry.get("image").and_then(text_of),
            synopsis: entry.get("synopsis").and_then(text_of),
            filler: entry.get("filler").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    if episodes.is_empty() && !list.is_empty() {
        return Err("findEpisodes returned no usable episode (each needs an id and a number)".to_string());
    }
    episodes.sort_by(|a, b| a.number.partial_cmp(&b.number).unwrap_or(std::cmp::Ordering::Equal));
    Ok(episodes)
}

fn parse_skip(server: &Value) -> Option<SkipTimes> {
    let skip = server.get("skip")?;
    let window = |start: &str, end: &str| -> (Option<f64>, Option<f64>) {
        let read = |name: &str| skip.get(name).and_then(json_number).filter(|n| n.is_finite() && *n >= 0.0);
        match (read(start), read(end)) {
            (Some(start), Some(end)) if end > start => (Some(start), Some(end)),
            _ => (None, None),
        }
    };
    let (intro_start, intro_end) = window("introStart", "introEnd");
    let (outro_start, outro_end) = window("outroStart", "outroEnd");
    if intro_start.is_none() && outro_start.is_none() {
        return None;
    }
    Some(SkipTimes {
        intro_start,
        intro_end,
        outro_start,
        outro_end,
    })
}

/// One server's answer: its streams, each with the subtitles that belong to it.
#[derive(Debug, Clone)]
struct ServerSources {
    streams: Vec<StreamItem>,
    subtitles: HashMap<String, Vec<SubtitleItem>>,
    skip: Option<SkipTimes>,
}

fn parse_server(value: &Value, server: &str, category: &Category) -> Result<ServerSources, String> {
    if !value.is_object() {
        return Err(format!("findEpisodeServer returned no result for {}", server));
    }
    let mut headers: HashMap<String, String> = HashMap::new();
    if let Some(given) = value.get("headers").and_then(Value::as_object) {
        for (name, header) in given {
            let text = match header {
                Value::String(text) => text.trim().to_string(),
                Value::Number(number) => number.to_string(),
                _ => continue,
            };
            if !name.trim().is_empty() && !text.is_empty() {
                headers.insert(name.trim().to_string(), text);
            }
        }
    }
    let header = |wanted: &str| {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.clone())
    };
    let (referer, origin) = (header("referer"), header("origin"));

    let mut streams: Vec<StreamItem> = Vec::new();
    let mut subtitles: HashMap<String, Vec<SubtitleItem>> = HashMap::new();
    for source in value.get("videoSources").and_then(Value::as_array).into_iter().flatten() {
        let Some(url) = source.get("url").and_then(text_of).map(|u| fix_url(&u)) else {
            continue;
        };
        let parsed = reqwest::Url::parse(&url);
        if !parsed.map_or(false, |u| matches!(u.scheme(), "http" | "https")) {
            continue;
        }
        if streams.iter().any(|known| known.url == url) {
            continue;
        }
        let stream_type = match source.get("type").and_then(Value::as_str).unwrap_or_default() {
            "m3u8" | "hls" => "hls",
            "mp4" => "mp4",
            _ if is_hls_url(&url) => "hls",
            _ => "mp4",
        };
        let quality = source.get("quality").and_then(text_of).unwrap_or_else(|| "auto".to_string());
        let label = source.get("label").and_then(text_of);
        let subtitle_variant = label.as_deref().and_then(|label| {
            let label = label.to_ascii_lowercase();
            if label.contains("soft") {
                Some("s-sub".to_string())
            } else if label.contains("hard") {
                Some("h-sub".to_string())
            } else {
                None
            }
        });
        let quality = match label {
            Some(label) if !quality.to_lowercase().contains(&label.to_lowercase()) => {
                format!("{} - {}", quality, label)
            }
            _ => quality,
        };

        let mut tracks: Vec<SubtitleItem> = Vec::new();
        for track in source.get("subtitles").and_then(Value::as_array).into_iter().flatten() {
            let Some(track_url) = track.get("url").and_then(text_of).map(|u| fix_url(&u)) else {
                continue;
            };
            if tracks.iter().any(|known| known.url == track_url) {
                continue;
            }
            tracks.push(SubtitleItem {
                format: subtitle_format(&track_url, track.get("format").and_then(Value::as_str)),
                language: track
                    .get("language")
                    .and_then(text_of)
                    .or_else(|| track.get("id").and_then(text_of))
                    .unwrap_or_else(|| "Unknown".to_string()),
                is_default: track.get("isDefault").and_then(Value::as_bool),
                url: track_url,
            });
        }

        subtitles.insert(url.clone(), tracks);
        streams.push(StreamItem {
            url,
            stream_type: stream_type.to_string(),
            quality: Some(quality),
            audio: Some(category.as_str().to_string()),
            subtitle_variant,
            referer: referer.clone(),
            origin: origin.clone(),
            headers: (!headers.is_empty()).then(|| headers.clone()),
            is_active: streams.is_empty(),
        });
    }
    if streams.is_empty() {
        return Err(format!("{} returned no video", server));
    }
    Ok(ServerSources {
        streams,
        subtitles,
        skip: parse_skip(value),
    })
}

fn same_tracks(a: &[SubtitleItem], b: &[SubtitleItem]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.url == y.url)
}

enum ServerEvent {
    Resolved(usize, Result<ServerSources, AppError>),
    Verified(usize, Result<Vec<StreamItem>, String>),
}

pub struct PluginProvider {
    id: &'static str,
    runtime: Arc<PluginRuntime>,
    settings: ProviderSettings,
    cache_key: String,
}

impl PluginProvider {
    /// `revision` tells one installed version of the plugin from the next, so episode lists
    /// cached for the old code are not used with the new.
    pub fn new(id: &str, runtime: Arc<PluginRuntime>, settings: ProviderSettings, revision: &str) -> Self {
        Self {
            id: intern(id),
            runtime,
            settings,
            cache_key: format!("{}@{}", id, revision),
        }
    }

    pub fn settings(&self) -> &ProviderSettings {
        &self.settings
    }

    fn error(&self, message: impl Into<String>) -> AppError {
        AppError::provider(self.id, message)
    }

    /// What the plugin's own requests ran into since `since`, when that explains a failure
    /// better than the plugin's words.
    fn network_failure(&self, since: Instant) -> Option<AppError> {
        self.runtime.engine().ok()?.net().failure_since(since)
    }

    async fn call(
        &self,
        method: &str,
        args: &[Value],
        timeout: Duration,
        access: SessionUse,
    ) -> Result<Value, AppError> {
        let started = Instant::now();
        match self.runtime.call_allowing_session(method, args, timeout, access).await {
            Ok(value) => Ok(value),
            Err(error) => Err(self.network_failure(started).unwrap_or_else(|| match error {
                PluginError::Timeout if !self.session_time_since(started).is_zero() => self.error(format!(
                    "{} timed out while the in-app browser was loading the site",
                    method
                )),
                PluginError::Timeout => self.error(format!("{} timed out", method)),
                other => self.error(other.reason()),
            })),
        }
    }

    fn media_json(query: &AnimeQuery) -> Value {
        let romaji = query.titles.first().cloned();
        let english = query.english.clone().or_else(|| romaji.clone());
        let synonyms: Vec<&String> = query
            .titles
            .iter()
            .skip(1)
            .filter(|title| Some(*title) != query.english.as_ref())
            .collect();
        json!({
            "id": query.anilist_id,
            "idMal": query.mal_id,
            "status": Value::Null,
            "format": query.format,
            "englishTitle": english,
            "romajiTitle": romaji,
            "episodeCount": query.episodes,
            "absoluteSeasonOffset": 0,
            "synonyms": synonyms,
            "isAdult": false,
            "startDate": query.year.map(|year| json!({ "year": year })),
        })
    }

    /// Searches with the known titles in turn until one search has a hit that matches the anime.
    async fn find_series(&self, query: &AnimeQuery, dub: bool, access: SessionUse) -> Result<Option<String>, AppError> {
        let started = Instant::now();
        let media = Self::media_json(query);
        let mut last_error: Option<AppError> = None;
        let mut answered = false;
        for title in query.titles.iter().take(MAX_SEARCH_TITLES) {
            let options = json!({ "media": media, "query": title, "dub": dub, "year": query.year });
            let value = match self.call("search", &[options], SEARCH_TIMEOUT, access).await {
                Ok(value) => value,
                Err(error) => {
                    let fatal = matches!(
                        error,
                        AppError::CaptchaRequired | AppError::VerificationRequired { .. } | AppError::Unreachable(_)
                    );
                    if fatal {
                        return Err(error);
                    }
                    last_error = Some(error);
                    continue;
                }
            };
            answered = true;
            let candidates: Vec<Candidate<SearchHit>> = parse_search(&value)
                .map_err(|reason| self.error(reason))?
                .into_iter()
                .filter(|candidate| candidate.item.audio != if dub { "sub" } else { "dub" })
                .collect();
            if let Some(hit) = pick_best(query, candidates) {
                return Ok(Some(hit.id));
            }
        }
        if let Some(blocked) = self.network_failure(started) {
            return Err(blocked);
        }
        match last_error {
            Some(error) if !answered => Err(error),
            _ => Ok(None),
        }
    }

    async fn episodes_of(&self, series: &str, access: SessionUse) -> Result<Vec<EpisodeItem>, AppError> {
        let value = self
            .call("findEpisodes", &[Value::String(series.to_string())], EPISODES_TIMEOUT, access)
            .await?;
        parse_episodes(&value).map_err(|reason| self.error(reason))
    }
}

async fn resolve_server(
    id: &'static str,
    runtime: Arc<PluginRuntime>,
    details: Value,
    server: String,
    category: Category,
    access: SessionUse,
) -> Result<ServerSources, AppError> {
    let started = Instant::now();
    let args = [details, Value::String(server.clone())];
    let failed = |reason: String| {
        runtime
            .engine()
            .ok()
            .and_then(|engine| engine.net().failure_since(started))
            .unwrap_or_else(|| AppError::provider(id, reason))
    };
    let waited = || {
        runtime
            .engine()
            .map_or(Duration::ZERO, |engine| engine.net().session_time_since(started))
    };
    match runtime
        .call_allowing_session("findEpisodeServer", &args, SERVER_TIMEOUT, access)
        .await
    {
        Ok(value) => parse_server(&value, &server, &category).map_err(failed),
        Err(PluginError::Timeout) if !waited().is_zero() => Err(failed(
            "timed out while the in-app browser was loading the site".to_string(),
        )),
        Err(PluginError::Timeout) => Err(failed("timed out".to_string())),
        Err(other) => Err(failed(other.reason())),
    }
}

#[async_trait]
impl AnimeProvider for PluginProvider {
    fn name(&self) -> &'static str {
        self.id
    }

    fn supports_dub(&self) -> bool {
        self.settings.supports_dub
    }

    fn cache_key(&self) -> String {
        self.cache_key.clone()
    }

    fn may_use_session(&self) -> bool {
        true
    }

    fn verifies_streams(&self) -> bool {
        true
    }

    fn session_time_since(&self, since: Instant) -> Duration {
        self.runtime
            .engine()
            .map_or(Duration::ZERO, |engine| engine.net().session_time_since(since))
    }

    async fn get_episodes(&self, query: &AnimeQuery) -> Result<ProviderData, AppError> {
        self.get_episodes_with(query, SessionUse::Existing).await
    }

    async fn get_sources(&self, episode: &EpisodeItem, category: Category) -> Result<SourcesResult, AppError> {
        self.get_sources_with(episode, category, SessionUse::Existing).await
    }

    async fn get_episodes_with(&self, query: &AnimeQuery, access: SessionUse) -> Result<ProviderData, AppError> {
        if query.titles.is_empty() {
            return Err(self.error("no title to search with"));
        }
        let find_dub = async {
            if self.settings.supports_dub {
                self.find_series(query, true, access).await
            } else {
                Ok(None)
            }
        };
        let (sub_series, dub_series) = tokio::join!(self.find_series(query, false, access), find_dub);
        let dub_series = match dub_series {
            Ok(series) => series,
            Err(error) => {
                tracing::warn!("plugin {}: dub search failed: {}", self.id, error.reason());
                None
            }
        };
        let sub_series = match sub_series {
            Ok(series) => series,
            Err(error) if dub_series.is_none() => return Err(error),
            Err(error) => {
                tracing::warn!("plugin {}: sub search failed: {}", self.id, error.reason());
                None
            }
        };

        let mut data = ProviderData::empty(self.id);
        match (sub_series, dub_series) {
            (None, None) => {}
            (Some(series), None) => data.sub = self.episodes_of(&series, access).await?,
            (None, Some(series)) => data.dub = self.episodes_of(&series, access).await?,
            (Some(sub), Some(dub)) if sub == dub => {
                data.sub = self.episodes_of(&sub, access).await?;
                data.dub = data.sub.clone();
            }
            (Some(sub), Some(dub)) => {
                let (sub_list, dub_list) =
                    tokio::join!(self.episodes_of(&sub, access), self.episodes_of(&dub, access));
                data.sub = sub_list?;
                data.dub = match dub_list {
                    Ok(list) => list,
                    Err(error) => {
                        tracing::warn!("plugin {}: dub episode list failed: {}", self.id, error.reason());
                        Vec::new()
                    }
                };
            }
        }
        Ok(data)
    }

    /// Asks the plugin's servers side by side. The first server with a stream that passes the
    /// playback check wins; streams of the other servers that had answered by then are kept
    /// behind it as fallbacks when they come with the same subtitles, so the subtitles returned
    /// belong to every stream returned.
    async fn get_sources_with(
        &self,
        episode: &EpisodeItem,
        category: Category,
        access: SessionUse,
    ) -> Result<SourcesResult, AppError> {
        let details: Value = serde_json::from_str(&episode.pipe_id)
            .ok()
            .filter(|value: &Value| value.get("id").is_some())
            .ok_or_else(|| self.error("invalid episode id"))?;
        let servers = servers_for(&self.settings.episode_servers, &category);

        let (events_tx, mut events) = mpsc::unbounded_channel::<ServerEvent>();
        let permits = Arc::new(Semaphore::new(SERVER_CONCURRENCY));
        let mut tasks = JoinSet::new();
        for (index, server) in servers.iter().cloned().enumerate() {
            let (id, runtime, details, category) = (self.id, self.runtime.clone(), details.clone(), category.clone());
            let (events_tx, permits) = (events_tx.clone(), permits.clone());
            tasks.spawn(async move {
                let resolved = {
                    let _permit = permits.acquire().await;
                    resolve_server(id, runtime, details, server, category, access).await
                };
                let streams = resolved.as_ref().ok().map(|sources| sources.streams.clone());
                let _ = events_tx.send(ServerEvent::Resolved(index, resolved));
                if let Some(streams) = streams {
                    let _ = events_tx.send(ServerEvent::Verified(index, verify_streams_in_order(streams).await));
                }
            });
        }
        drop(events_tx);

        let mut resolved: HashMap<usize, ServerSources> = HashMap::new();
        let mut failures: Vec<(usize, AppError)> = Vec::new();
        let mut winner: Option<(usize, Vec<StreamItem>)> = None;
        while let Some(event) = events.recv().await {
            match event {
                ServerEvent::Resolved(index, Ok(sources)) => {
                    resolved.insert(index, sources);
                }
                ServerEvent::Resolved(index, Err(error)) => failures.push((index, error)),
                ServerEvent::Verified(index, Ok(ordered)) => {
                    winner = Some((index, ordered));
                    break;
                }
                ServerEvent::Verified(index, Err(reason)) => {
                    resolved.remove(&index);
                    failures.push((index, self.error(format!("stream check failed: {}", reason))));
                }
            }
        }
        drop(tasks);

        let Some((winning, ordered)) = winner else {
            failures.sort_by_key(|(index, _)| *index);
            let special = failures.iter().position(|(_, error)| {
                matches!(error, AppError::VerificationRequired { .. } | AppError::CaptchaRequired)
            });
            if let Some(at) = special {
                return Err(failures.swap_remove(at).1);
            }
            if !failures.is_empty() && failures.iter().all(|(_, error)| error.is_connection_failure()) {
                return Err(failures.swap_remove(0).1);
            }
            if failures.len() == 1 {
                return Err(self.error(failures[0].1.reason()));
            }
            let listed: Vec<(String, String)> = failures
                .iter()
                .map(|(index, error)| (servers[*index].clone(), error.reason()))
                .collect();
            return Err(if listed.is_empty() {
                AppError::NoSourcesFound
            } else {
                self.error(summarize_failures(&listed))
            });
        };

        let Some(won) = resolved.remove(&winning) else {
            return Err(AppError::NoSourcesFound);
        };
        let tracks = ordered
            .first()
            .and_then(|stream| won.subtitles.get(&stream.url))
            .cloned()
            .unwrap_or_default();
        let mut streams: Vec<StreamItem> = Vec::new();
        let mut keep = |stream: StreamItem, owner: &ServerSources| {
            let own = owner.subtitles.get(&stream.url).map(Vec::as_slice).unwrap_or(&[]);
            if same_tracks(own, &tracks) && !streams.iter().any(|known: &StreamItem| known.url == stream.url) {
                streams.push(stream);
            }
        };
        for stream in ordered {
            keep(stream, &won);
        }
        let mut others: Vec<(usize, ServerSources)> = resolved.into_iter().collect();
        others.sort_by_key(|(index, _)| *index);
        for (_, other) in &others {
            for stream in other.streams.clone() {
                keep(stream, other);
            }
        }
        for (position, stream) in streams.iter_mut().enumerate() {
            stream.is_active = position == 0;
        }

        Ok(SourcesResult {
            streams,
            subtitles: tracks,
            skip: won.skip,
            provider: Some(self.id.to_string()),
        })
    }
}
