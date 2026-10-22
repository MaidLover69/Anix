//! Saved catalogue answers for when no anime catalogue can be reached.
//!
//! Every successful read the app depends on (home lists, anime details, search results and
//! airing schedules) is written to `anilist_cache`. When a later read of the same thing fails
//! because AniList is unreachable, the saved copy is served instead and the frontend is told
//! with [`OFFLINE_EVENT`], so it can say that it shows saved data. Each kind keeps a bounded
//! number of rows and nothing older than [`MAX_AGE_S`].

use crate::db::current_unix_timestamp;
use crate::error::AppError;
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::sync::Mutex;
use tauri::Emitter;

/// Sent to every window when the app starts or stops serving saved AniList data.
pub const OFFLINE_EVENT: &str = "offline-status";

/// Rows older than this are dropped.
pub const MAX_AGE_S: i64 = 60 * 24 * 3600;

pub const SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS anilist_cache (
        kind        TEXT NOT NULL,
        key         TEXT NOT NULL,
        data        TEXT NOT NULL,
        cached_at   INTEGER NOT NULL,
        PRIMARY KEY (kind, key)
    );
    CREATE INDEX IF NOT EXISTS idx_anilist_cache_age ON anilist_cache(kind, cached_at DESC);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Home,
    Details,
    Search,
    Schedule,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Home, Kind::Details, Kind::Search, Kind::Schedule];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Home => "home",
            Kind::Details => "details",
            Kind::Search => "search",
            Kind::Schedule => "schedule",
        }
    }

    /// How many rows of this kind are kept; the most recently saved ones stay.
    pub fn max_rows(self) -> i64 {
        match self {
            Kind::Home => 2,
            Kind::Details => 400,
            Kind::Search => 40,
            Kind::Schedule => 21,
        }
    }
}

/// Creates the table; called from `db::run_migrations`.
pub async fn create_tables(conn: &mut SqliteConnection) -> Result<(), AppError> {
    sqlx::query(SCHEMA).execute(conn).await?;
    Ok(())
}

/// Saves `data` under `(kind, key)` at `now` and prunes that kind.
pub async fn store_at(pool: &SqlitePool, kind: Kind, key: &str, data: &Value, now: i64) -> Result<(), AppError> {
    let text = serde_json::to_string(data)?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO anilist_cache (kind, key, data, cached_at)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(kind, key) DO UPDATE SET
            data = excluded.data,
            cached_at = excluded.cached_at
        "#,
    )
    .bind(kind.as_str())
    .bind(key)
    .bind(text)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    prune_kind(&mut tx, kind, now).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn store(pool: &SqlitePool, kind: Kind, key: &str, data: &Value) -> Result<(), AppError> {
    store_at(pool, kind, key, data, current_unix_timestamp()).await
}

/// The saved copy and when it was saved (Unix seconds).
pub async fn load(pool: &SqlitePool, kind: Kind, key: &str) -> Result<Option<(Value, i64)>, AppError> {
    let row = sqlx::query("SELECT data, cached_at FROM anilist_cache WHERE kind = ?1 AND key = ?2")
        .bind(kind.as_str())
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|r| {
        let data: String = r.get("data");
        let cached_at: i64 = r.get("cached_at");
        serde_json::from_str::<Value>(&data).ok().map(|value| (value, cached_at))
    }))
}

/// Every saved row of one kind, newest first.
pub async fn load_all(pool: &SqlitePool, kind: Kind) -> Result<Vec<(String, Value, i64)>, AppError> {
    let rows = sqlx::query("SELECT key, data, cached_at FROM anilist_cache WHERE kind = ?1 ORDER BY cached_at DESC")
        .bind(kind.as_str())
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let data: String = r.get("data");
            let value = serde_json::from_str::<Value>(&data).ok()?;
            Some((r.get("key"), value, r.get("cached_at")))
        })
        .collect())
}

async fn prune_kind(conn: &mut SqliteConnection, kind: Kind, now: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM anilist_cache WHERE kind = ?1 AND cached_at < ?2")
        .bind(kind.as_str())
        .bind(now - MAX_AGE_S)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        r#"
        DELETE FROM anilist_cache WHERE kind = ?1 AND key NOT IN (
            SELECT key FROM anilist_cache WHERE kind = ?1 ORDER BY cached_at DESC, key LIMIT ?2
        )
        "#,
    )
    .bind(kind.as_str())
    .bind(kind.max_rows())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Applies the age and size limits to every kind.
pub async fn prune(pool: &SqlitePool, now: i64) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    for kind in Kind::ALL {
        prune_kind(&mut tx, kind, now).await?;
    }
    sqlx::query("DELETE FROM anilist_cache WHERE kind NOT IN ('home', 'details', 'search', 'schedule')")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Failures that mean "AniList is out of reach right now", for which a saved copy is served.
/// A rejected or malformed request is not one of them.
pub fn is_offline_error(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Unreachable(_) | AppError::Network(_) | AppError::RateLimited { .. }
    )
}

/// The cache key of a search: the request in a fixed order, with the text trimmed and in
/// lower case so the same search typed differently finds the same row.
#[allow(clippy::too_many_arguments)]
pub fn search_key(
    query: Option<&str>,
    genres: &[String],
    format: Option<&str>,
    status: Option<&str>,
    sort: Option<&str>,
    page: i32,
    per_page: i32,
    hide_adult: bool,
) -> String {
    let clean = |value: Option<&str>| value.map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty());
    let mut genres: Vec<String> = genres.iter().map(|g| g.trim().to_lowercase()).collect();
    genres.sort();
    serde_json::json!([
        clean(query),
        genres,
        clean(format),
        clean(status),
        clean(sort),
        page,
        per_page,
        hide_adult
    ])
    .to_string()
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
pub struct OfflineStatus {
    /// The last AniList read failed and saved data was (or would have been) used.
    pub offline: bool,
    /// When the saved data that was served last was saved (Unix seconds).
    pub saved_at: Option<i64>,
    /// The catalogue that answered while AniList is unavailable (for example "MyAnimeList").
    pub fallback: Option<&'static str>,
}

static STATUS: Lazy<Mutex<OfflineStatus>> = Lazy::new(|| Mutex::new(OfflineStatus::default()));

pub fn status() -> OfflineStatus {
    *STATUS.lock().unwrap_or_else(|p| p.into_inner())
}

fn set_status(app: &tauri::AppHandle, next: OfflineStatus) {
    let changed = {
        let mut current = STATUS.lock().unwrap_or_else(|p| p.into_inner());
        let changed = *current != next;
        *current = next;
        changed
    };
    if changed {
        if let Err(e) = app.emit(OFFLINE_EVENT, next) {
            tracing::warn!("could not announce the offline state: {}", e);
        }
    }
}

/// An AniList read succeeded.
pub fn mark_online(app: &tauri::AppHandle) {
    mark_served(app, None);
}

/// A catalogue read succeeded; `fallback` names the catalogue when it was not AniList's turn.
pub fn mark_served(app: &tauri::AppHandle, fallback: Option<&'static str>) {
    set_status(app, OfflineStatus { fallback, ..Default::default() });
}

/// An AniList read failed for lack of a connection; `saved_at` is the age of what was served.
pub fn mark_offline(app: &tauri::AppHandle, saved_at: Option<i64>) {
    let previous = status();
    let saved_at = match (previous.saved_at, saved_at) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    set_status(app, OfflineStatus { offline: true, saved_at, fallback: None });
}

/// The airing entries between `from_ts` and `to_ts` found in saved schedules (each saved under
/// its own window), each entry once, in air-time order; with the save time of the oldest
/// schedule used, or `None` when no saved schedule overlaps the window.
pub fn saved_schedule(
    saved: Vec<(String, Value, i64)>,
    from_ts: i64,
    to_ts: i64,
) -> (Vec<crate::models::AiringEntry>, Option<i64>) {
    // One entry per anime and episode; AniList's (positive id) over an estimate (negative).
    let mut seen: std::collections::HashMap<(i64, i32), usize> = std::collections::HashMap::new();
    let mut entries: Vec<crate::models::AiringEntry> = Vec::new();
    let mut saved_at: Option<i64> = None;
    for (_, value, at) in saved {
        let Ok(list) = serde_json::from_value::<Vec<crate::models::AiringEntry>>(value) else {
            continue;
        };
        let mut used = false;
        for entry in list {
            if entry.airing_at < from_ts || entry.airing_at > to_ts {
                continue;
            }
            match seen.get(&(entry.media_id, entry.episode)) {
                Some(&i) if entries[i].id <= 0 && entry.id > 0 => entries[i] = entry,
                Some(_) => continue,
                None => {
                    seen.insert((entry.media_id, entry.episode), entries.len());
                    entries.push(entry);
                }
            }
            used = true;
        }
        if used {
            saved_at = Some(saved_at.map_or(at, |known| known.min(at)));
        }
    }
    entries.sort_by_key(|entry| entry.airing_at);
    (entries, saved_at)
}
