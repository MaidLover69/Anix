//! Pushing watch progress, watchlist additions and list statuses to the connected lists.
//!
//! What has to be pushed is first written to a small queue table, then sent. A push that fails
//! stays queued and is tried again at the next opportunity: the next watched episode, a
//! sign-in, or the next start of the app. Progress recorded while an account is not connected
//! waits in the queue until it is.

use super::comick::{self, ComickEntry, ComickProgress, ComickWrite};
use super::oauth::Service;
use super::session::{self, Context, Notice};
use crate::catalogue::ids::{self, AnimeKey, Site};
use crate::clients::anilist::{parse_media_hint, ListEntryState, MediaListContext};
use crate::db;
use crate::error::AppError;
use crate::models::{HistoryEntry, MediaListEntry};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

/// A queued push that keeps being refused is dropped after this many refusals. Being offline
/// or rate limited does not count.
pub const MAX_ATTEMPTS: i64 = 10;

pub const LIST_STATUSES: [&str; 6] = [
    "CURRENT",
    "PLANNING",
    "COMPLETED",
    "PAUSED",
    "DROPPED",
    "REPEATING",
];

// ---- What to send -------------------------------------------------------------------------

/// The list progress an episode number stands for. Specials numbered between episodes (5.5)
/// and anything below episode 1 are not list progress.
pub fn whole_episode(episode_number: f64) -> Option<i32> {
    let in_range = episode_number.is_finite()
        && episode_number >= 1.0
        && episode_number <= i32::MAX as f64
        && episode_number.fract() == 0.0;
    in_range.then_some(episode_number as i32)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressUpdate {
    pub progress: i32,
    /// AniList vocabulary; `None` leaves the entry's status as it is.
    pub status: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    InvalidProgress,
    /// The list already has this progress or more.
    AlreadyAtOrAhead,
    /// The entry is completed; watching an episode again does not reopen it.
    RemoteCompleted,
    /// A watchlist addition for an anime that is already on the list.
    AlreadyListed,
    /// The anime has no MyAnimeList id on AniList.
    NoMalId,
    /// AniList has no such anime.
    UnknownAnime,
    /// No Comick title carries this anime's AniList (or MyAnimeList) id.
    NoComickTitle,
    /// The Comick connection was granted without `library:write`.
    NoWriteAccess,
    /// The remote progress is a label that is not a number; it is left as it is.
    UnknownRemoteProgress,
}

/// Decides what a watched episode changes on a list. Progress is never lowered and a status
/// the user chose (REPEATING, PAUSED, DROPPED) is kept; a new or PLANNING entry becomes
/// CURRENT, and the final episode of a finished show makes it COMPLETED.
pub fn plan_progress_update(
    current: Option<&ListEntryState>,
    watched: i32,
    total_episodes: Option<i32>,
    show_finished: bool,
) -> Result<ProgressUpdate, SkipReason> {
    if watched < 1 {
        return Err(SkipReason::InvalidProgress);
    }
    let total = total_episodes.filter(|total| *total > 0);
    let progress = total.map_or(watched, |total| watched.min(total));

    if let Some(current) = current {
        if current.status == "COMPLETED" {
            return Err(SkipReason::RemoteCompleted);
        }
        if progress <= current.progress {
            return Err(SkipReason::AlreadyAtOrAhead);
        }
    }

    let completed = show_finished && total.is_some_and(|total| progress >= total);
    let status = match current.map(|entry| entry.status.as_str()) {
        None | Some("PLANNING") => Some(if completed { "COMPLETED" } else { "CURRENT" }),
        Some("CURRENT") if completed => Some("COMPLETED"),
        _ => None,
    };
    Ok(ProgressUpdate { progress, status })
}

/// Whether the last episode of `media` can complete a list entry: the show has finished
/// airing, or AniList does not say.
fn show_finished(media: &MediaListContext) -> bool {
    matches!(media.media_status.as_deref(), Some("FINISHED") | None)
}

pub fn mal_status(anilist_status: &str) -> &'static str {
    match anilist_status {
        "CURRENT" | "REPEATING" => "watching",
        "COMPLETED" => "completed",
        "PAUSED" => "on_hold",
        "DROPPED" => "dropped",
        _ => "plan_to_watch",
    }
}

pub fn anilist_status_from_mal(status: Option<&str>, is_rewatching: bool) -> &'static str {
    if is_rewatching {
        return "REPEATING";
    }
    match status {
        Some("watching") => "CURRENT",
        Some("completed") => "COMPLETED",
        Some("on_hold") => "PAUSED",
        Some("dropped") => "DROPPED",
        _ => "PLANNING",
    }
}

/// The MyAnimeList id behind a stand-in id (an anime the catalogue knows by its MyAnimeList id
/// only). Stand-ins have no AniList entry, so they sync to MyAnimeList alone.
pub fn stand_in_mal_id(anilist_id: i32) -> Option<i32> {
    match ids::decode(anilist_id as i64) {
        Some(AnimeKey::Other(Site::Mal, mal_id)) => i32::try_from(mal_id).ok(),
        _ => None,
    }
}

/// The services an anime can be pushed to: all of them for an AniList id, MyAnimeList for a
/// stand-in that carries a MyAnimeList id, none otherwise.
fn services_for(anilist_id: i32) -> &'static [Service] {
    if anilist_id > 0 {
        &Service::ALL
    } else if stand_in_mal_id(anilist_id).is_some() {
        &[Service::Mal]
    } else {
        &[]
    }
}

// ---- Queue --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKind {
    /// An episode was watched; `progress` is its number.
    Progress,
    /// The anime was added to the in-app watchlist.
    Planning,
}

impl PendingKind {
    fn key(self) -> &'static str {
        match self {
            PendingKind::Progress => "progress",
            PendingKind::Planning => "planning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSync {
    pub service: Service,
    pub anilist_id: i32,
    pub kind: PendingKind,
    pub progress: i32,
    pub attempts: i64,
    pub last_error: Option<String>,
}

/// The queue lives in its own table, created on first use.
async fn ensure_queue(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS account_sync_queue (
            service     TEXT NOT NULL,
            anilist_id  INTEGER NOT NULL,
            kind        TEXT NOT NULL,
            progress    INTEGER NOT NULL DEFAULT 0,
            attempts    INTEGER NOT NULL DEFAULT 0,
            last_error  TEXT,
            updated_at  INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (service, anilist_id, kind)
        );
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Queues one push. A later episode of the same anime replaces an earlier one; an earlier
/// episode never replaces a later one.
pub async fn enqueue(
    pool: &SqlitePool,
    service: Service,
    anilist_id: i32,
    kind: PendingKind,
    progress: i32,
) -> Result<(), AppError> {
    ensure_queue(pool).await?;
    sqlx::query(
        r#"
        INSERT INTO account_sync_queue (service, anilist_id, kind, progress, attempts, last_error, updated_at)
        VALUES (?1, ?2, ?3, ?4, 0, NULL, ?5)
        ON CONFLICT(service, anilist_id, kind) DO UPDATE SET
            attempts = CASE WHEN excluded.progress > account_sync_queue.progress THEN 0 ELSE account_sync_queue.attempts END,
            last_error = CASE WHEN excluded.progress > account_sync_queue.progress THEN NULL ELSE account_sync_queue.last_error END,
            progress = MAX(account_sync_queue.progress, excluded.progress),
            updated_at = excluded.updated_at;
        "#,
    )
    .bind(service.key())
    .bind(anilist_id)
    .bind(kind.key())
    .bind(progress)
    .bind(db::current_unix_millis())
    .execute(pool)
    .await?;
    Ok(())
}

/// Queues a watched episode for every service this build can sign in to, connected or not.
/// Returns false for an episode that is not list progress.
pub async fn queue_watched_episode(
    pool: &SqlitePool,
    anilist_id: i32,
    episode_number: f64,
) -> Result<bool, AppError> {
    let Some(progress) = whole_episode(episode_number) else {
        return Ok(false);
    };
    let services = services_for(anilist_id);
    if services.is_empty() {
        return Ok(false);
    }
    for service in services.iter().copied().filter(|service| service.available()) {
        enqueue(pool, service, anilist_id, PendingKind::Progress, progress).await?;
    }
    Ok(true)
}

/// Queues "add as planning" for every service that is connected right now.
pub async fn queue_watchlist_addition(ctx: &Context<'_>, anilist_id: i32) -> Result<bool, AppError> {
    let mut queued = false;
    for &service in services_for(anilist_id) {
        if ctx.store.is_connected(service) {
            enqueue(ctx.pool, service, anilist_id, PendingKind::Planning, 0).await?;
            queued = true;
        }
    }
    Ok(queued)
}

/// Forgets a watchlist addition that has not been pushed yet. Nothing is removed remotely.
pub async fn cancel_watchlist_addition(pool: &SqlitePool, anilist_id: i32) -> Result<(), AppError> {
    ensure_queue(pool).await?;
    sqlx::query("DELETE FROM account_sync_queue WHERE anilist_id = ?1 AND kind = 'planning'")
        .bind(anilist_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Queued pushes of one service, oldest first.
pub async fn pending(pool: &SqlitePool, service: Service) -> Result<Vec<PendingSync>, AppError> {
    ensure_queue(pool).await?;
    let rows = sqlx::query(
        r#"
        SELECT anilist_id, kind, progress, attempts, last_error
        FROM account_sync_queue
        WHERE service = ?1
        ORDER BY updated_at ASC, anilist_id ASC
        "#,
    )
    .bind(service.key())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PendingSync {
            service,
            anilist_id: row.get::<i64, _>("anilist_id") as i32,
            kind: if row.get::<String, _>("kind") == "planning" {
                PendingKind::Planning
            } else {
                PendingKind::Progress
            },
            progress: row.get::<i64, _>("progress") as i32,
            attempts: row.get("attempts"),
            last_error: row.get("last_error"),
        })
        .collect())
}

/// Removes a queued push once it is done, unless a later episode was queued meanwhile.
async fn settle(pool: &SqlitePool, item: &PendingSync) -> Result<(), AppError> {
    sqlx::query(
        "DELETE FROM account_sync_queue WHERE service = ?1 AND anilist_id = ?2 AND kind = ?3 AND progress <= ?4",
    )
    .bind(item.service.key())
    .bind(item.anilist_id)
    .bind(item.kind.key())
    .bind(item.progress)
    .execute(pool)
    .await?;
    Ok(())
}

async fn record_refusal(pool: &SqlitePool, item: &PendingSync, error: &str) -> Result<(), AppError> {
    if item.attempts + 1 >= MAX_ATTEMPTS {
        tracing::warn!(
            "giving up on {} sync of anime {} after {} attempts: {}",
            item.service.label(),
            item.anilist_id,
            MAX_ATTEMPTS,
            error
        );
        return settle(pool, item).await;
    }
    sqlx::query(
        r#"
        UPDATE account_sync_queue SET attempts = attempts + 1, last_error = ?5
        WHERE service = ?1 AND anilist_id = ?2 AND kind = ?3 AND progress = ?4
        "#,
    )
    .bind(item.service.key())
    .bind(item.anilist_id)
    .bind(item.kind.key())
    .bind(item.progress)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

async fn forget_progress(pool: &SqlitePool, service: Service, anilist_id: i32) -> Result<(), AppError> {
    ensure_queue(pool).await?;
    sqlx::query(
        "DELETE FROM account_sync_queue WHERE service = ?1 AND anilist_id = ?2 AND kind = 'progress'",
    )
    .bind(service.key())
    .bind(anilist_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ---- Sending ------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Updated(ProgressUpdate),
    Skipped(SkipReason),
}

/// AniList's facts about each anime of one drain, so that pushing the same anime to both
/// services asks AniList once.
type MediaCache = HashMap<i32, Option<MediaListContext>>;

async fn push_to_anilist(
    ctx: &Context<'_>,
    item: &PendingSync,
    cache: &mut MediaCache,
) -> Result<PushOutcome, AppError> {
    let id = item.anilist_id;
    let media = session::with_anilist(ctx, |token| async move {
        ctx.anilist.list_context(Some(&token), id).await
    })
    .await?;
    cache.insert(id, media.clone());
    let Some(media) = media else {
        return Ok(PushOutcome::Skipped(SkipReason::UnknownAnime));
    };

    let update = match item.kind {
        PendingKind::Planning if media.entry.is_some() => {
            return Ok(PushOutcome::Skipped(SkipReason::AlreadyListed));
        }
        PendingKind::Planning => ProgressUpdate {
            progress: 0,
            status: Some("PLANNING"),
        },
        PendingKind::Progress => {
            let plan = plan_progress_update(
                media.entry.as_ref(),
                item.progress,
                media.episodes,
                show_finished(&media),
            );
            match plan {
                Ok(update) => update,
                Err(reason) => return Ok(PushOutcome::Skipped(reason)),
            }
        }
    };

    let progress = (item.kind == PendingKind::Progress).then_some(update.progress);
    session::with_anilist(ctx, |token| async move {
        ctx.anilist
            .save_list_entry(&token, id, update.status.map(String::from), progress, None)
            .await
    })
    .await?;
    Ok(PushOutcome::Updated(update))
}

/// The MyAnimeList id of an anime and what AniList knows about it. A stand-in id carries its
/// MyAnimeList id and is not looked up on AniList (its episode count stays unknown). `None`
/// when there is no MyAnimeList id to push to.
async fn mal_target(
    ctx: &Context<'_>,
    id: i32,
    cache: &mut MediaCache,
) -> Result<Option<(i32, MediaListContext)>, AppError> {
    if id <= 0 {
        return Ok(stand_in_mal_id(id).map(|mal_id| (mal_id, MediaListContext::default())));
    }
    let media = match cache.get(&id) {
        Some(known) => known.clone(),
        None => {
            let fetched = ctx.anilist.list_context(None, id).await?;
            cache.insert(id, fetched.clone());
            fetched
        }
    };
    Ok(media.and_then(|media| media.id_mal.filter(|mal_id| *mal_id > 0).map(|mal_id| (mal_id, media))))
}

async fn push_to_mal(
    ctx: &Context<'_>,
    item: &PendingSync,
    cache: &mut MediaCache,
) -> Result<PushOutcome, AppError> {
    let id = item.anilist_id;
    let Some((mal_id, media)) = mal_target(ctx, id, cache).await? else {
        return Ok(PushOutcome::Skipped(SkipReason::NoMalId));
    };

    let current = session::with_mal(ctx, |token| async move {
        ctx.mal.get_list_status(&token, mal_id).await
    })
    .await?
    .map(|entry| ListEntryState {
        status: anilist_status_from_mal(entry.status.as_deref(), entry.is_rewatching).to_string(),
        progress: entry.num_episodes_watched,
    });

    let update = match item.kind {
        PendingKind::Planning if current.is_some() => {
            return Ok(PushOutcome::Skipped(SkipReason::AlreadyListed));
        }
        PendingKind::Planning => ProgressUpdate {
            progress: 0,
            status: Some("PLANNING"),
        },
        PendingKind::Progress => {
            let plan = plan_progress_update(
                current.as_ref(),
                item.progress,
                media.episodes,
                show_finished(&media),
            );
            match plan {
                Ok(update) => update,
                Err(reason) => return Ok(PushOutcome::Skipped(reason)),
            }
        }
    };

    let progress = (item.kind == PendingKind::Progress).then_some(update.progress);
    let status = update.status.map(mal_status);
    let confirmed = session::with_mal(ctx, |token| async move {
        ctx.mal.update_list_status(&token, mal_id, progress, status).await
    })
    .await?;
    if let Some(requested) = progress {
        if confirmed.num_episodes_watched != requested {
            return Err(AppError::Other(format!(
                "MyAnimeList confirmed {} watched episodes after {} were sent",
                confirmed.num_episodes_watched, requested
            )));
        }
    }
    Ok(PushOutcome::Updated(update))
}

// ---- Comick titles --------------------------------------------------------------------------

/// A title that Comick did not have is looked for again after this long.
pub const COMICK_MISS_RECHECK_S: i64 = 7 * 24 * 3600;

async fn ensure_comick_titles(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS comick_titles (
            anilist_id  INTEGER PRIMARY KEY,
            hid         TEXT,
            checked_at  INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// What is known about the Comick title of an anime: `None` when it was never looked up (or a
/// miss is due for another look), `Some(None)` when Comick recently had no match.
pub async fn known_comick_hid(pool: &SqlitePool, anilist_id: i32) -> Result<Option<Option<String>>, AppError> {
    ensure_comick_titles(pool).await?;
    let row = sqlx::query("SELECT hid, checked_at FROM comick_titles WHERE anilist_id = ?1")
        .bind(anilist_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let hid: Option<String> = row.get("hid");
    let checked_at: i64 = row.get("checked_at");
    match hid {
        Some(hid) => Ok(Some(Some(hid))),
        None if checked_at + COMICK_MISS_RECHECK_S > db::current_unix_timestamp() => Ok(Some(None)),
        None => Ok(None),
    }
}

pub async fn remember_comick_hid(pool: &SqlitePool, anilist_id: i32, hid: Option<&str>) -> Result<(), AppError> {
    ensure_comick_titles(pool).await?;
    sqlx::query(
        r#"
        INSERT INTO comick_titles (anilist_id, hid, checked_at) VALUES (?1, ?2, ?3)
        ON CONFLICT(anilist_id) DO UPDATE SET hid = excluded.hid, checked_at = excluded.checked_at;
        "#,
    )
    .bind(anilist_id)
    .bind(hid)
    .bind(db::current_unix_timestamp())
    .execute(pool)
    .await?;
    Ok(())
}

/// How many of an anime's titles are searched on Comick before giving up.
const COMICK_SEARCHES: usize = 3;

/// The Comick HID of an AniList anime: from the cache, or by searching Comick for its titles
/// and keeping the result whose external AniList id (or MyAnimeList id) is this anime.
pub async fn comick_hid(ctx: &Context<'_>, anilist_id: i32, id_mal: Option<i32>) -> Result<Option<String>, AppError> {
    if let Some(known) = known_comick_hid(ctx.pool, anilist_id).await? {
        return Ok(known);
    }
    let media = ctx.anilist.fetch_details(anilist_id as i64).await?;
    let hint = parse_media_hint(&media);
    let id_mal = id_mal.or(hint.id_mal.map(|id| id as i32));

    for title in hint.titles.iter().take(COMICK_SEARCHES) {
        let results = session::with_comick(ctx, |token| async move {
            ctx.comick.search_anime(&token, title).await
        })
        .await?;
        if let Some(hid) = comick::match_hid(&results, anilist_id, id_mal) {
            remember_comick_hid(ctx.pool, anilist_id, Some(&hid)).await?;
            return Ok(Some(hid));
        }
    }
    remember_comick_hid(ctx.pool, anilist_id, None).await?;
    Ok(None)
}

async fn push_to_comick(
    ctx: &Context<'_>,
    item: &PendingSync,
    cache: &mut MediaCache,
) -> Result<PushOutcome, AppError> {
    let scope = ctx.store.comick().and_then(|session| session.scope);
    if !comick::scope_allows_writes(scope.as_deref()) {
        return Ok(PushOutcome::Skipped(SkipReason::NoWriteAccess));
    }

    let id = item.anilist_id;
    let media = match cache.get(&id) {
        Some(known) => known.clone(),
        None => {
            let fetched = ctx.anilist.list_context(None, id).await?;
            cache.insert(id, fetched.clone());
            fetched
        }
    };
    let Some(media) = media else {
        return Ok(PushOutcome::Skipped(SkipReason::UnknownAnime));
    };
    let Some(hid) = comick_hid(ctx, id, media.id_mal.filter(|mal_id| *mal_id > 0)).await? else {
        return Ok(PushOutcome::Skipped(SkipReason::NoComickTitle));
    };

    let remote: Option<ComickEntry> = session::with_comick(ctx, |token| {
        let hid = hid.clone();
        async move { ctx.comick.get_entry(&token, &hid).await }
    })
    .await?;
    let current = match remote {
        None => None,
        Some(entry) => match comick::progress_episodes(entry.progress.as_ref()) {
            Ok(progress) => Some(ListEntryState {
                status: comick::anilist_status_from_comick(entry.status).to_string(),
                progress,
            }),
            Err(()) if item.kind == PendingKind::Planning => {
                return Ok(PushOutcome::Skipped(SkipReason::AlreadyListed));
            }
            Err(()) => return Ok(PushOutcome::Skipped(SkipReason::UnknownRemoteProgress)),
        },
    };

    let update = match item.kind {
        PendingKind::Planning if current.is_some() => {
            return Ok(PushOutcome::Skipped(SkipReason::AlreadyListed));
        }
        PendingKind::Planning => ProgressUpdate {
            progress: 0,
            status: Some("PLANNING"),
        },
        PendingKind::Progress => {
            let plan = plan_progress_update(
                current.as_ref(),
                item.progress,
                media.episodes,
                show_finished(&media),
            );
            match plan {
                Ok(update) => update,
                Err(reason) => return Ok(PushOutcome::Skipped(reason)),
            }
        }
    };

    let write = ComickWrite {
        status: update.status.map(comick::comick_status),
        progress: (item.kind == PendingKind::Progress).then(|| ComickProgress {
            number: Some(update.progress.to_string()),
        }),
    };
    session::with_comick(ctx, |token| {
        let hid = hid.clone();
        let write = write.clone();
        async move { ctx.comick.put_entry(&token, &hid, &write).await }
    })
    .await?;
    Ok(PushOutcome::Updated(update))
}

/// The connected user's Comick anime library, every page of it.
pub async fn comick_library(ctx: &Context<'_>) -> Result<Vec<ComickEntry>, AppError> {
    const MAX_PAGES: usize = 50;
    let mut entries = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let page = session::with_comick(ctx, |token| {
            let cursor = cursor.clone();
            async move { ctx.comick.library_page(&token, cursor.as_deref()).await }
        })
        .await?;
        entries.extend(page.data);
        match page.next_cursor {
            Some(next) if !next.is_empty() => cursor = Some(next),
            _ => break,
        }
    }
    Ok(entries)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrainReport {
    pub updated: usize,
    pub skipped: usize,
    /// Pushes that failed and stay queued.
    pub failed: usize,
}

static DRAIN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Sends everything queued for the connected services. One drain runs at a time. Nothing is
/// returned as an error: a push that fails is logged and stays queued.
pub async fn drain(ctx: &Context<'_>) -> DrainReport {
    let _guard = DRAIN.lock().await;
    let mut report = DrainReport::default();
    let mut cache = MediaCache::new();

    for service in Service::ALL {
        if !ctx.store.is_connected(service) {
            continue;
        }
        let items = match pending(ctx.pool, service).await {
            Ok(items) => items,
            Err(e) => {
                tracing::error!("could not read the {} sync queue: {}", service.label(), e);
                continue;
            }
        };

        for item in items {
            let result = match service {
                Service::AniList => push_to_anilist(ctx, &item, &mut cache).await,
                Service::Mal => push_to_mal(ctx, &item, &mut cache).await,
                Service::Comick => push_to_comick(ctx, &item, &mut cache).await,
            };
            let stored = match &result {
                Ok(PushOutcome::Updated(update)) => {
                    tracing::info!(
                        "{} list updated: anime {} progress {} status {}",
                        service.label(),
                        item.anilist_id,
                        update.progress,
                        update.status.unwrap_or("unchanged")
                    );
                    report.updated += 1;
                    ctx.notifier.notify(Notice::Synced {
                        service,
                        anilist_id: item.anilist_id,
                    });
                    settle(ctx.pool, &item).await
                }
                Ok(PushOutcome::Skipped(reason)) => {
                    tracing::info!(
                        "{} list left as it is for anime {} ({:?})",
                        service.label(),
                        item.anilist_id,
                        reason
                    );
                    report.skipped += 1;
                    settle(ctx.pool, &item).await
                }
                // Signed out, offline or rate limited: the rest would fail the same way.
                Err(e @ (AppError::AuthRequired | AppError::Unreachable(_) | AppError::RateLimited { .. })) => {
                    tracing::warn!("{} sync postponed: {}", service.label(), e);
                    report.failed += 1;
                    break;
                }
                Err(e) => {
                    tracing::warn!(
                        "{} sync of anime {} failed: {}",
                        service.label(),
                        item.anilist_id,
                        e
                    );
                    report.failed += 1;
                    record_refusal(ctx.pool, &item, &e.to_string()).await
                }
            };
            if let Err(e) = stored {
                tracing::error!("could not update the {} sync queue: {}", service.label(), e);
            }
        }
    }
    push_statuses(ctx, &mut cache, &mut report).await;
    report
}

// ---- List statuses --------------------------------------------------------------------------
//
// The watchlist keeps a status per anime. With AniList or MyAnimeList connected the tracker
// wins: its status is read back into the watchlist and a status chosen here is sent to it.
// Without an account, or while the tracker cannot be reached, the status chosen here is kept
// with `synced = 0` and sent by a later drain.

/// The watchlist's vocabulary: AniList's, without REPEATING (a rewatch is "Watching" here).
pub fn local_status(remote: &str) -> &'static str {
    match remote {
        "CURRENT" | "REPEATING" => "CURRENT",
        "COMPLETED" => "COMPLETED",
        "PAUSED" => "PAUSED",
        "DROPPED" => "DROPPED",
        _ => "PLANNING",
    }
}

/// Failures that say "try again later" rather than "this will never work".
fn is_transient(error: &AppError) -> bool {
    matches!(error, AppError::AuthRequired) || crate::offline::is_offline_error(error)
}

/// Sends one status to one service. `Ok(false)` when the service has nowhere to put it.
async fn push_status(
    ctx: &Context<'_>,
    service: Service,
    id: i32,
    status: &str,
    cache: &mut MediaCache,
) -> Result<bool, AppError> {
    match service {
        Service::AniList if id > 0 => {
            let status = status.to_string();
            session::with_anilist(ctx, |token| {
                let status = status.clone();
                async move { ctx.anilist.save_list_entry(&token, id, Some(status), None, None).await }
            })
            .await?;
            Ok(true)
        }
        Service::Mal => {
            let Some((mal_id, _)) = mal_target(ctx, id, cache).await? else {
                return Ok(false);
            };
            let value = mal_status(status);
            session::with_mal(ctx, |token| async move {
                ctx.mal.update_list_status(&token, mal_id, None, Some(value)).await
            })
            .await?;
            Ok(true)
        }
        // Comick and stand-ins on AniList: statuses stay local there.
        _ => Ok(false),
    }
}

/// Sends the statuses chosen here that the connected lists do not have yet. A status counts as
/// sent once every connected list took it (or has nowhere to put it); one that cannot be sent
/// for now stays unsynced for the next drain.
async fn push_statuses(ctx: &Context<'_>, cache: &mut MediaCache, report: &mut DrainReport) {
    let connected: Vec<Service> = [Service::AniList, Service::Mal]
        .into_iter()
        .filter(|service| ctx.store.is_connected(*service))
        .collect();
    if connected.is_empty() {
        return;
    }
    let waiting = match db::unsynced_watchlist(ctx.pool).await {
        Ok(waiting) => waiting,
        Err(e) => {
            tracing::error!("could not read the unsynced list statuses: {}", e);
            return;
        }
    };
    let mut down: Vec<Service> = Vec::new();
    for (id, status) in waiting {
        let mut done = true;
        for &service in &connected {
            if down.contains(&service) {
                done = false;
                continue;
            }
            match push_status(ctx, service, id, &status, cache).await {
                Ok(true) => {
                    report.updated += 1;
                    ctx.notifier.notify(Notice::Synced { service, anilist_id: id });
                }
                Ok(false) => report.skipped += 1,
                Err(e) if is_transient(&e) => {
                    tracing::warn!("{} status sync postponed: {}", service.label(), e);
                    report.failed += 1;
                    down.push(service);
                    done = false;
                }
                // Refused for good (unknown anime and the like): kept here, not retried.
                Err(e) => {
                    tracing::warn!("{} refused the status of anime {}: {}", service.label(), id, e);
                    report.skipped += 1;
                }
            }
        }
        if done {
            if let Err(e) = db::mark_watchlist_synced(ctx.pool, id, &status).await {
                tracing::error!("could not mark the status of anime {} as sent: {}", id, e);
            }
        }
    }
}

/// Chooses a status for one anime: stored with `synced = 0`, then sent by the next drain
/// (right away when a list is connected).
#[allow(clippy::too_many_arguments)]
pub async fn set_status(
    pool: &SqlitePool,
    anilist_id: i32,
    title: &str,
    cover: Option<&str>,
    format: Option<&str>,
    average_score: Option<i32>,
    status: &str,
) -> Result<(), AppError> {
    db::set_watchlist_status(pool, anilist_id, title, cover, format, average_score, status, false).await?;
    // The status replaces a queued "add as planning".
    cancel_watchlist_addition(pool, anilist_id).await
}

/// The status shown for one anime and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListStatus {
    pub status: Option<String>,
    /// False while a status chosen here waits to be sent.
    pub synced: bool,
    /// "anilist", "myanimelist" or "local".
    pub source: &'static str,
}

/// The status of one anime: the connected tracker's when it has the anime (and nothing chosen
/// here is waiting to be sent), else the watchlist's. A tracker that cannot be reached falls
/// back to the watchlist.
pub async fn list_status(ctx: &Context<'_>, anilist_id: i32) -> Result<ListStatus, AppError> {
    let local = db::get_watchlist_status(ctx.pool, anilist_id).await?;
    let local_view = |source| ListStatus {
        status: local.as_ref().map(|(status, _)| status.clone()),
        synced: local.as_ref().is_none_or(|(_, synced)| *synced),
        source,
    };
    if local.as_ref().is_some_and(|(_, synced)| !synced) {
        return Ok(local_view("local"));
    }

    let remote = remote_status(ctx, anilist_id).await;
    let (status, source) = match remote {
        Ok(Some(found)) => found,
        Ok(None) => return Ok(local_view("local")),
        Err(e) => {
            tracing::info!("list status of anime {} read from the watchlist: {}", anilist_id, e);
            return Ok(local_view("local"));
        }
    };
    db::take_remote_status(ctx.pool, anilist_id, status).await?;
    Ok(ListStatus {
        status: Some(status.to_string()),
        synced: true,
        source,
    })
}

/// The tracker's status of one anime in watchlist vocabulary, AniList first.
async fn remote_status(ctx: &Context<'_>, id: i32) -> Result<Option<(&'static str, &'static str)>, AppError> {
    if id > 0 && ctx.store.is_connected(Service::AniList) {
        let media = session::with_anilist(ctx, |token| async move {
            ctx.anilist.list_context(Some(&token), id).await
        })
        .await?;
        return Ok(media
            .and_then(|media| media.entry)
            .map(|entry| (local_status(&entry.status), "anilist")));
    }
    if ctx.store.is_connected(Service::Mal) {
        let mut cache = MediaCache::new();
        let Some((mal_id, _)) = mal_target(ctx, id, &mut cache).await? else {
            return Ok(None);
        };
        let entry = session::with_mal(ctx, |token| async move {
            ctx.mal.get_list_status(&token, mal_id).await
        })
        .await?;
        return Ok(entry.map(|entry| {
            let status = anilist_status_from_mal(entry.status.as_deref(), entry.is_rewatching);
            (local_status(status), "myanimelist")
        }));
    }
    Ok(None)
}

/// Reads the connected AniList list into the watchlist: every anime on both takes AniList's
/// status, except one whose status chosen here is still waiting to be sent. Returns how many
/// statuses changed.
pub async fn pull_statuses(ctx: &Context<'_>) -> Result<usize, AppError> {
    if !ctx.store.is_connected(Service::AniList) {
        return Ok(0);
    }
    let user_id = session::anilist_viewer_id(ctx).await?;
    let entries = session::with_anilist(ctx, |token| async move {
        ctx.anilist.get_user_library(&token, user_id, None).await
    })
    .await?;
    let mut changed = 0;
    for entry in entries {
        let Some(status) = entry.status.as_deref() else { continue };
        if db::take_remote_status(ctx.pool, entry.id, local_status(status)).await? {
            changed += 1;
        }
    }
    Ok(changed)
}

// ---- Watch progress -----------------------------------------------------------------------

async fn is_marked_watched(pool: &SqlitePool, anilist_id: i32, episode_number: f64) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT completed FROM watch_history WHERE anilist_id = ?1 AND episode_number = ?2",
    )
    .bind(anilist_id)
    .bind(episode_number)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some_and(|completed| completed != 0)
}

/// Saves watch progress and reports whether this save is the one that made the episode count
/// as watched.
pub async fn save_watch_progress(pool: &SqlitePool, entry: &HistoryEntry) -> Result<bool, AppError> {
    let watched_before = is_marked_watched(pool, entry.anilist_id, entry.episode_number).await;
    db::update_watch_progress(pool, entry).await?;
    let watched_now = db::is_episode_watched(entry.position_ms.max(0), entry.duration_ms.max(0));
    Ok(watched_now && !watched_before)
}

// ---- Editing an entry by hand -------------------------------------------------------------

/// What happened to the MyAnimeList copy of an entry edited in the AniList tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MalMirror {
    NotConnected,
    NoMalId,
    Saved,
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SavedEntry {
    pub entry: MediaListEntry,
    pub mal: MalMirror,
}

/// Saves the status and progress the user chose for one anime to AniList and, when
/// MyAnimeList is connected and knows the anime, to MyAnimeList as well. Unlike the sync of
/// watched episodes this writes exactly what was asked for, including a lower progress.
pub async fn save_entry(
    ctx: &Context<'_>,
    media_id: i32,
    status: Option<String>,
    progress: Option<i32>,
) -> Result<SavedEntry, AppError> {
    if let Some(status) = status.as_deref() {
        if !LIST_STATUSES.contains(&status) {
            return Err(AppError::Other(format!("Unknown list status: {}", status)));
        }
    }

    let media = session::with_anilist(ctx, |token| async move {
        ctx.anilist.list_context(Some(&token), media_id).await
    })
    .await?
    .ok_or_else(|| AppError::Other(format!("AniList has no anime with id {}", media_id)))?;

    let total = media.episodes.filter(|total| *total > 0);
    let progress = progress.map(|value| total.map_or(value.max(0), |total| value.clamp(0, total)));

    let entry = session::with_anilist(ctx, |token| {
        let status = status.clone();
        async move {
            ctx.anilist
                .save_list_entry(&token, media_id, status, progress, None)
                .await
        }
    })
    .await?;
    if let Err(e) = forget_progress(ctx.pool, Service::AniList, media_id).await {
        tracing::warn!("could not clear queued AniList progress: {}", e);
    }
    if let Some(saved) = entry.status.as_deref() {
        if let Err(e) = db::take_remote_status(ctx.pool, media_id, local_status(saved)).await {
            tracing::warn!("could not update the watchlist status of anime {}: {}", media_id, e);
        }
    }

    let mal = if !ctx.store.is_connected(Service::Mal) {
        MalMirror::NotConnected
    } else if let Some(mal_id) = media.id_mal.filter(|mal_id| *mal_id > 0) {
        let mal_status_value = status.as_deref().map(mal_status);
        let mirrored = session::with_mal(ctx, |token| async move {
            ctx.mal
                .update_list_status(&token, mal_id, progress, mal_status_value)
                .await
        })
        .await;
        match mirrored {
            Ok(_) => {
                if let Err(e) = forget_progress(ctx.pool, Service::Mal, media_id).await {
                    tracing::warn!("could not clear queued MyAnimeList progress: {}", e);
                }
                MalMirror::Saved
            }
            Err(e) => {
                tracing::warn!("MyAnimeList copy of anime {} not saved: {}", media_id, e);
                MalMirror::Failed {
                    message: session::error_message(Service::Mal, true, &e),
                }
            }
        }
    } else {
        MalMirror::NoMalId
    };

    Ok(SavedEntry { entry, mal })
}
