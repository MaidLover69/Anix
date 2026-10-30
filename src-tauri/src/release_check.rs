//! Confirmed releases: after a belled episode's air time, the enabled extensions are asked
//! whether they have it, separately for the subtitled version (Japanese audio) and the dub.
//!
//! The first check is [`FIRST_CHECK_DELAY_S`] after the air time, then one every
//! [`CHECK_INTERVAL_S`] (longer when the limit is raised, so a track never gets more than
//! [`MAX_CHECKS`]) until the limit set in Settings › Notifications (default three hours). A hit
//! is announced once per track, with its language ("Episode 5 is out — Japanese audio ·
//! English subtitles"); a track with no hit by the limit gets one last note that no source has
//! the episode yet (only the track the user watches: the dub with "Prefer dub" on, else the
//! sub; the other ends quietly).
//!
//! Extensions are asked one at a time, the provider picked in settings first, then by the
//! language tiers of `providers::languages::plan_tiers`, and the search stops at the first hit.
//!
//! Every track's state is a row of `release_checks`; a hit or a final note is recorded with an
//! update that only succeeds once, so two processes (the app and `--background`) never both
//! announce it. [`plan`] decides what is due from those rows and the clock alone.

use crate::db::NotificationBell as Bell;
use crate::models::{AnimeQuery, Category, ProviderData};
use crate::notifier::{Airing, Snapshot, CATCH_UP_WINDOW_S};
use crate::providers::languages::{plan_tiers, served_language, LanguagePrefs};
use crate::providers::AnimeProvider;
use crate::web_session::SessionUse;
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::sync::Arc;
use std::time::Duration;

/// The first check, after the air time.
pub const FIRST_CHECK_DELAY_S: i64 = 10 * 60;
/// Time between checks with the default limit.
pub const CHECK_INTERVAL_S: i64 = 15 * 60;
/// Checks per episode and track, at most.
pub const MAX_CHECKS: u32 = 12;
pub const DEFAULT_LIMIT_HOURS: i64 = 3;
pub const MAX_LIMIT_HOURS: i64 = 12;
/// The settings key of the limit, in hours.
pub const LIMIT_HOURS_KEY: &str = "release_check_hours";
/// Longest wait for one extension's answer.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const HINT_TIMEOUT: Duration = Duration::from_secs(10);

/// What the checks read from the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckSettings {
    /// Seconds after the air time at which checking stops.
    pub limit_s: i64,
    /// The track whose miss gets a final note.
    pub main_track: Category,
}

impl Default for CheckSettings {
    fn default() -> Self {
        Self { limit_s: DEFAULT_LIMIT_HOURS * 3600, main_track: Category::Sub }
    }
}

impl CheckSettings {
    pub fn from_settings(settings: &Value) -> Self {
        let hours = settings
            .get(LIMIT_HOURS_KEY)
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_LIMIT_HOURS)
            .clamp(1, MAX_LIMIT_HOURS);
        let prefer_dub = settings.get("prefer_dub").and_then(Value::as_bool).unwrap_or(false);
        Self {
            limit_s: hours * 3600,
            main_track: if prefer_dub { Category::Dub } else { Category::Sub },
        }
    }
}

/// One track of one aired episode that is still being looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRow {
    pub media_id: i64,
    pub episode: i32,
    pub track: Category,
    pub airing_at: i64,
    pub checks: u32,
    /// Unix seconds of the last check; 0 before the first.
    pub last_check_at: i64,
}

/// Time between checks under `limit_s`: [`CHECK_INTERVAL_S`], or longer so that at most
/// [`MAX_CHECKS`] fit before the limit.
pub fn interval_s(limit_s: i64) -> i64 {
    let span = (limit_s - FIRST_CHECK_DELAY_S).max(0);
    CHECK_INTERVAL_S.max((span + MAX_CHECKS as i64 - 1) / MAX_CHECKS as i64)
}

/// When `row` is next checked: on its fixed slot, and never sooner than one interval after the
/// last check (a check that ran late, after a suspend, does not cause a burst).
pub fn next_check_at(row: &CheckRow, limit_s: i64) -> i64 {
    let interval = interval_s(limit_s);
    let slot = row.airing_at + FIRST_CHECK_DELAY_S + row.checks as i64 * interval;
    if row.checks == 0 {
        slot
    } else {
        slot.max(row.last_check_at + interval)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Ask the extensions now.
    Check,
    /// Stop looking; `announce` sends the final note (not for a check that went stale while
    /// the computer was off).
    GiveUp { announce: bool },
    /// Nothing until then.
    Wait(i64),
}

pub fn decide(row: &CheckRow, now: i64, limit_s: i64) -> Decision {
    let give_up_at = row.airing_at + limit_s;
    if now - give_up_at > CATCH_UP_WINDOW_S {
        return Decision::GiveUp { announce: false };
    }
    let next = next_check_at(row, limit_s);
    // Once checks run out the note waits for the limit; a track never checked (the computer
    // was off) is checked once before it is given up.
    let may_check = row.checks < MAX_CHECKS && (next <= give_up_at || row.checks == 0);
    if may_check {
        return if next <= now { Decision::Check } else { Decision::Wait(next) };
    }
    if now >= give_up_at {
        Decision::GiveUp { announce: true }
    } else {
        Decision::Wait(give_up_at)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckPlan {
    pub check: Vec<CheckRow>,
    /// Rows to close, with whether their final note is sent.
    pub give_up: Vec<(CheckRow, bool)>,
    /// When something is next due.
    pub wake_at: Option<i64>,
}

pub fn plan(rows: &[CheckRow], now: i64, limit_s: i64) -> CheckPlan {
    let mut out = CheckPlan::default();
    for row in rows {
        match decide(row, now, limit_s) {
            Decision::Check => out.check.push(row.clone()),
            Decision::GiveUp { announce } => out.give_up.push((row.clone(), announce)),
            Decision::Wait(at) => out.wake_at = Some(out.wake_at.map_or(at, |t| t.min(at))),
        }
    }
    out
}

/// The aired episodes of belled anime in `snapshot` that checks start for: aired by `now`,
/// after the bell was switched on and notifications were last off, and not yet past the limit.
pub fn starts(snapshot: &Snapshot, bells: &[Bell], silenced_until: i64, now: i64, limit_s: i64) -> Vec<Airing> {
    let mut out: Vec<Airing> = snapshot
        .airings
        .iter()
        .filter(|a| a.airing_at <= now && now - a.airing_at < limit_s && a.airing_at > silenced_until)
        .filter(|a| bells.iter().any(|b| b.media_id == a.media_id && a.airing_at >= b.enabled_at))
        .cloned()
        .collect();
    out.sort_by_key(|a| (a.media_id, a.episode, std::cmp::Reverse(a.airing_at)));
    out.dedup_by_key(|a| (a.media_id, a.episode));
    out
}

pub const TRACKS: [Category; 2] = [Category::Sub, Category::Dub];

/// How a track reads in an alert: "Japanese audio · English subtitles", "Hindi audio".
pub fn track_label(track: &Category, language: Option<&str>) -> String {
    track_label_in("en", track, language)
}

/// `track_label` in an interface language (`crate::i18n`).
pub fn track_label_in(lang: &str, track: &Category, language: Option<&str>) -> String {
    use crate::i18n::{fill, language_name as name_in, text};
    match (track, language) {
        (Category::Sub, Some(code)) => fill(lang, "track_sub_in", &[("language", &name_in(lang, code))]),
        (Category::Sub, None) => text(lang, "track_sub").to_string(),
        (Category::Dub, Some(code)) => fill(lang, "track_dub_in", &[("language", &name_in(lang, code))]),
        (Category::Dub, None) => text(lang, "track_dub").to_string(),
    }
}

pub fn found_body(episode: i32, track: &Category, language: Option<&str>) -> String {
    found_body_in("en", episode, track, language)
}

pub fn found_body_in(lang: &str, episode: i32, track: &Category, language: Option<&str>) -> String {
    crate::i18n::fill(
        lang,
        "found",
        &[("episode", &episode.to_string()), ("track", &track_label_in(lang, track, language))],
    )
}

pub fn missing_body(episode: i32, track: &Category, prefs: &LanguagePrefs) -> String {
    missing_body_in("en", episode, track, prefs)
}

pub fn missing_body_in(lang: &str, episode: i32, track: &Category, prefs: &LanguagePrefs) -> String {
    let languages = crate::i18n::language_list(lang, prefs.for_category(track));
    let wanted = crate::i18n::fill(
        lang,
        if *track == Category::Dub { "wanted_dub" } else { "wanted_sub" },
        &[("languages", &languages)],
    );
    crate::i18n::fill(lang, "missing", &[("episode", &episode.to_string()), ("wanted", &wanted)])
}

// ---------------------------------------------------------------------------------------
// Asking the extensions
// ---------------------------------------------------------------------------------------

/// An extension that has the episode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub provider: String,
    /// ISO 639-1 code: the subtitle language of a sub, the audio language of a dub.
    pub language: Option<String>,
}

#[async_trait]
pub trait ReleaseProbe: Send + Sync {
    /// For each of `tracks`, the first extension that has `episode` of the anime.
    async fn find(&self, bell: &Bell, episode: i32, tracks: &[Category]) -> Vec<(Category, Option<Hit>)>;
}

fn dub_only(provider: &dyn AnimeProvider) -> bool {
    provider.languages().map_or(false, |l| l.dub_only(provider.supports_dub()))
}

/// Whether the provider can answer `track` at all.
fn answers(provider: &dyn AnimeProvider, track: &Category) -> bool {
    match track {
        Category::Sub => !dub_only(provider),
        Category::Dub => provider.supports_dub() || dub_only(provider),
    }
}

/// The order in which providers are asked for `track`, with the language each is asked in:
/// the provider picked in settings first, then the language tiers.
pub fn probe_order(providers: &[Arc<dyn AnimeProvider>], track: &Category, prefs: &LanguagePrefs) -> Vec<(usize, Option<String>)> {
    let lineup: Vec<_> = providers.iter().map(|p| (p.languages(), answers(p.as_ref(), track))).collect();
    let wanted = prefs.for_category(track);
    let language_of = |idx: usize, tier: Option<String>| {
        tier.or_else(|| served_language(lineup[idx].0.as_ref(), track, wanted))
    };
    let mut order: Vec<(usize, Option<String>)> = Vec::new();
    if let Some(pinned) = &prefs.pinned {
        if let Some(idx) = providers.iter().position(|p| p.name().eq_ignore_ascii_case(pinned)) {
            if lineup[idx].1 {
                order.push((idx, language_of(idx, None)));
            }
        }
    }
    for tier in plan_tiers(&lineup, track, prefs) {
        for idx in tier.members {
            if !order.iter().any(|(i, _)| *i == idx) {
                order.push((idx, language_of(idx, tier.language.clone())));
            }
        }
    }
    order
}

/// The episode in a listing of `track`.
fn listed(provider: &dyn AnimeProvider, data: &ProviderData, track: &Category, episode: i32) -> bool {
    let list = match track {
        Category::Dub if dub_only(provider) && data.dub.is_empty() => &data.sub,
        Category::Dub => &data.dub,
        Category::Sub => &data.sub,
    };
    list.iter().any(|item| item.number == episode as f64)
}

/// Asks one provider whether it has `episode` in `track`. A provider addressed by episode
/// number has no list, so it is asked for the episode's streams instead. `listing` keeps the
/// provider's list between the tracks of one search.
async fn has_episode(
    provider: &dyn AnimeProvider,
    query: &AnimeQuery,
    episode: i32,
    track: &Category,
    listing: &mut Option<ProviderData>,
) -> bool {
    if let Some(item) = provider.episode_by_number(query, episode as f64) {
        let asked = if *track == Category::Dub && !provider.supports_dub() { Category::Sub } else { track.clone() };
        let sources = tokio::time::timeout(PROBE_TIMEOUT, provider.get_sources_with(&item, asked, SessionUse::Existing)).await;
        return matches!(sources, Ok(Ok(result)) if !result.streams.is_empty());
    }
    if listing.is_none() {
        match tokio::time::timeout(PROBE_TIMEOUT, provider.get_episodes_with(query, SessionUse::Existing)).await {
            Ok(Ok(data)) => *listing = Some(data),
            Ok(Err(e)) => {
                tracing::info!("Release check: {} gave no episode list ({})", provider.name(), e);
                return false;
            }
            Err(_) => {
                tracing::info!("Release check: {} timed out", provider.name());
                return false;
            }
        }
    }
    listing.as_ref().map_or(false, |data| listed(provider, data, track, episode))
}

/// The first provider in `order` with the episode, asking one provider at a time.
pub async fn find_first(
    providers: &[Arc<dyn AnimeProvider>],
    query: &AnimeQuery,
    episode: i32,
    track: &Category,
    prefs: &LanguagePrefs,
    listings: &mut [Option<ProviderData>],
) -> Option<Hit> {
    for (idx, language) in probe_order(providers, track, prefs) {
        let provider = providers[idx].as_ref();
        if has_episode(provider, query, episode, track, &mut listings[idx]).await {
            return Some(Hit { provider: provider.name().to_string(), language });
        }
    }
    None
}

/// The installed, enabled extensions under the user's language settings.
pub struct ExtensionProbe {
    providers: crate::providers::ProviderSource,
    prefs: crate::providers::LanguageSource,
}

impl ExtensionProbe {
    pub fn new(providers: crate::providers::ProviderSource, prefs: crate::providers::LanguageSource) -> Self {
        Self { providers, prefs }
    }
}

/// The query extensions search with: the bell's title, plus the catalogue's titles and ids.
async fn query_for(bell: &Bell) -> AnimeQuery {
    let mut query = AnimeQuery::new(bell.media_id, None, Some(&bell.media_title));
    if let Ok(Ok(hint)) = tokio::time::timeout(HINT_TIMEOUT, crate::catalogue::media_hint(bell.media_id)).await {
        for title in &hint.titles {
            query.add_title(title);
        }
        query.mal_id = hint.id_mal;
        query.year = hint.year;
        query.episodes = hint.episodes;
        query.format = hint.format.clone();
        query.english = hint.english.clone();
    }
    query
}

#[async_trait]
impl ReleaseProbe for ExtensionProbe {
    async fn find(&self, bell: &Bell, episode: i32, tracks: &[Category]) -> Vec<(Category, Option<Hit>)> {
        let providers = (self.providers)();
        let prefs = (self.prefs)();
        let query = query_for(bell).await;
        let mut listings: Vec<Option<ProviderData>> = vec![None; providers.len()];
        let mut out = Vec::new();
        for track in tracks {
            let hit = find_first(&providers, &query, episode, track, &prefs, &mut listings).await;
            out.push((track.clone(), hit));
        }
        out
    }
}

/// What the notifier runs the checks with.
#[derive(Clone)]
pub struct ReleaseChecks {
    pub probe: Arc<dyn ReleaseProbe>,
    pub settings: CheckSettings,
    pub prefs: LanguagePrefs,
}

impl std::fmt::Debug for ReleaseChecks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReleaseChecks").field("settings", &self.settings).finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------------------

pub async fn create_tables(conn: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS release_checks (
            media_id INTEGER NOT NULL,
            episode INTEGER NOT NULL,
            track TEXT NOT NULL,
            airing_at INTEGER NOT NULL,
            checks INTEGER NOT NULL DEFAULT 0,
            last_check_at INTEGER NOT NULL DEFAULT 0,
            outcome TEXT,
            provider TEXT,
            language TEXT,
            finished_at INTEGER,
            PRIMARY KEY (media_id, episode, track)
        )
        "#,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Starts checking a track of an aired episode; nothing changes when it is already known.
pub async fn start(pool: &SqlitePool, airing: &Airing, track: &Category) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR IGNORE INTO release_checks (media_id, episode, track, airing_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(airing.media_id)
    .bind(airing.episode)
    .bind(track.as_str())
    .bind(airing.airing_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every open track of an anime that still has its bell on.
pub async fn pending(pool: &SqlitePool) -> Result<Vec<CheckRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT media_id, episode, track, airing_at, checks, last_check_at FROM release_checks \
         WHERE outcome IS NULL \
           AND media_id IN (SELECT media_id FROM notification_preferences WHERE enabled = 1) \
           AND media_id NOT IN (SELECT stand_in FROM catalogue_merged_ids) \
         ORDER BY airing_at, media_id, episode, track",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| CheckRow {
            media_id: r.get("media_id"),
            episode: r.get("episode"),
            track: Category::from(r.get::<String, _>("track").as_str()),
            airing_at: r.get("airing_at"),
            checks: r.get::<i64, _>("checks").max(0) as u32,
            last_check_at: r.get("last_check_at"),
        })
        .collect())
}

pub async fn note_check(pool: &SqlitePool, row: &CheckRow, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE release_checks SET checks = checks + 1, last_check_at = ?4 \
         WHERE media_id = ?1 AND episode = ?2 AND track = ?3 AND outcome IS NULL",
    )
    .bind(row.media_id)
    .bind(row.episode)
    .bind(row.track.as_str())
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Closes a track as `found` (with the hit) or `missing`. False when it was already closed,
/// by this process or another, so the caller announces it only on true.
pub async fn finish(pool: &SqlitePool, row: &CheckRow, hit: Option<&Hit>, now: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE release_checks SET outcome = ?4, provider = ?5, language = ?6, finished_at = ?7 \
         WHERE media_id = ?1 AND episode = ?2 AND track = ?3 AND outcome IS NULL",
    )
    .bind(row.media_id)
    .bind(row.episode)
    .bind(row.track.as_str())
    .bind(if hit.is_some() { "found" } else { "missing" })
    .bind(hit.map(|h| h.provider.clone()))
    .bind(hit.and_then(|h| h.language.clone()))
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Unused rows are not kept forever.
pub async fn prune(pool: &SqlitePool, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM release_checks WHERE airing_at < ?1")
        .bind(now - 60 * 86_400)
        .execute(pool)
        .await?;
    Ok(())
}
