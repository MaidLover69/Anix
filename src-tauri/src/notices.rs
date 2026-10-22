//! The in-app notification centre: a list of notices (an episode came out, a download
//! failed, …) with a read flag, shown on the Notifications screen and counted on the sidebar.
//!
//! Any backend code adds one with [`push`]; the main window hears [`CHANGED_EVENT`] and
//! reloads the list and the unread count.

use crate::db;
use crate::error::AppError;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection, SqlitePool};
use tauri::AppHandle;

/// Kinds the Notifications screen has a tab for; any other kind shows under All only.
pub const AIRING: &str = "airing";
pub const DOWNLOAD: &str = "download";
pub const TEST: &str = "test";

/// Sent to the main window after a notice was added or marked read.
pub const CHANGED_EVENT: &str = "notices-changed";

/// Only the newest notices are kept.
pub const KEEP: i64 = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub body: String,
    /// An in-app route (`/anime/123`) or an `anix://` link; opened when the notice is clicked.
    pub link: Option<String>,
    pub read: bool,
    /// Unix milliseconds.
    pub created_at: i64,
}

/// Creates the table; called from `db::run_migrations`.
pub async fn create_tables(conn: &mut SqliteConnection) -> Result<(), AppError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notices (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            kind        TEXT NOT NULL,
            title       TEXT NOT NULL,
            body        TEXT NOT NULL DEFAULT '',
            link        TEXT,
            read        INTEGER NOT NULL DEFAULT 0,
            created_at  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_notices_created ON notices(created_at DESC);
        "#,
    )
    .execute(conn)
    .await?;
    Ok(())
}

static APP: OnceCell<AppHandle> = OnceCell::new();

/// Lets [`push`] tell the main window; called once at startup.
pub fn init(app: AppHandle) {
    let _ = APP.set(app);
}

fn announce() {
    if let Some(app) = APP.get() {
        crate::accounts::emit(app, CHANGED_EVENT, ());
    }
}

/// Adds an unread notice and returns its id. `kind` is [`AIRING`], [`DOWNLOAD`] or any other
/// short word.
pub async fn push(
    pool: &SqlitePool,
    kind: &str,
    title: &str,
    body: &str,
    link: Option<&str>,
) -> Result<i64, AppError> {
    let id = sqlx::query(
        "INSERT INTO notices (kind, title, body, link, read, created_at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
    )
    .bind(kind)
    .bind(title)
    .bind(body)
    .bind(link)
    .bind(db::current_unix_millis())
    .execute(pool)
    .await?
    .last_insert_rowid();
    sqlx::query("DELETE FROM notices WHERE id <= ?1 - ?2")
        .bind(id)
        .bind(KEEP)
        .execute(pool)
        .await?;
    announce();
    Ok(id)
}

/// Newest first; `kind` limits the list to one kind.
pub async fn list(pool: &SqlitePool, kind: Option<&str>) -> Result<Vec<Notice>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, kind, title, body, link, read, created_at FROM notices
        WHERE ?1 IS NULL OR kind = ?1
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(kind)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Notice {
            id: r.get("id"),
            kind: r.get("kind"),
            title: r.get("title"),
            body: r.get("body"),
            link: r.get("link"),
            read: r.get::<i64, _>("read") != 0,
            created_at: r.get("created_at"),
        })
        .collect())
}

pub async fn unread_count(pool: &SqlitePool) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM notices WHERE read = 0")
        .fetch_one(pool)
        .await?)
}

/// Marks the notices `ids` read, or every notice when `ids` is `None`. Returns how many changed.
pub async fn mark_read(pool: &SqlitePool, ids: Option<&[i64]>) -> Result<u64, AppError> {
    let changed = match ids {
        None => sqlx::query("UPDATE notices SET read = 1 WHERE read = 0")
            .execute(pool)
            .await?
            .rows_affected(),
        Some(ids) => {
            let mut changed = 0;
            for id in ids {
                changed += sqlx::query("UPDATE notices SET read = 1 WHERE id = ?1 AND read = 0")
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected();
            }
            changed
        }
    };
    if changed > 0 {
        announce();
    }
    Ok(changed)
}

pub async fn clear(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query("DELETE FROM notices").execute(pool).await?;
    announce();
    Ok(())
}
