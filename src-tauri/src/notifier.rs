//! Release notifications: a desktop notification when a new episode airs for an anime the
//! user has a bell on.
//!
//! One background task ([`start`]) keeps a snapshot of the airing schedule (AniList's, or
//! the catalogue fallback's while AniList is unavailable) for the belled anime. It re-reads the schedule every [`REFRESH_INTERVAL_S`], right after a bell
//! changes ([`replan`]) and once more when an air time passes, and sleeps until the nearest of
//! those moments. The sleep is cut into slices of at most [`WATCH_TICK_S`]; between slices only
//! the clock, the bells and the setting are looked at, never the network, so a suspended
//! machine or a changed clock is noticed soon after it happens.
//!
//! An episode is announced only once a schedule read made at or after its air time still lists
//! it as aired, so a delay or a hiatus never produces a false notification, and being offline
//! at the air time only postpones it. Times from a catalogue other than AniList are estimates
//! (weekly slots), so those episodes are never announced at their air time: only the release
//! checks ([`crate::release_check`]) announce them, once an extension has the episode. Anime
//! known only by a stand-in (negative) id get their times that way too. Every episode that was announced or deliberately skipped
//! is recorded in `notified_episodes`, which is what makes a notification fire at most once
//! across restarts, repeated checks and schedule shifts.
//!
//! All decisions are made by [`plan`], a pure function of the bells, the snapshot, the recorded
//! episodes and the current time.
//!
//! Notifications are sent with `notify-rust` rather than the notification plugin, whose desktop
//! backend drops the notification's handle and so can never report a click. Each notification
//! carries the freedesktop default action; a task waits for it and, on a click, brings the main
//! window forward and sends it [`OPEN_ANIME_EVENT`] with the anime's id.

use crate::clients::anilist::{interpret_graphql_response, ANILIST_GRAPHQL_URL};
use crate::commands::schedule::BELLS_CHANGED;
use crate::db;
use crate::error::AppError;
use crate::models::Category;
use crate::release_check::{self, ReleaseChecks};
use crate::state::AppState;
use async_trait::async_trait;
use notify_rust::{Hint, NotificationHandle, NotificationResponse, Timeout};
use reqwest::header::RETRY_AFTER;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{error, info, warn};

pub use crate::db::NotificationBell as Bell;

/// An episode that aired longer ago than this is recorded without a notification.
pub const CATCH_UP_WINDOW_S: i64 = 6 * 3600;
/// How far back a schedule read reaches. Longer than the catch-up window, so episodes that
/// are too old to announce are still seen and recorded.
pub const LOOKBACK_S: i64 = 24 * 3600;
/// How far ahead a schedule read reaches.
pub const LOOKAHEAD_S: i64 = 48 * 3600;
/// The schedule is re-read this often while at least one bell is on.
pub const REFRESH_INTERVAL_S: i64 = 30 * 60;
/// Two schedule reads are never closer together than this.
pub const MIN_FETCH_GAP_S: i64 = 5;
/// Longest single sleep while a bell is on.
pub const WATCH_TICK_S: i64 = 30;
/// First retry delay after a failed schedule read; doubles up to `RETRY_MAX_S`.
pub const RETRY_BASE_S: i64 = 30;
pub const RETRY_MAX_S: i64 = 15 * 60;
/// Upper bound for a server-requested wait.
pub const RATE_LIMIT_MAX_S: i64 = 3600;

const STORAGE_RETRY_S: i64 = 60;
/// While notifications are switched off, the time of the last check is saved this often.
const SILENCE_SAVE_EVERY_S: i64 = 10 * 60;
const SILENCED_UNTIL_KEY: &str = "silenced_until";

const IDS_PER_REQUEST: usize = 50;
const MAX_PAGES: i64 = 10;

const COVER_MAX_BYTES: usize = 4 * 1024 * 1024;
const COVER_TIMEOUT: Duration = Duration::from_secs(4);

const MAIN_WINDOW: &str = "main";
/// Sent to the main window when a notification is clicked; the payload is the AniList id.
pub const OPEN_ANIME_EVENT: &str = "open-anime";
/// The freedesktop action key for a click on the notification itself.
pub const OPEN_ACTION: &str = "default";
/// How long the pop-up stays on desktops that honour the request.
const POPUP_TIMEOUT_MS: u32 = 15_000;
/// Longest wait for the notification service to accept or close a notification.
const SERVICE_TIMEOUT: Duration = Duration::from_secs(10);
/// A notification the desktop keeps in its list is withdrawn after this long, which also
/// ends the task waiting for its click.
const KEEP_CLICKABLE: Duration = Duration::from_secs(CATCH_UP_WINDOW_S as u64);
/// Notifications waiting for a click at the same time; each holds a session-bus connection.
const MAX_CLICKABLE: usize = 16;

/// One entry of the airing schedule.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Airing {
    pub media_id: i64,
    pub episode: i32,
    /// Unix seconds.
    pub airing_at: i64,
}

/// The airing schedule as it was read at `fetched_at`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub fetched_at: i64,
    /// The anime the read asked about.
    pub media_ids: BTreeSet<i64>,
    pub airings: Vec<Airing>,
    /// Anime whose times are estimates, not AniList's.
    pub estimated: BTreeSet<i64>,
}

/// A schedule read, with the anime whose times are estimates.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarkedAirings {
    pub airings: Vec<Airing>,
    pub estimated: BTreeSet<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNotification {
    pub media_id: i64,
    pub title: String,
    pub cover_image: Option<String>,
    /// The episodes this notification announces; more than one when several became due together.
    pub episodes: Vec<Airing>,
    /// The text in place of "Episode N is out" (release checks say which version is out).
    pub note: Option<String>,
}

impl ReleaseNotification {
    /// The text in English (log lines and tests); `body_in` for what the user sees.
    pub fn body(&self) -> String {
        self.body_in("en")
    }

    /// The text in an interface language (`crate::i18n`).
    pub fn body_in(&self, lang: &str) -> String {
        use crate::i18n::fill;
        if let Some(note) = &self.note {
            return note.clone();
        }
        let mut numbers: Vec<i32> = self.episodes.iter().map(|a| a.episode).collect();
        numbers.sort_unstable();
        numbers.dedup();
        match numbers.as_slice() {
            [] => String::new(),
            [only] => fill(lang, "episode_out", &[("episode", &only.to_string())]),
            [first, second] => {
                fill(lang, "two_out", &[("first", &first.to_string()), ("second", &second.to_string())])
            }
            [first, .., last] => {
                fill(lang, "range_out", &[("first", &first.to_string()), ("last", &last.to_string())])
            }
        }
    }
}

pub struct PlanInput<'a> {
    /// Unix seconds.
    pub now: i64,
    /// The "Release notifications" setting.
    pub enabled: bool,
    /// The last moment at which the setting was seen switched off.
    pub silenced_until: i64,
    /// Bells that are switched on.
    pub bells: &'a [Bell],
    /// The previous schedule read, if there was one.
    pub snapshot: Option<&'a Snapshot>,
    /// `(media_id, episode)` of every episode already announced or skipped.
    pub notified: &'a HashSet<(i64, i32)>,
    /// Earliest time the schedule may be read again (retry back-off, minimum gap).
    pub next_fetch_allowed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Notifications to show now, earliest air time first.
    pub fire: Vec<ReleaseNotification>,
    /// Episodes to record without showing anything.
    pub mark_seen: Vec<Airing>,
    /// Read the schedule now, then plan again.
    pub refresh: bool,
    /// When to plan next, in Unix seconds.
    pub wake_at: i64,
}

/// Decides what to do at `input.now`. An episode is only acted on once it is confirmed, that
/// is, listed with an air time at or before the moment the snapshot was read.
pub fn plan(input: &PlanInput<'_>) -> Plan {
    let now = input.now;
    let mut plan = Plan {
        fire: Vec::new(),
        mark_seen: Vec::new(),
        refresh: false,
        wake_at: now + REFRESH_INTERVAL_S,
    };
    if !input.enabled || input.bells.is_empty() {
        return plan;
    }

    let bells: HashMap<i64, &Bell> = input.bells.iter().map(|b| (b.media_id, b)).collect();
    // A snapshot from "the future" means the clock was set back; nothing in it is confirmed.
    let snapshot = input.snapshot.filter(|s| s.fetched_at <= now);

    let mut needs_refresh = match snapshot {
        None => true,
        Some(s) => {
            now - s.fetched_at >= REFRESH_INTERVAL_S
                || input.bells.iter().any(|b| !s.media_ids.contains(&b.media_id))
        }
    };
    let mut next_airing: Option<i64> = None;
    let mut due: Vec<&Airing> = Vec::new();

    if let Some(snapshot) = snapshot {
        // If an episode is listed twice, the later air time is the one that counts.
        let mut airings: Vec<&Airing> = snapshot.airings.iter().collect();
        airings.sort_by_key(|a| std::cmp::Reverse(a.airing_at));
        let mut handled: HashSet<(i64, i32)> = HashSet::new();

        for airing in airings {
            let Some(bell) = bells.get(&airing.media_id) else {
                continue;
            };
            let key = (airing.media_id, airing.episode);
            if input.notified.contains(&key) || !handled.insert(key) {
                continue;
            }
            if airing.airing_at > snapshot.fetched_at {
                if airing.airing_at <= now {
                    needs_refresh = true;
                } else {
                    next_airing = Some(next_airing.map_or(airing.airing_at, |t| t.min(airing.airing_at)));
                }
                continue;
            }
            // Estimated times are announced by the release checks, once a source has the episode.
            let silent = snapshot.estimated.contains(&airing.media_id)
                || airing.airing_at < bell.enabled_at
                || airing.airing_at <= input.silenced_until
                || now - airing.airing_at > CATCH_UP_WINDOW_S;
            if silent {
                plan.mark_seen.push(airing.clone());
            } else {
                due.push(airing);
            }
        }
    }

    plan.mark_seen.sort_by_key(|a| (a.airing_at, a.media_id, a.episode));
    due.sort_by_key(|a| (a.airing_at, a.media_id, a.episode));
    for airing in due {
        match plan.fire.iter_mut().find(|n| n.media_id == airing.media_id) {
            Some(notification) => notification.episodes.push(airing.clone()),
            None => {
                let bell = bells[&airing.media_id];
                plan.fire.push(ReleaseNotification {
                    media_id: bell.media_id,
                    title: bell.media_title.clone(),
                    cover_image: bell.cover_image.clone(),
                    episodes: vec![airing.clone()],
                    note: None,
                });
            }
        }
    }

    match snapshot {
        Some(snapshot) if !needs_refresh => {
            let refresh_at = snapshot.fetched_at + REFRESH_INTERVAL_S;
            plan.wake_at = next_airing.map_or(refresh_at, |t| t.min(refresh_at));
        }
        _ => {
            // No back-off is longer than this, so a later time means the clock was set back.
            let allowed_at = if input.next_fetch_allowed_at - now > RATE_LIMIT_MAX_S {
                now
            } else {
                input.next_fetch_allowed_at
            };
            plan.refresh = allowed_at <= now;
            plan.wake_at = now.max(allowed_at);
        }
    }
    plan
}

/// Seconds to wait before the next schedule read after `failures` consecutive failed ones.
pub fn retry_delay_s(failures: u32, error: &AppError) -> i64 {
    let doublings = failures.saturating_sub(1).min(10);
    let backoff = (RETRY_BASE_S << doublings).min(RETRY_MAX_S);
    match error {
        AppError::RateLimited { retry_after_s } => {
            let requested = i64::try_from(*retry_after_s).unwrap_or(RATE_LIMIT_MAX_S);
            backoff.max(requested.min(RATE_LIMIT_MAX_S))
        }
        _ => backoff,
    }
}

/// The ids to ask the schedule about: valid (stand-in ids included), without duplicates, in
/// ascending order.
pub fn schedule_media_ids(bells: &[Bell]) -> Vec<i64> {
    let ids: BTreeSet<i64> = bells.iter().map(|b| b.media_id).filter(|id| *id != 0).collect();
    ids.into_iter().collect()
}

/// The "Release notifications" setting; on unless `release_notifications` is `false`.
pub fn release_notifications_enabled(settings: &Value) -> bool {
    settings
        .get("release_notifications")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

#[async_trait]
pub trait ScheduleSource: Send + Sync {
    /// Every airing of `media_ids` between `from_ts` and `to_ts` (Unix seconds, exclusive).
    async fn fetch(&self, media_ids: &[i64], from_ts: i64, to_ts: i64) -> Result<Vec<Airing>, AppError>;

    /// `fetch`, saying which anime's times are estimates; none, unless a source says so.
    async fn fetch_marked(&self, media_ids: &[i64], from_ts: i64, to_ts: i64) -> Result<MarkedAirings, AppError> {
        Ok(MarkedAirings { airings: self.fetch(media_ids, from_ts, to_ts).await?, estimated: BTreeSet::new() })
    }
}

#[async_trait]
pub trait NotificationSink: Send + Sync {
    async fn show(&self, notification: &ReleaseNotification);
}

const AIRING_QUERY: &str = r#"
query ($ids: [Int], $from: Int, $to: Int, $page: Int) {
    Page(page: $page, perPage: 50) {
        pageInfo {
            hasNextPage
        }
        airingSchedules(mediaId_in: $ids, airingAt_greater: $from, airingAt_lesser: $to, sort: TIME) {
            mediaId
            episode
            airingAt
        }
    }
}
"#;

/// One page of [`AIRING_QUERY`]'s `data`: its airings and whether another page follows.
pub fn parse_airing_page(data: &Value) -> Result<(Vec<Airing>, bool), AppError> {
    let page = &data["Page"];
    let schedules = page["airingSchedules"]
        .as_array()
        .ok_or_else(|| AppError::Parse("AniList response has no airingSchedules".to_string()))?;

    let airings = schedules
        .iter()
        .filter_map(|s| {
            let media_id = s["mediaId"].as_i64().filter(|id| *id > 0)?;
            let episode = s["episode"].as_i64().and_then(|e| i32::try_from(e).ok())?;
            let airing_at = s["airingAt"].as_i64().filter(|t| *t > 0)?;
            Some(Airing { media_id, episode, airing_at })
        })
        .collect();

    Ok((airings, page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false)))
}

/// AniList's airing schedule, filtered to the given anime.
pub struct AniListSchedule {
    client: reqwest::Client,
    url: String,
}

impl AniListSchedule {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_url(client, ANILIST_GRAPHQL_URL)
    }

    pub fn with_url(client: reqwest::Client, url: impl Into<String>) -> Self {
        Self { client, url: url.into() }
    }

    async fn post(&self, body: &Value) -> Result<Value, AppError> {
        let response = self.client.post(&self.url).json(body).send().await?;
        let status = response.status().as_u16();
        let retry_after_s = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let text = response.text().await?;
        interpret_graphql_response(status, retry_after_s, &text)
    }
}

#[async_trait]
impl ScheduleSource for AniListSchedule {
    async fn fetch(&self, media_ids: &[i64], from_ts: i64, to_ts: i64) -> Result<Vec<Airing>, AppError> {
        let mut airings = Vec::new();
        for ids in media_ids.chunks(IDS_PER_REQUEST) {
            let mut page = 1;
            loop {
                let body = json!({
                    "query": AIRING_QUERY,
                    "variables": { "ids": ids, "from": from_ts, "to": to_ts, "page": page },
                });
                let (found, has_next_page) = parse_airing_page(&self.post(&body).await?)?;
                airings.extend(found);
                // Entries are sorted by time, so a cut-off list only lacks the furthest ones.
                if !has_next_page || page >= MAX_PAGES {
                    break;
                }
                page += 1;
            }
        }
        Ok(airings)
    }
}

/// What [`Notifier::step`] asks of its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepOutcome {
    /// When to call `step` again, in Unix seconds.
    pub wake_at: i64,
    /// A bell is on, so the wall clock has to be checked in short slices until then.
    pub watching: bool,
}

/// How long to sleep at `now` before the next check. While a bell is on, never longer than
/// [`WATCH_TICK_S`]: a timer does not advance while the machine is suspended, so the wall
/// clock is looked at again after each slice.
pub fn sleep_seconds(outcome: StepOutcome, now: i64) -> u64 {
    let longest = if outcome.watching { WATCH_TICK_S } else { REFRESH_INTERVAL_S };
    (outcome.wake_at - now).clamp(1, longest) as u64
}

/// The notifier's state between checks. Only `notified_episodes` and the silenced-until time
/// outlive the process; the schedule snapshot is read again after a restart.
#[derive(Debug, Default)]
pub struct Notifier {
    snapshot: Option<Snapshot>,
    notified: HashSet<(i64, i32)>,
    silenced_until: i64,
    silenced_saved: i64,
    next_fetch_allowed_at: i64,
    failures: u32,
    storage_failing: bool,
    /// Without it, nothing is asked of the extensions.
    checks: Option<ReleaseChecks>,
}

impl Notifier {
    /// Turns the release checks on (or off with `None`) from the next step on.
    pub fn set_release_checks(&mut self, checks: Option<ReleaseChecks>) {
        self.checks = checks;
    }

    pub async fn load(pool: &SqlitePool) -> Result<Self, AppError> {
        let notified = db::list_notified_episodes(pool).await?.into_iter().collect();
        let silenced_until = db::get_notifier_state(pool, SILENCED_UNTIL_KEY).await?.unwrap_or(0);
        Ok(Self {
            notified,
            silenced_until,
            silenced_saved: silenced_until,
            ..Self::default()
        })
    }

    /// One check at `now`: reads the schedule if that is due, records and shows what became
    /// due, and says when to check again. `enabled` is the "Release notifications" setting.
    pub async fn step(
        &mut self,
        pool: &SqlitePool,
        source: &dyn ScheduleSource,
        sink: &dyn NotificationSink,
        enabled: bool,
        now: i64,
    ) -> StepOutcome {
        match self.try_step(pool, source, sink, enabled, now).await {
            Ok(outcome) => {
                self.storage_failing = false;
                outcome
            }
            Err(e) => {
                if !self.storage_failing {
                    warn!("Release notifications are paused, the database is not usable: {}", e);
                    self.storage_failing = true;
                }
                StepOutcome { wake_at: now + STORAGE_RETRY_S, watching: true }
            }
        }
    }

    async fn try_step(
        &mut self,
        pool: &SqlitePool,
        source: &dyn ScheduleSource,
        sink: &dyn NotificationSink,
        enabled: bool,
        now: i64,
    ) -> Result<StepOutcome, AppError> {
        let bells = db::list_notification_bells(pool).await?;
        if bells.is_empty() {
            self.snapshot = None;
            return Ok(StepOutcome { wake_at: now + REFRESH_INTERVAL_S, watching: false });
        }

        if !enabled {
            // Whatever airs up to now stays silent, also after the setting is switched back on.
            self.silenced_until = now;
            if (now - self.silenced_saved).abs() >= SILENCE_SAVE_EVERY_S {
                self.save_silenced_until(pool).await?;
            }
            return Ok(StepOutcome { wake_at: now + REFRESH_INTERVAL_S, watching: true });
        }
        if self.silenced_saved != self.silenced_until {
            self.save_silenced_until(pool).await?;
        }

        let mut plan = self.plan(&bells, now);
        if plan.refresh {
            self.refresh(source, &bells, now).await;
            plan = self.plan(&bells, now);
        }

        for airing in &plan.mark_seen {
            self.record(pool, airing, now, false).await?;
        }
        for notification in plan.fire {
            // Recorded before it is shown: a crash in between loses one notification instead
            // of repeating it.
            let mut fresh = Vec::new();
            let mut failure = None;
            for airing in &notification.episodes {
                match self.record(pool, airing, now, true).await {
                    Ok(true) => fresh.push(airing.clone()),
                    Ok(false) => {}
                    Err(e) => {
                        failure = Some(e);
                        break;
                    }
                }
            }
            if !fresh.is_empty() {
                let notification = ReleaseNotification { episodes: fresh, ..notification };
                info!("Release notification: {} ({})", notification.title, notification.body());
                sink.show(&notification).await;
                keep_notice(pool, &notification).await;
            }
            if let Some(e) = failure {
                return Err(e);
            }
        }

        let mut wake_at = plan.wake_at;
        if let Some(at) = self.run_release_checks(pool, sink, &bells, now).await? {
            wake_at = wake_at.min(at.max(now));
        }
        if let Some(at) = self.run_reminders(pool, sink, &bells, now).await? {
            wake_at = wake_at.min(at.max(now));
        }
        Ok(StepOutcome { wake_at, watching: true })
    }

    /// Shows "airs in N minutes" reminders for bells with a lead time. Returns when the next
    /// reminder is due.
    async fn run_reminders(
        &mut self,
        pool: &SqlitePool,
        sink: &dyn NotificationSink,
        bells: &[Bell],
        now: i64,
    ) -> Result<Option<i64>, AppError> {
        let Some(snapshot) = self.snapshot.as_ref() else { return Ok(None) };
        let leads = crate::reminders::lead_minutes(pool).await?;
        if leads.is_empty() {
            return Ok(None);
        }
        let sent = crate::reminders::sent(pool).await?;
        for reminder in crate::reminders::due(&leads, &snapshot.airings, &sent, now) {
            let Some(bell) = bells.iter().find(|b| b.media_id == reminder.media_id) else { continue };
            // Recorded before it is shown, like airing notifications.
            if !crate::reminders::record_sent(pool, &reminder, now).await? {
                continue;
            }
            let notification = ReleaseNotification {
                media_id: bell.media_id,
                title: bell.media_title.clone(),
                cover_image: bell.cover_image.clone(),
                episodes: vec![Airing { media_id: reminder.media_id, episode: reminder.episode, airing_at: reminder.airing_at }],
                note: Some(reminder.body_in(crate::i18n::current(), now)),
            };
            info!("Reminder: {} ({})", notification.title, reminder.body(now));
            sink.show(&notification).await;
            keep_notice(pool, &notification).await;
        }
        let sent = crate::reminders::sent(pool).await?;
        Ok(crate::reminders::next_due_at(&leads, &snapshot.airings, &sent, now))
    }

    /// Starts checks for newly aired episodes, asks the extensions about what is due, and
    /// announces hits and final misses. Returns when a check is next due.
    async fn run_release_checks(
        &mut self,
        pool: &SqlitePool,
        sink: &dyn NotificationSink,
        bells: &[Bell],
        now: i64,
    ) -> Result<Option<i64>, AppError> {
        let Some(checks) = self.checks.clone() else {
            return Ok(None);
        };
        let limit_s = checks.settings.limit_s;
        if let Some(snapshot) = self.snapshot.as_ref().filter(|s| s.fetched_at <= now) {
            for airing in release_check::starts(snapshot, bells, self.silenced_until, now, limit_s) {
                for track in &release_check::TRACKS {
                    release_check::start(pool, &airing, track).await?;
                }
            }
        }

        let rows = release_check::pending(pool).await?;
        let plan = release_check::plan(&rows, now, limit_s);
        let bell_of = |media_id: i64| bells.iter().find(|b| b.media_id == media_id);

        // Both tracks of an episode are looked for in one search, which lists each provider once.
        let mut episodes: Vec<(i64, i32)> = plan.check.iter().map(|r| (r.media_id, r.episode)).collect();
        episodes.dedup();
        for (media_id, episode) in episodes {
            let Some(bell) = bell_of(media_id) else { continue };
            let due: Vec<&release_check::CheckRow> =
                plan.check.iter().filter(|r| r.media_id == media_id && r.episode == episode).collect();
            let tracks: Vec<Category> = due.iter().map(|r| r.track.clone()).collect();
            let found = checks.probe.find(bell, episode, &tracks).await;
            for row in due {
                release_check::note_check(pool, row, now).await?;
                let hit = found.iter().find(|(track, _)| *track == row.track).and_then(|(_, hit)| hit.as_ref());
                let Some(hit) = hit else {
                    info!(
                        "Release check: {} episode {} ({}) not found yet, check {} of at most {}",
                        bell.media_title,
                        episode,
                        row.track.as_str(),
                        row.checks + 1,
                        release_check::MAX_CHECKS
                    );
                    continue;
                };
                if release_check::finish(pool, row, Some(hit), now).await? {
                    let note = release_check::found_body_in(
                        crate::i18n::current(),
                        episode,
                        &row.track,
                        hit.language.as_deref(),
                    );
                    info!(
                        "Release check: {} episode {} found on {}: {}",
                        bell.media_title,
                        episode,
                        hit.provider,
                        release_check::found_body(episode, &row.track, hit.language.as_deref())
                    );
                    announce(pool, sink, bell, row, note).await;
                }
            }
        }

        for (row, announce_it) in &plan.give_up {
            let Some(bell) = bell_of(row.media_id) else { continue };
            if !release_check::finish(pool, row, None, now).await? {
                continue;
            }
            if *announce_it && row.track == checks.settings.main_track {
                let note = release_check::missing_body_in(crate::i18n::current(), row.episode, &row.track, &checks.prefs);
                info!(
                    "Release check: {} — {}",
                    bell.media_title,
                    release_check::missing_body(row.episode, &row.track, &checks.prefs)
                );
                announce(pool, sink, bell, row, note).await;
            }
        }
        Ok(plan.wake_at)
    }

    fn plan(&self, bells: &[Bell], now: i64) -> Plan {
        plan(&PlanInput {
            now,
            enabled: true,
            silenced_until: self.silenced_until,
            bells,
            snapshot: self.snapshot.as_ref(),
            notified: &self.notified,
            next_fetch_allowed_at: self.next_fetch_allowed_at,
        })
    }

    async fn refresh(&mut self, source: &dyn ScheduleSource, bells: &[Bell], now: i64) {
        let ids = schedule_media_ids(bells);
        match source.fetch_marked(&ids, now - LOOKBACK_S, now + LOOKAHEAD_S).await {
            Ok(marked) => {
                if self.failures > 0 {
                    info!("The airing schedule is reachable again");
                }
                self.failures = 0;
                self.next_fetch_allowed_at = now + MIN_FETCH_GAP_S;
                self.snapshot = Some(Snapshot {
                    fetched_at: now,
                    media_ids: bells.iter().map(|b| b.media_id).collect(),
                    airings: marked.airings,
                    estimated: marked.estimated,
                });
            }
            Err(e) => {
                self.failures = self.failures.saturating_add(1);
                let delay = retry_delay_s(self.failures, &e);
                // Logged once per outage; the retries that follow are silent.
                if self.failures == 1 {
                    warn!(
                        "Could not read the airing schedule for release notifications ({}); retrying with back-off",
                        e
                    );
                }
                self.next_fetch_allowed_at = now + delay;
            }
        }
    }

    async fn record(
        &mut self,
        pool: &SqlitePool,
        airing: &Airing,
        now: i64,
        shown: bool,
    ) -> Result<bool, AppError> {
        let inserted =
            db::record_notified_episode(pool, airing.media_id, airing.episode, airing.airing_at, now, shown)
                .await?;
        self.notified.insert((airing.media_id, airing.episode));
        Ok(inserted)
    }

    async fn save_silenced_until(&mut self, pool: &SqlitePool) -> Result<(), AppError> {
        db::set_notifier_state(pool, SILENCED_UNTIL_KEY, self.silenced_until).await?;
        self.silenced_saved = self.silenced_until;
        Ok(())
    }
}

/// Shows a release-check alert and keeps it for the notification centre.
async fn announce(pool: &SqlitePool, sink: &dyn NotificationSink, bell: &Bell, row: &release_check::CheckRow, note: String) {
    let notification = ReleaseNotification {
        media_id: bell.media_id,
        title: bell.media_title.clone(),
        cover_image: bell.cover_image.clone(),
        episodes: vec![Airing { media_id: row.media_id, episode: row.episode, airing_at: row.airing_at }],
        note: Some(note),
    };
    sink.show(&notification).await;
    keep_notice(pool, &notification).await;
}

/// Keeps an alert for the in-app notification centre, so alerts sent while the window was
/// closed (or by `--background`) are listed when the app opens.
async fn keep_notice(pool: &SqlitePool, n: &ReleaseNotification) {
    let link = format!("/detail/{}", n.media_id);
    if let Err(e) = crate::notices::push(pool, crate::notices::AIRING, &n.title, &n.body_in(crate::i18n::current()), Some(&link)).await {
        warn!("Could not keep the alert for {} in the notification centre: {}", n.title, e);
    }
}

/// `jpg` or `png` if `bytes` start like one; the only formats handed to the desktop as an icon.
pub fn cover_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else {
        None
    }
}

fn cached_cover(dir: &Path, media_id: i64) -> Option<PathBuf> {
    ["jpg", "png"]
        .iter()
        .map(|ext| dir.join(format!("{}.{}", media_id, ext)))
        .find(|path| path.is_file())
}

async fn download_cover(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response.content_length().is_some_and(|n| n > COVER_MAX_BYTES as u64) {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    (bytes.len() <= COVER_MAX_BYTES).then(|| bytes.to_vec())
}

/// A local copy of the anime's cover, fetched once and kept in the cache directory.
async fn cover_icon(client: &reqwest::Client, dir: &Path, media_id: i64, url: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = cached_cover(dir, media_id) {
        return Some(path);
    }
    let url = url.filter(|u| u.starts_with("https://"))?;
    let bytes = tokio::time::timeout(COVER_TIMEOUT, download_cover(client, url))
        .await
        .ok()??;
    let extension = cover_extension(&bytes)?;

    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(format!("{}.{}", media_id, extension));
    let tmp = dir.join(format!("{}.tmp{}", media_id, std::process::id()));
    if std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    Some(path)
}

/// Deletes cached covers of anime that no longer have a bell.
fn prune_covers(dir: &Path, bells: &[Bell]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let keep: HashSet<i64> = bells.iter().map(|b| b.media_id).collect();
    for entry in entries.flatten() {
        let path = entry.path();
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<i64>().ok());
        if !id.is_some_and(|id| keep.contains(&id)) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// What a response to a shown notification asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Closed, expired, or an action this app did not offer.
    Nothing,
    /// Clicked, but the notification belongs to no anime: only bring the window forward.
    Focus,
    /// Clicked: bring the window forward and open this anime.
    OpenAnime(i64),
}

/// Maps the desktop's response to the notification shown for `media_id`.
pub fn click_for(media_id: i64, response: &NotificationResponse) -> Click {
    let clicked = match response {
        NotificationResponse::Default => true,
        NotificationResponse::Action(key) => key == OPEN_ACTION,
        _ => false,
    };
    match (clicked, media_id > 0) {
        (false, _) => Click::Nothing,
        (true, true) => Click::OpenAnime(media_id),
        (true, false) => Click::Focus,
    }
}

fn act_on_click(app: &tauri::AppHandle, click: Click) {
    let media_id = match click {
        Click::Nothing => return,
        Click::Focus => None,
        Click::OpenAnime(media_id) => Some(media_id),
    };
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    if let Some(media_id) = media_id {
        if let Err(e) = app.emit_to(MAIN_WINDOW, OPEN_ANIME_EVENT, media_id) {
            warn!("Could not open anime {} from its notification: {}", media_id, e);
        }
    }
}

/// Waits until the notification is clicked or closed. Holding `handle` keeps the session-bus
/// connection the click is reported on; `_slot` is given back when the wait ends.
async fn wait_for_click(
    on_click: OnClick,
    handle: NotificationHandle,
    media_id: i64,
    _slot: OwnedSemaphorePermit,
) {
    let mut click = Click::Nothing;
    let waited = tokio::time::timeout(
        KEEP_CLICKABLE,
        handle.wait_for_action_async(|response| click = click_for(media_id, response)),
    )
    .await;
    if waited.is_err() {
        // Nothing listens from here on, so the notification must not stay clickable.
        let _ = tokio::time::timeout(SERVICE_TIMEOUT, handle.close_async()).await;
    }
    on_click(click);
}

/// What a click on a notification does.
pub type OnClick = Arc<dyn Fn(Click) + Send + Sync>;

/// Shows notifications on the desktop, with the cover as the icon when a local copy can be
/// had quickly and the app's own icon otherwise.
pub struct DesktopSink {
    on_click: OnClick,
    client: reqwest::Client,
    cover_dir: Option<PathBuf>,
    /// The product name: what the desktop shows as the sender, and the name of the installed
    /// `.desktop` file (`<product name>.desktop`), which ties the notification to the app.
    app_name: String,
    clickable: Arc<Semaphore>,
}

impl DesktopSink {
    pub fn new(app_name: String, client: reqwest::Client, cover_dir: Option<PathBuf>, on_click: OnClick) -> Self {
        Self {
            on_click,
            client,
            cover_dir,
            app_name,
            clickable: Arc::new(Semaphore::new(MAX_CLICKABLE)),
        }
    }
}

#[async_trait]
impl NotificationSink for DesktopSink {
    async fn show(&self, notification: &ReleaseNotification) {
        let icon = match &self.cover_dir {
            Some(dir) => {
                cover_icon(&self.client, dir, notification.media_id, notification.cover_image.as_deref()).await
            }
            None => None,
        };
        // Without a free slot the notification is shown without a click action, so the
        // desktop never offers a click that nothing would answer.
        let slot = self.clickable.clone().try_acquire_owned().ok();

        let mut popup = notify_rust::Notification::new();
        popup
            .appname(&self.app_name)
            .summary(&notification.title)
            .body(&notification.body_in(crate::i18n::current()))
            .hint(Hint::DesktopEntry(self.app_name.clone()))
            .timeout(Timeout::Milliseconds(POPUP_TIMEOUT_MS));
        match &icon {
            Some(path) => popup.icon(&path.to_string_lossy()),
            // The executable's name, which is also the name of the installed icon.
            None => popup.auto_icon(),
        };
        if slot.is_some() {
            popup.action(OPEN_ACTION, crate::i18n::text(crate::i18n::current(), "open"));
        }

        let handle = match tokio::time::timeout(SERVICE_TIMEOUT, popup.show_async()).await {
            Ok(Ok(handle)) => handle,
            Ok(Err(e)) => {
                warn!("Could not show the release notification for {}: {}", notification.title, e);
                return;
            }
            Err(_) => {
                warn!(
                    "Could not show the release notification for {}: the notification service did not answer",
                    notification.title
                );
                return;
            }
        };
        if let Some(slot) = slot {
            tokio::spawn(wait_for_click(self.on_click.clone(), handle, notification.media_id, slot));
        }
    }
}

static STARTED: AtomicBool = AtomicBool::new(false);

/// Makes the notifier look at the bells and the setting again right away.
pub fn replan() {
    BELLS_CHANGED.notify_one();
}

/// Starts the background task. Call it once, after `AppState` is managed.
pub fn start(app: tauri::AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(run(app));
}

/// The settings file's object; an unreadable file counts as empty, so every setting has its
/// default (release notifications on).
fn read_settings(settings_path: Option<&Path>) -> Value {
    settings_path
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({}))
}

/// Where notification icons are kept.
pub fn cover_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join(db::APP_DIR_NAME).join("notification-covers"))
}

async fn run(app: tauri::AppHandle) {
    let (pool, client, catalogue, plugins) = match app.try_state::<AppState>() {
        Some(state) => (state.pool.clone(), state.client.clone(), state.catalogue.clone(), state.plugins.clone()),
        None => {
            error!("Release notifications cannot start: the app state is not available");
            STARTED.store(false, Ordering::SeqCst);
            return;
        }
    };

    // The app does the checking while it runs; `--background` stands down until it quits.
    let _owner = crate::system::background::claim_for_app().await;

    let app_name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| app.package_info().name.clone());
    let on_click: OnClick = Arc::new(move |click| act_on_click(&app, click));
    let sink = DesktopSink::new(app_name, client, cover_dir(), on_click);

    #[cfg(debug_assertions)]
    show_test_notification(&pool, &sink).await;

    // AniList's schedule, or Shikimori's next episodes while AniList is unavailable.
    drive(&pool, &*catalogue, extension_probe(plugins), &sink, || true).await;
}

/// The release checks' view of the installed extensions.
pub fn extension_probe(plugins: crate::plugins::registry::PluginRegistry) -> Arc<dyn release_check::ReleaseProbe> {
    Arc::new(release_check::ExtensionProbe::new(
        Arc::new(move || plugins.providers()),
        Arc::new(crate::commands::settings::language_prefs),
    ))
}

/// Runs the notifier and the release checks for good. `may_check` is asked before every
/// step: while it says no, nothing is checked (another process is checking), and the state is
/// read again from the database when it says yes again.
pub async fn drive(
    pool: &SqlitePool,
    source: &dyn ScheduleSource,
    probe: Arc<dyn release_check::ReleaseProbe>,
    sink: &dyn NotificationSink,
    mut may_check: impl FnMut() -> bool,
) {
    let load = || async {
        let mut logged = false;
        loop {
            match Notifier::load(pool).await {
                Ok(notifier) => break notifier,
                Err(e) => {
                    if !logged {
                        warn!("Release notifications cannot read their state yet: {}", e);
                        logged = true;
                    }
                    tokio::time::sleep(Duration::from_secs(STORAGE_RETRY_S as u64)).await;
                }
            }
        }
    };
    let mut notifier = load().await;

    info!("Release notifications are running");
    let settings_path = db::get_config_dir().ok().map(|dir| dir.join("settings.json"));
    if let (Some(dir), Ok(bells)) = (cover_dir(), db::list_notification_bells(pool).await) {
        prune_covers(&dir, &bells);
    }
    if let Err(e) = release_check::prune(pool, db::current_unix_timestamp()).await {
        warn!("Old release checks could not be pruned: {}", e);
    }

    let mut standing_by = false;
    loop {
        if !may_check() {
            if !standing_by {
                info!("Release notifications stand by: Anix is open and does the checking");
                standing_by = true;
            }
            tokio::time::sleep(Duration::from_secs(WATCH_TICK_S as u64)).await;
            continue;
        }
        if standing_by {
            info!("Release notifications take over again");
            notifier = load().await;
            standing_by = false;
        }

        let settings = read_settings(settings_path.as_deref());
        notifier.set_release_checks(Some(ReleaseChecks {
            probe: probe.clone(),
            settings: release_check::CheckSettings::from_settings(&settings),
            prefs: crate::providers::languages::LanguagePrefs::from_settings(&settings),
        }));
        let outcome = notifier
            .step(pool, source, sink, release_notifications_enabled(&settings), db::current_unix_timestamp())
            .await;

        let sleep_s = sleep_seconds(outcome, db::current_unix_timestamp());
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(sleep_s)) => {}
            _ = BELLS_CHANGED.notified() => {
                // Lets a quick run of bell changes settle into one schedule read.
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        }
    }
}

/// Debug builds only: with `ANIX_TEST_NOTIFICATION` (or the earlier `ANILILI_TEST_NOTIFICATION`) set, shows one sample notification at
/// startup (for the first bell, if there is one) without recording anything.
#[cfg(debug_assertions)]
async fn show_test_notification(pool: &SqlitePool, sink: &dyn NotificationSink) {
    if crate::env_var_os("ANIX_TEST_NOTIFICATION", "ANILILI_TEST_NOTIFICATION").is_none() {
        return;
    }
    let bell = db::list_notification_bells(pool)
        .await
        .ok()
        .and_then(|bells| bells.into_iter().next());
    let (media_id, title, cover_image) = match bell {
        Some(bell) => (bell.media_id, bell.media_title, bell.cover_image),
        None => (0, "Anix test notification".to_string(), None),
    };
    sink.show(&ReleaseNotification {
        media_id,
        title,
        cover_image,
        episodes: vec![Airing { media_id, episode: 1, airing_at: db::current_unix_timestamp() }],
        note: None,
    })
    .await;
}
