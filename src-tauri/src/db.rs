use crate::error::AppError;
use crate::models::{
    AiringEntry, CustomPlaylist, CustomPlaylistItem, DownloadRecord, HistoryEntry,
    NotificationPreference,
};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteRow, SqliteSynchronous,
};
use sqlx::Row;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

/// An episode counts as watched once playback reached this share of its duration.
pub const WATCHED_THRESHOLD_PERCENT: i64 = 85;

/// Schedule cache rows older than this are neither served nor kept.
pub const SCHEDULE_CACHE_TTL_S: i64 = 1800;

pub const STATUS_QUEUED: &str = "QUEUED";
pub const STATUS_DOWNLOADING: &str = "DOWNLOADING";
pub const STATUS_COMPLETED: &str = "COMPLETED";
pub const STATUS_FAILED: &str = "FAILED";

const DOWNLOAD_COLUMNS: &str = "id, anilist_id, episode_num, episode_title, series_title, \
     series_cover, provider, category, quality, status, progress, file_path, file_size, \
     duration_s, error_msg, created_at, updated_at, audio_lang, sub_lang";

/// Where the app's folder is, given the directory it belongs in (`base`, for example
/// `$XDG_DATA_HOME`) and the one an install may still be using (`home_base`, for example
/// `~/.local/share`): the first folder that holds one of the `markers`, looking under the
/// current name in both and then under the earlier name in both. The earlier name is only still
/// there when `migration` could not rename the folder, and using it keeps the data in reach.
/// With no marker anywhere it is the current name in `base`.
pub fn choose_app_dir(base: Option<PathBuf>, home_base: Option<PathBuf>, markers: &[&str]) -> Option<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    for candidate in [base, home_base].into_iter().flatten() {
        if !bases.contains(&candidate) {
            bases.push(candidate);
        }
    }
    for name in [APP_DIR_NAME, LEGACY_APP_DIR_NAME] {
        for base in &bases {
            let dir = base.join(name);
            if markers.iter().any(|marker| dir.join(marker).exists()) {
                return Some(dir);
            }
        }
    }
    bases.first().map(|base| base.join(APP_DIR_NAME))
}

fn ensure_dir(path: Option<PathBuf>, what: &str) -> Result<PathBuf, AppError> {
    let path = path.ok_or_else(|| {
        AppError::Other(format!(
            "Could not determine the {} directory (is HOME set?)",
            what
        ))
    })?;
    fs::create_dir_all(&path).map_err(|e| {
        AppError::Other(format!(
            "Could not create the {} directory {}: {}",
            what,
            path.display(),
            e
        ))
    })?;
    Ok(path)
}

/// The name of the app's folder in the data, config and cache directories.
pub const APP_DIR_NAME: &str = "anix";
/// What that folder was called before the app was renamed; see `migration`.
pub const LEGACY_APP_DIR_NAME: &str = "anilili";
pub const DB_FILE_NAME: &str = "anix.db";
/// What the database file was called before the app was renamed. `migration` renames it; it is
/// still opened under this name if that did not work.
pub const LEGACY_DB_FILE_NAME: &str = "anilili.db";

/// `$XDG_DATA_HOME/anix`, normally `~/.local/share/anix`.
pub fn get_app_dir() -> Result<PathBuf, AppError> {
    let home_base = dirs::home_dir().map(|h| h.join(".local").join("share"));
    let chosen = choose_app_dir(dirs::data_dir(), home_base, &[DB_FILE_NAME, LEGACY_DB_FILE_NAME]);
    ensure_dir(chosen, "data")
}

/// `$XDG_CONFIG_HOME/anix`, normally `~/.config/anix`.
pub fn get_config_dir() -> Result<PathBuf, AppError> {
    let home_base = dirs::home_dir().map(|h| h.join(".config"));
    ensure_dir(choose_app_dir(dirs::config_dir(), home_base, &["settings.json"]), "config")
}

/// The database file in `app_dir`: `anix.db`, or a database still under its earlier name when
/// that is the only one there.
pub fn database_path(app_dir: &Path) -> PathBuf {
    let current = app_dir.join(DB_FILE_NAME);
    let legacy = app_dir.join(LEGACY_DB_FILE_NAME);
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}

fn error_detail(err: AppError) -> String {
    match err {
        AppError::Database(msg) => msg,
        other => other.to_string(),
    }
}

/// Opens (creating and migrating if needed) the app's database in the data directory.
/// The error text names the file and the cause, and is meant to be shown to the user.
pub async fn init_db() -> Result<SqlitePool, AppError> {
    let db_path = database_path(&get_app_dir()?);
    let pool = init_db_at(&db_path).await?;
    info!("Database initialized at {}", db_path.display());
    Ok(pool)
}

/// Opens (creating and migrating if needed) the database file at `db_path`.
pub async fn init_db_at(db_path: &Path) -> Result<SqlitePool, AppError> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await
        .map_err(|e| {
            AppError::Database(format!("could not open {}: {}", db_path.display(), e))
        })?;

    if let Err(e) = run_migrations(&pool).await {
        pool.close().await;
        return Err(AppError::Database(format!(
            "could not prepare {}: {}",
            db_path.display(),
            error_detail(e)
        )));
    }
    // Best effort: a merge that fails must never keep the app from starting.
    if let Err(e) = merge_stand_in_ids(&pool).await {
        warn!("could not merge stand-in ids: {}", e);
    }

    Ok(pool)
}

/// A private in-memory database with the real schema, for tests.
pub async fn init_db_in_memory() -> Result<SqlitePool, AppError> {
    let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);

    // Every connection to `:memory:` is its own database, so the pool keeps exactly one.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await?;

    run_migrations(&pool).await?;
    Ok(pool)
}

const SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS watchlist (
        anilist_id      INTEGER PRIMARY KEY,
        title           TEXT NOT NULL,
        cover           TEXT,
        format          TEXT,
        average_score   INTEGER,
        added_at        INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS remote_library (
        anilist_id      INTEGER PRIMARY KEY,
        status          TEXT,
        progress        INTEGER NOT NULL DEFAULT 0,
        score           REAL NOT NULL DEFAULT 0.0,
        title           TEXT,
        cover           TEXT,
        format          TEXT,
        episodes        INTEGER,
        average_score   INTEGER,
        synced_at       INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS downloads (
        id            TEXT PRIMARY KEY,
        anilist_id    INTEGER NOT NULL,
        episode_num   REAL NOT NULL,
        episode_title TEXT,
        series_title  TEXT NOT NULL,
        series_cover  TEXT,
        provider      TEXT NOT NULL,
        category      TEXT NOT NULL,
        quality       TEXT NOT NULL,
        status        TEXT NOT NULL DEFAULT 'QUEUED',
        progress      REAL DEFAULT 0.0,
        file_path     TEXT,
        file_size     INTEGER,
        duration_s    REAL,
        error_msg     TEXT,
        created_at    INTEGER NOT NULL,
        updated_at    INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS notification_preferences (
        media_id      INTEGER PRIMARY KEY,
        enabled       INTEGER NOT NULL DEFAULT 1,
        media_title   TEXT NOT NULL,
        cover_image   TEXT,
        enabled_at    INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS notified_episodes (
        media_id      INTEGER NOT NULL,
        episode       INTEGER NOT NULL,
        airing_at     INTEGER NOT NULL,
        notified_at   INTEGER NOT NULL,
        shown         INTEGER NOT NULL DEFAULT 1,
        PRIMARY KEY (media_id, episode)
    );

    CREATE TABLE IF NOT EXISTS notifier_state (
        key           TEXT PRIMARY KEY,
        value         INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS schedule_cache (
        from_ts     INTEGER NOT NULL,
        to_ts       INTEGER NOT NULL,
        data        TEXT NOT NULL,
        cached_at   INTEGER NOT NULL,
        PRIMARY KEY (from_ts, to_ts)
    );

    CREATE TABLE IF NOT EXISTS filler_cache (
        mal_id      INTEGER PRIMARY KEY,
        filler_json TEXT NOT NULL,
        cached_at   INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS custom_playlists (
        id          TEXT PRIMARY KEY,
        name        TEXT NOT NULL,
        description TEXT,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS custom_playlist_items (
        id            TEXT PRIMARY KEY,
        playlist_id   TEXT NOT NULL,
        anilist_id    INTEGER NOT NULL,
        mal_id        INTEGER,
        episode_num   REAL NOT NULL,
        episode_title TEXT,
        series_title  TEXT NOT NULL,
        series_cover  TEXT,
        category      TEXT NOT NULL DEFAULT 'sub',
        sort_order    INTEGER NOT NULL DEFAULT 0,
        added_at      INTEGER NOT NULL,
        FOREIGN KEY(playlist_id) REFERENCES custom_playlists(id) ON DELETE CASCADE
    );

    -- "Your files": a folder and/or single files the user linked to one AniList title.
    CREATE TABLE IF NOT EXISTS local_media_links (
        anilist_id    INTEGER PRIMARY KEY,
        folder_path   TEXT,
        updated_at    INTEGER NOT NULL
    );

    -- `manual` is set once the user chose the episode number (NULL = not used); a re-scan
    -- keeps it. `from_folder` rows are pruned when they leave the linked folder.
    CREATE TABLE IF NOT EXISTS local_media_files (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        anilist_id     INTEGER NOT NULL,
        file_path      TEXT NOT NULL,
        file_size      INTEGER NOT NULL DEFAULT 0,
        episode_number REAL,
        manual         INTEGER NOT NULL DEFAULT 0,
        from_folder    INTEGER NOT NULL DEFAULT 0,
        added_at       INTEGER NOT NULL,
        UNIQUE(anilist_id, file_path),
        FOREIGN KEY(anilist_id) REFERENCES local_media_links(anilist_id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_local_media_files_episode
        ON local_media_files(anilist_id, episode_number);

    -- Each day an episode was watched (at most one row per episode and UTC day), for the watch
    -- streak: watch_history keeps only the last time, so a rewatch would move the day.
    CREATE TABLE IF NOT EXISTS watch_events (
        anilist_id     INTEGER NOT NULL,
        episode_number REAL NOT NULL,
        day            INTEGER NOT NULL,
        watched_at     INTEGER NOT NULL,
        UNIQUE(anilist_id, episode_number, day)
    );
"#;

/// One row per (anime, episode). `completed` is set once the episode has been watched to
/// the threshold and stays set when the episode is replayed from the start.
fn watch_history_ddl(table: &str) -> String {
    format!(
        r#"
        CREATE TABLE IF NOT EXISTS {table} (
            anilist_id      INTEGER NOT NULL,
            episode_number  REAL NOT NULL DEFAULT 1.0,
            episode_title   TEXT,
            provider        TEXT NOT NULL DEFAULT 'auto',
            category        TEXT NOT NULL DEFAULT 'sub',
            position_ms     INTEGER NOT NULL DEFAULT 0,
            duration_ms     INTEGER NOT NULL DEFAULT 0,
            updated_at      INTEGER NOT NULL DEFAULT 0,
            from_remote     INTEGER NOT NULL DEFAULT 0,
            title           TEXT NOT NULL DEFAULT '',
            cover           TEXT,
            completed       INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (anilist_id, episode_number)
        );
        "#
    )
}

const WATCH_HISTORY_COLUMNS: [&str; 12] = [
    "anilist_id",
    "episode_number",
    "episode_title",
    "provider",
    "category",
    "position_ms",
    "duration_ms",
    "updated_at",
    "from_remote",
    "title",
    "cover",
    "completed",
];

/// Creates missing tables and brings older layouts up to date. Runs in one transaction, so a
/// failure leaves the database exactly as it was.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    sqlx::query(SCHEMA).execute(&mut *tx).await?;
    migrate_watch_history(&mut tx).await?;
    migrate_notification_preferences(&mut tx).await?;
    crate::reminders::create_tables(&mut tx).await?;
    crate::offline::create_tables(&mut tx).await?;
    crate::local_library::create_tables(&mut tx).await?;
    crate::catalogue::ids::create_tables(&mut tx).await?;
    crate::release_check::create_tables(&mut tx).await?;
    crate::notices::create_tables(&mut tx).await?;
    migrate_watchlist_status(&mut tx).await?;
    migrate_download_languages(&mut tx).await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_history_updated ON watch_history(updated_at DESC)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status, created_at)")
        .execute(&mut *tx)
        .await?;

    // Older builds stored whatever the caller sent: second or millisecond timestamps and
    // lower-case statuses. Second values are far below any millisecond timestamp.
    sqlx::query("UPDATE downloads SET created_at = created_at * 1000 WHERE created_at > 0 AND created_at < 100000000000")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE downloads SET updated_at = updated_at * 1000 WHERE updated_at > 0 AND updated_at < 100000000000")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE downloads SET status = UPPER(status) WHERE status <> UPPER(status)")
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// The user's own data keyed by an anime id, as (table, id column). Links come before files
/// (files refer to their link). Downloads, playlist items and the tracker queue are left out:
/// their rows are single things (a file, a playlist entry, a push) that a copy would double.
const ANIME_KEYED_TABLES: [(&str, &str); 9] = [
    ("watch_history", "anilist_id"),
    ("watch_events", "anilist_id"),
    ("watchlist", "anilist_id"),
    ("notification_preferences", "media_id"),
    ("notified_episodes", "media_id"),
    ("sent_reminders", "media_id"),
    ("release_checks", "media_id"),
    ("local_media_links", "anilist_id"),
    ("local_media_files", "anilist_id"),
];

/// Copies what the user did under a stand-in id (an anime first known without its AniList id)
/// to the AniList id the mapping cache learned since, so history, watchlist, bells and files
/// are not split across two ids. Rows the AniList id lacks are copied; for an episode in both,
/// the further progress wins (finished over unfinished, then the newer save). The stand-in rows
/// stay (only adding, never destroying), and the lists leave merged stand-ins out. Each mapping
/// is copied once, when it is new (or points at another AniList id): from then on the catalogue
/// hands out the AniList id, so later writes go there, and what the user removes under it must
/// not come back. Runs at every start (after the migrations, best effort) and after new
/// mappings are learned.
pub async fn merge_stand_in_ids(pool: &SqlitePool) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    merge_stand_ins(&mut tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn merge_stand_ins(conn: &mut SqliteConnection) -> Result<(), AppError> {
    // The stand-in formula of `catalogue::ids::synthetic_id`, in SQL. A new or changed mapping
    // gets merged_at = 0, which marks it as not copied yet.
    sqlx::query(&format!(
        r#"
        INSERT INTO catalogue_merged_ids (stand_in, anilist_id, merged_at)
        SELECT -((CASE site WHEN 'mal' THEN 1 WHEN 'kitsu' THEN 2 ELSE 3 END) * {span} + ext_id), anilist_id, 0
        FROM catalogue_ids
        WHERE anilist_id > 0 AND site IN ('mal', 'kitsu', 'imdb') AND ext_id BETWEEN 1 AND {span} - 1
        ON CONFLICT(stand_in) DO UPDATE SET anilist_id = excluded.anilist_id, merged_at = 0
        WHERE catalogue_merged_ids.anilist_id <> excluded.anilist_id
        "#,
        span = crate::catalogue::ids::SITE_SPAN
    ))
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        r#"
        UPDATE watch_history SET
            position_ms = s.position_ms,
            duration_ms = s.duration_ms,
            provider = s.provider,
            category = s.category,
            episode_title = COALESCE(watch_history.episode_title, s.episode_title),
            completed = s.completed,
            updated_at = MAX(watch_history.updated_at, s.updated_at)
        FROM watch_history AS s JOIN catalogue_merged_ids AS m ON s.anilist_id = m.stand_in
        WHERE m.merged_at = 0
          AND watch_history.anilist_id = m.anilist_id
          AND watch_history.episode_number = s.episode_number
          AND (s.completed > watch_history.completed
               OR (s.completed = watch_history.completed AND s.updated_at > watch_history.updated_at))
        "#,
    )
    .execute(&mut *conn)
    .await?;

    for (table, key) in ANIME_KEYED_TABLES {
        let columns: Vec<String> = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(&mut *conn)
            .await?
            .iter()
            .map(|row| row.get::<String, _>("name"))
            // A row id of its own is given anew.
            .filter(|name| name != "id")
            .collect();
        if columns.is_empty() {
            continue;
        }
        let values: Vec<String> = columns
            .iter()
            .map(|name| if name == key { "m.anilist_id".to_string() } else { format!("t.{name}") })
            .collect();
        // INSERT OR IGNORE does not ignore a foreign-key failure: a file goes only where its
        // AniList id has a link (copied just before, or the user's own).
        let link_check = if table == "local_media_files" {
            " AND EXISTS (SELECT 1 FROM local_media_links AS l WHERE l.anilist_id = m.anilist_id)"
        } else {
            ""
        };
        sqlx::query(&format!(
            "INSERT OR IGNORE INTO {table} ({}) SELECT {} FROM {table} AS t \
             JOIN catalogue_merged_ids AS m ON t.{key} = m.stand_in WHERE m.merged_at = 0{link_check}",
            columns.join(", "),
            values.join(", ")
        ))
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query("UPDATE catalogue_merged_ids SET merged_at = ?1 WHERE merged_at = 0")
        .bind(current_unix_millis())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn migrate_watch_history(conn: &mut SqliteConnection) -> Result<(), AppError> {
    let info_rows = sqlx::query("PRAGMA table_info(watch_history)")
        .fetch_all(&mut *conn)
        .await?;

    if info_rows.is_empty() {
        sqlx::query(&watch_history_ddl("watch_history"))
            .execute(&mut *conn)
            .await?;
        return Ok(());
    }

    let columns: Vec<String> = info_rows.iter().map(|r| r.get::<String, _>("name")).collect();
    let mut key: Vec<String> = info_rows
        .iter()
        .filter(|r| r.get::<i64, _>("pk") > 0)
        .map(|r| r.get::<String, _>("name"))
        .collect();
    key.sort();
    let has = |name: &str| columns.iter().any(|c| c == name);

    let up_to_date = key == ["anilist_id", "episode_number"]
        && WATCH_HISTORY_COLUMNS.iter().all(|c| has(*c));
    if up_to_date {
        return Ok(());
    }

    if !has("anilist_id") {
        // Not a layout any build of this app wrote. Keep the rows under another name rather
        // than guess at their meaning.
        let parked = format!("watch_history_unrecognized_{}", current_unix_millis());
        warn!("watch_history has an unknown layout; keeping it as {}", parked);
        sqlx::query(&format!("ALTER TABLE watch_history RENAME TO {}", parked))
            .execute(&mut *conn)
            .await?;
        sqlx::query(&watch_history_ddl("watch_history"))
            .execute(&mut *conn)
            .await?;
        return Ok(());
    }

    // Every name below is a compile-time constant; only its presence is read from the table.
    let episode = if has("episode_number") {
        "COALESCE(CAST(episode_number AS REAL), 1.0)"
    } else if has("episode") {
        "COALESCE(CAST(episode AS REAL), 1.0)"
    } else {
        "1.0"
    };
    let or_default = |name: &str, default: &str| {
        if has(name) {
            format!("COALESCE({}, {})", name, default)
        } else {
            default.to_string()
        }
    };
    let or_null = |name: &str| if has(name) { name.to_string() } else { "NULL".to_string() };

    let position = or_default("position_ms", "0");
    let duration = or_default("duration_ms", "0");
    let updated_at = or_default("updated_at", "0");
    let reached_threshold = format!(
        "CASE WHEN {d} > 0 AND {p} * 100 >= {d} * {t} THEN 1 ELSE 0 END",
        p = position,
        d = duration,
        t = WATCHED_THRESHOLD_PERCENT
    );
    let completed = if has("completed") {
        format!("MAX(COALESCE(completed, 0), {})", reached_threshold)
    } else {
        reached_threshold
    };

    let old_count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM watch_history WHERE anilist_id IS NOT NULL")
        .fetch_one(&mut *conn)
        .await?
        .get("n");

    sqlx::query("DROP TABLE IF EXISTS watch_history_migrating")
        .execute(&mut *conn)
        .await?;
    sqlx::query(&watch_history_ddl("watch_history_migrating"))
        .execute(&mut *conn)
        .await?;

    // Oldest first, so if an old layout held the same (anime, episode) twice the newest wins.
    let copy = format!(
        r#"
        INSERT OR REPLACE INTO watch_history_migrating
            (anilist_id, episode_number, episode_title, provider, category, position_ms,
             duration_ms, updated_at, from_remote, title, cover, completed)
        SELECT anilist_id, {episode}, {episode_title}, {provider}, {category}, {position},
               {duration}, {updated_at}, {from_remote}, {title}, {cover}, {completed}
        FROM watch_history
        WHERE anilist_id IS NOT NULL
        ORDER BY {updated_at} ASC
        "#,
        episode = episode,
        episode_title = or_null("episode_title"),
        provider = or_default("provider", "'auto'"),
        category = or_default("category", "'sub'"),
        position = position,
        duration = duration,
        updated_at = updated_at,
        from_remote = or_default("from_remote", "0"),
        title = or_default("title", "''"),
        cover = or_null("cover"),
        completed = completed,
    );
    sqlx::query(&copy).execute(&mut *conn).await?;

    let new_count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM watch_history_migrating")
        .fetch_one(&mut *conn)
        .await?
        .get("n");

    sqlx::query("DROP TABLE watch_history").execute(&mut *conn).await?;
    sqlx::query("ALTER TABLE watch_history_migrating RENAME TO watch_history")
        .execute(&mut *conn)
        .await?;

    info!(
        "Migrated watch history to one row per episode ({} rows read, {} kept)",
        old_count, new_count
    );
    Ok(())
}

pub fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn current_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// True once `position_ms` is at least 85% of a known duration.
pub fn is_episode_watched(position_ms: i64, duration_ms: i64) -> bool {
    duration_ms > 0
        && (position_ms as i128) * 100 >= (duration_ms as i128) * (WATCHED_THRESHOLD_PERCENT as i128)
}

fn map_history_row(r: SqliteRow) -> HistoryEntry {
    let anilist_id: i64 = r.get("anilist_id");
    let from_remote: i64 = r.get("from_remote");

    HistoryEntry {
        anilist_id: anilist_id as i32,
        title: r.get("title"),
        cover: r.get("cover"),
        episode_number: r.get("episode_number"),
        episode_title: r.get("episode_title"),
        provider: r.get("provider"),
        category: r.get("category"),
        position_ms: r.get("position_ms"),
        duration_ms: r.get("duration_ms"),
        updated_at: r.get("updated_at"),
        from_remote: from_remote != 0,
    }
}

/// One entry per anime, most recently watched anime first.
///
/// The entry is the anime's most recently played episode. If that episode is already
/// finished and a later episode of the same anime has been started but not finished, the
/// later one is returned instead; a finished episode is only listed when nothing later is
/// in progress.
pub async fn get_continue_watching(pool: &SqlitePool) -> Result<Vec<HistoryEntry>, AppError> {
    let sql = format!(
        r#"
        WITH ranked AS (
            SELECT anilist_id, episode_number, position_ms, duration_ms, updated_at,
                   ROW_NUMBER() OVER (
                       PARTITION BY anilist_id
                       ORDER BY updated_at DESC, episode_number DESC
                   ) AS rn
            FROM watch_history
            WHERE position_ms > 0 AND anilist_id NOT IN (SELECT stand_in FROM catalogue_merged_ids)
        ),
        pick AS (
            SELECT l.anilist_id,
                   l.updated_at AS last_played,
                   COALESCE(
                       CASE WHEN l.duration_ms > 0 AND l.position_ms * 100 >= l.duration_ms * {t} THEN (
                           SELECT n.episode_number
                           FROM watch_history n
                           WHERE n.anilist_id = l.anilist_id
                             AND n.episode_number > l.episode_number
                             AND n.position_ms > 0
                             AND NOT (n.duration_ms > 0 AND n.position_ms * 100 >= n.duration_ms * {t})
                           ORDER BY n.updated_at DESC, n.episode_number ASC
                           LIMIT 1
                       ) END,
                       l.episode_number
                   ) AS episode_number
            FROM ranked l
            WHERE l.rn = 1
        )
        SELECT h.anilist_id, h.episode_number, h.episode_title, h.provider, h.category,
               h.position_ms, h.duration_ms, h.updated_at, h.from_remote, h.title, h.cover
        FROM pick p
        JOIN watch_history h
          ON h.anilist_id = p.anilist_id AND h.episode_number = p.episode_number
        ORDER BY p.last_played DESC
        LIMIT 20
        "#,
        t = WATCHED_THRESHOLD_PERCENT
    );

    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    Ok(rows.into_iter().map(map_history_row).collect())
}

/// Episode numbers of `anilist_id` that have been watched to the threshold, ascending.
pub async fn get_watched_episodes(pool: &SqlitePool, anilist_id: i32) -> Result<Vec<f64>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT episode_number
        FROM watch_history
        WHERE anilist_id = ?1 AND completed = 1
        ORDER BY episode_number ASC
        "#,
    )
    .bind(anilist_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.get::<f64, _>("episode_number")).collect())
}

/// Saved `(position_ms, duration_ms)` of one episode.
pub async fn get_episode_progress(
    pool: &SqlitePool,
    anilist_id: i32,
    episode_number: f64,
) -> Result<Option<(i64, i64)>, AppError> {
    let row = sqlx::query(
        r#"
        SELECT position_ms, duration_ms
        FROM watch_history
        WHERE anilist_id = ?1 AND ABS(episode_number - ?2) < 0.01
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(anilist_id)
    .bind(episode_number)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| (r.get("position_ms"), r.get("duration_ms"))))
}

/// Inserts or updates the row of `(entry.anilist_id, entry.episode_number)`. A finished
/// episode stays as it was (position, time, provider) until a save finishes it again, so
/// opening it for a moment never makes it look unwatched, moves it in Continue Watching or
/// counts today in the statistics.
pub async fn update_watch_progress(
    pool: &SqlitePool,
    entry: &HistoryEntry,
) -> Result<(), AppError> {
    let from_remote_val = if entry.from_remote { 1 } else { 0 };
    let position_ms = entry.position_ms.max(0);
    let duration_ms = entry.duration_ms.max(0);
    let completed = if is_episode_watched(position_ms, duration_ms) { 1 } else { 0 };

    sqlx::query(
        r#"
        INSERT INTO watch_history (anilist_id, episode_number, episode_title, provider, category, position_ms, duration_ms, updated_at, from_remote, title, cover, completed)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(anilist_id, episode_number) DO UPDATE SET
            episode_title = COALESCE(excluded.episode_title, watch_history.episode_title),
            provider = excluded.provider,
            category = excluded.category,
            position_ms = excluded.position_ms,
            duration_ms = CASE WHEN excluded.duration_ms > 0 THEN excluded.duration_ms ELSE watch_history.duration_ms END,
            updated_at = excluded.updated_at,
            from_remote = excluded.from_remote,
            title = excluded.title,
            cover = COALESCE(excluded.cover, watch_history.cover),
            completed = MAX(watch_history.completed, excluded.completed)
        WHERE NOT (watch_history.completed = 1 AND excluded.completed = 0);
        "#,
    )
    .bind(entry.anilist_id)
    .bind(entry.episode_number)
    .bind(&entry.episode_title)
    .bind(&entry.provider)
    .bind(&entry.category)
    .bind(position_ms)
    .bind(duration_ms)
    .bind(entry.updated_at)
    .bind(from_remote_val)
    .bind(&entry.title)
    .bind(&entry.cover)
    .bind(completed)
    .execute(pool)
    .await?;
    if completed == 1 && !entry.from_remote {
        record_watch_event(pool, entry.anilist_id as i64, entry.episode_number).await?;
    }

    Ok(())
}

/// Notes that an episode was watched today (once per episode and day).
async fn record_watch_event<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    anilist_id: i64,
    episode_number: f64,
) -> Result<(), AppError> {
    let now = current_unix_millis();
    sqlx::query("INSERT OR IGNORE INTO watch_events (anilist_id, episode_number, day, watched_at) VALUES (?1, ?2, ?3, ?4)")
        .bind(anilist_id)
        .bind(episode_number)
        .bind(now / 86_400_000)
        .bind(now)
        .execute(executor)
        .await?;
    Ok(())
}

/// The times episodes were watched, in Unix milliseconds: every recorded watch, plus the last
/// time of each watched episode (rows from before the record existed, or restored from a backup).
pub async fn get_watch_times(pool: &SqlitePool) -> Result<Vec<i64>, AppError> {
    let rows = sqlx::query(
        "SELECT watched_at AS at FROM watch_events WHERE anilist_id NOT IN (SELECT stand_in FROM catalogue_merged_ids) \
         UNION ALL SELECT updated_at AS at FROM watch_history WHERE completed = 1 \
         AND anilist_id NOT IN (SELECT stand_in FROM catalogue_merged_ids)",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.get::<i64, _>("at")).collect())
}

/// Marks `episodes` of one anime as watched, or takes the mark off, in one transaction.
/// A marked episode is put at its end: its known length, or 1 ms of 1 ms when the length is
/// not known, so the 85% rule counts it everywhere. Unmarking keeps the row (title, length)
/// with the position back at the start and drops the episode's watch days, so the streak and
/// Stats no longer count it. Returns the number of rows written.
pub async fn set_episodes_watched(
    pool: &SqlitePool,
    anilist_id: i32,
    title: &str,
    cover: Option<&str>,
    episodes: &[f64],
    watched: bool,
) -> Result<u64, AppError> {
    let now = current_unix_millis();
    let mut tx = pool.begin().await?;
    let mut written = 0;
    for &episode in episodes.iter().filter(|n| n.is_finite()) {
        let result = if watched {
            let marked = sqlx::query(
                r#"
                INSERT INTO watch_history (anilist_id, episode_number, position_ms, duration_ms, updated_at, from_remote, title, cover, completed)
                VALUES (?1, ?2, 1, 1, ?3, 0, ?4, ?5, 1)
                ON CONFLICT(anilist_id, episode_number) DO UPDATE SET
                    position_ms = CASE WHEN watch_history.duration_ms > 1 THEN watch_history.duration_ms ELSE 1 END,
                    duration_ms = CASE WHEN watch_history.duration_ms > 1 THEN watch_history.duration_ms ELSE 1 END,
                    updated_at = excluded.updated_at,
                    from_remote = 0,
                    title = CASE WHEN excluded.title <> '' THEN excluded.title ELSE watch_history.title END,
                    cover = COALESCE(excluded.cover, watch_history.cover),
                    completed = 1;
                "#,
            )
            .bind(anilist_id)
            .bind(episode)
            .bind(now)
            .bind(title)
            .bind(cover)
            .execute(&mut *tx)
            .await?;
            record_watch_event(&mut *tx, anilist_id as i64, episode).await?;
            marked
        } else {
            let unmarked = sqlx::query(
                r#"
                UPDATE watch_history SET position_ms = 0, completed = 0
                WHERE anilist_id = ?1 AND ABS(episode_number - ?2) < 0.001
                "#,
            )
            .bind(anilist_id)
            .bind(episode)
            .execute(&mut *tx)
            .await?;
            sqlx::query("DELETE FROM watch_events WHERE anilist_id = ?1 AND ABS(episode_number - ?2) < 0.001")
                .bind(anilist_id)
                .bind(episode)
                .execute(&mut *tx)
                .await?;
            unmarked
        };
        written += result.rows_affected();
    }
    tx.commit().await?;
    Ok(written)
}

/// One watched episode, for the statistics.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WatchedEpisodeRow {
    pub anilist_id: i32,
    pub title: String,
    pub cover: Option<String>,
    pub episode_number: f64,
    /// 0 or 1 when the length is not known (marked by hand, or synced from a list).
    pub duration_ms: i64,
    pub updated_at: i64,
}

/// Every episode that counts as watched, newest first.
pub async fn get_watched_history(pool: &SqlitePool) -> Result<Vec<WatchedEpisodeRow>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT anilist_id, title, cover, episode_number, duration_ms, updated_at
        FROM watch_history
        WHERE completed = 1 AND anilist_id NOT IN (SELECT stand_in FROM catalogue_merged_ids)
        ORDER BY updated_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| WatchedEpisodeRow {
            anilist_id: r.get::<i64, _>("anilist_id") as i32,
            title: r.get("title"),
            cover: r.get("cover"),
            episode_number: r.get("episode_number"),
            duration_ms: r.get("duration_ms"),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

pub async fn add_to_watchlist(
    pool: &SqlitePool,
    anilist_id: i32,
    title: &str,
    cover: Option<&str>,
    format: Option<&str>,
    average_score: Option<i32>,
) -> Result<(), AppError> {
    let now = current_unix_millis();

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO watchlist (anilist_id, title, cover, format, average_score, added_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#
    )
    .bind(anilist_id)
    .bind(title)
    .bind(cover)
    .bind(format)
    .bind(average_score)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn remove_from_watchlist(pool: &SqlitePool, anilist_id: i32) -> Result<(), AppError> {
    sqlx::query("DELETE FROM watchlist WHERE anilist_id = ?1")
        .bind(anilist_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn is_in_watchlist(pool: &SqlitePool, anilist_id: i32) -> Result<bool, AppError> {
    let row = sqlx::query("SELECT anilist_id FROM watchlist WHERE anilist_id = ?1")
        .bind(anilist_id)
        .fetch_optional(pool)
        .await?;

    Ok(row.is_some())
}

pub async fn get_watchlist(pool: &SqlitePool) -> Result<Vec<crate::models::WatchlistEntry>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT anilist_id, title, cover, format, average_score, added_at, status, synced
        FROM watchlist
        WHERE anilist_id NOT IN (SELECT stand_in FROM catalogue_merged_ids)
        ORDER BY added_at DESC
        "#
    )
    .fetch_all(pool)
    .await?;

    let entries = rows
        .into_iter()
        .map(|r| crate::models::WatchlistEntry {
            anilist_id: r.get("anilist_id"),
            title: r.get("title"),
            cover: r.get("cover"),
            format: r.get("format"),
            average_score: r.get("average_score"),
            added_at: r.get("added_at"),
            status: r.get("status"),
            synced: r.get::<i64, _>("synced") != 0,
        })
        .collect();

    Ok(entries)
}

/// The list statuses of the watchlist, in AniList vocabulary.
pub const WATCHLIST_STATUSES: [&str; 5] = ["CURRENT", "PLANNING", "COMPLETED", "PAUSED", "DROPPED"];

/// 1.2.0 watchlists had no status: every bookmark becomes "Plan to watch", already in sync.
/// `synced = 0` marks a status chosen here that the connected lists have not received yet.
async fn migrate_watchlist_status(conn: &mut SqliteConnection) -> Result<(), AppError> {
    let columns: Vec<String> = sqlx::query("PRAGMA table_info(watchlist)")
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !columns.iter().any(|c| c == "status") {
        sqlx::query("ALTER TABLE watchlist ADD COLUMN status TEXT NOT NULL DEFAULT 'PLANNING'")
            .execute(&mut *conn)
            .await?;
    }
    if !columns.iter().any(|c| c == "synced") {
        sqlx::query("ALTER TABLE watchlist ADD COLUMN synced INTEGER NOT NULL DEFAULT 1")
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Puts the anime on the watchlist with `status`, or changes the status of its row.
#[allow(clippy::too_many_arguments)]
pub async fn set_watchlist_status(
    pool: &SqlitePool,
    anilist_id: i32,
    title: &str,
    cover: Option<&str>,
    format: Option<&str>,
    average_score: Option<i32>,
    status: &str,
    synced: bool,
) -> Result<(), AppError> {
    if !WATCHLIST_STATUSES.contains(&status) {
        return Err(AppError::Other(format!("Unknown list status: {}", status)));
    }
    sqlx::query(
        r#"
        INSERT INTO watchlist (anilist_id, title, cover, format, average_score, added_at, status, synced)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(anilist_id) DO UPDATE SET status = excluded.status, synced = excluded.synced
        "#,
    )
    .bind(anilist_id)
    .bind(title)
    .bind(cover)
    .bind(format)
    .bind(average_score)
    .bind(current_unix_millis())
    .bind(status)
    .bind(synced as i64)
    .execute(pool)
    .await?;
    Ok(())
}

/// The status of an anime on the watchlist, and whether the connected lists have it.
pub async fn get_watchlist_status(pool: &SqlitePool, anilist_id: i32) -> Result<Option<(String, bool)>, AppError> {
    let row = sqlx::query("SELECT status, synced FROM watchlist WHERE anilist_id = ?1")
        .bind(anilist_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| (r.get("status"), r.get::<i64, _>("synced") != 0)))
}

/// Takes a status the connected list reported, unless a local change is still waiting to be
/// sent (that one is newer). Only anime already on the watchlist are changed.
pub async fn take_remote_status(pool: &SqlitePool, anilist_id: i32, status: &str) -> Result<bool, AppError> {
    let changed = sqlx::query("UPDATE watchlist SET status = ?2 WHERE anilist_id = ?1 AND synced = 1 AND status <> ?2")
        .bind(anilist_id)
        .bind(status)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(changed > 0)
}

/// Statuses chosen here that still have to be sent: (anime, status).
pub async fn unsynced_watchlist(pool: &SqlitePool) -> Result<Vec<(i32, String)>, AppError> {
    let rows = sqlx::query(
        "SELECT anilist_id, status FROM watchlist WHERE synced = 0 \
         AND anilist_id NOT IN (SELECT stand_in FROM catalogue_merged_ids) ORDER BY added_at ASC",
    )
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| (r.get::<i64, _>("anilist_id") as i32, r.get("status"))).collect())
}

/// Marks a sent status as in sync, unless it was changed again meanwhile.
pub async fn mark_watchlist_synced(pool: &SqlitePool, anilist_id: i32, status: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE watchlist SET synced = 1 WHERE anilist_id = ?1 AND status = ?2")
        .bind(anilist_id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn upsert_library_entry(
    pool: &SqlitePool,
    entry: &crate::models::MediaListEntry,
) -> Result<(), AppError> {
    let now = current_unix_millis();

    sqlx::query(
        r#"
        INSERT OR REPLACE INTO remote_library (anilist_id, status, progress, score, title, cover, format, episodes, average_score, synced_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#
    )
    .bind(entry.id)
    .bind(&entry.status)
    .bind(entry.progress)
    .bind(entry.score)
    .bind(&entry.title)
    .bind(&entry.cover)
    .bind(&entry.format_str)
    .bind(entry.total_episodes)
    .bind(entry.average_score)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_library_entries(
    pool: &SqlitePool,
    status_filter: Option<String>,
) -> Result<Vec<crate::models::MediaListEntry>, AppError> {
    let rows = if let Some(ref status) = status_filter {
        sqlx::query(
            r#"
            SELECT anilist_id, status, progress, score, title, cover, format, episodes, average_score
            FROM remote_library
            WHERE status = ?1
            ORDER BY title ASC
            "#
        )
        .bind(status)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT anilist_id, status, progress, score, title, cover, format, episodes, average_score
            FROM remote_library
            ORDER BY title ASC
            "#
        )
        .fetch_all(pool)
        .await?
    };

    let entries = rows
        .into_iter()
        .map(|r| crate::models::MediaListEntry {
            id: r.get("anilist_id"),
            status: r.get("status"),
            progress: r.get("progress"),
            score: r.get("score"),
            title: r.get("title"),
            cover: r.get("cover"),
            format_str: r.get("format"),
            total_episodes: r.get("episodes"),
            average_score: r.get("average_score"),
        })
        .collect();

    Ok(entries)
}

fn parse_download_status(status: &str) -> Option<&'static str> {
    match status.trim().to_ascii_uppercase().as_str() {
        STATUS_QUEUED => Some(STATUS_QUEUED),
        STATUS_DOWNLOADING => Some(STATUS_DOWNLOADING),
        STATUS_COMPLETED => Some(STATUS_COMPLETED),
        STATUS_FAILED => Some(STATUS_FAILED),
        _ => None,
    }
}

fn clamp_progress(progress: f64) -> f64 {
    if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Inserts a download row. `created_at` and `updated_at` are always written here, in
/// milliseconds; the values in `record` are ignored. The status is stored upper-case and a
/// missing or unknown status becomes `QUEUED`. Progress is a fraction from 0.0 to 1.0.
pub async fn create_download(
    pool: &SqlitePool,
    record: &DownloadRecord,
) -> Result<String, AppError> {
    let now = current_unix_millis();
    let status = parse_download_status(&record.status).unwrap_or(STATUS_QUEUED);

    sqlx::query(
        r#"
        INSERT INTO downloads (id, anilist_id, episode_num, episode_title, series_title, series_cover, provider, category, quality, status, progress, file_path, file_size, duration_s, error_msg, created_at, updated_at, audio_lang, sub_lang)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        "#
    )
    .bind(&record.id)
    .bind(record.anilist_id)
    .bind(record.episode_num)
    .bind(&record.episode_title)
    .bind(&record.series_title)
    .bind(&record.series_cover)
    .bind(&record.provider)
    .bind(&record.category)
    .bind(&record.quality)
    .bind(status)
    .bind(clamp_progress(record.progress))
    .bind(&record.file_path)
    .bind(record.file_size)
    .bind(record.duration_s)
    .bind(&record.error_msg)
    .bind(now)
    .bind(now)
    .bind(&record.audio_lang)
    .bind(&record.sub_lang)
    .execute(pool)
    .await?;

    Ok(record.id.clone())
}

/// The audio and subtitle language codes of a download, once its source is known. A `None`
/// keeps what is stored.
pub async fn set_download_languages(
    pool: &SqlitePool,
    id: &str,
    audio_lang: Option<&str>,
    sub_lang: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE downloads SET audio_lang = COALESCE(?1, audio_lang), sub_lang = COALESCE(?2, sub_lang) WHERE id = ?3")
        .bind(audio_lang)
        .bind(sub_lang)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Adds the language columns of 1.3.0 to a downloads table from an earlier version.
async fn migrate_download_languages(conn: &mut SqliteConnection) -> Result<(), AppError> {
    let columns: Vec<String> = sqlx::query("PRAGMA table_info(downloads)")
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    for column in ["audio_lang", "sub_lang"] {
        if !columns.iter().any(|c| c == column) {
            // The name is one of the two constants above.
            sqlx::query(&format!("ALTER TABLE downloads ADD COLUMN {} TEXT", column))
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

pub async fn update_download_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    progress: Option<f64>,
    file_path: Option<&str>,
    file_size: Option<i64>,
    duration_s: Option<f64>,
    error_msg: Option<&str>,
) -> Result<(), AppError> {
    let status = parse_download_status(status)
        .ok_or_else(|| AppError::Other(format!("Unknown download status: {}", status)))?;
    let now = current_unix_millis();

    sqlx::query(
        r#"
        UPDATE downloads
        SET status = ?1,
            progress = COALESCE(?2, progress),
            file_path = COALESCE(?3, file_path),
            file_size = COALESCE(?4, file_size),
            duration_s = COALESCE(?5, duration_s),
            error_msg = ?6,
            updated_at = ?7
        WHERE id = ?8
        "#
    )
    .bind(status)
    .bind(progress.map(clamp_progress))
    .bind(file_path)
    .bind(file_size)
    .bind(duration_s)
    .bind(error_msg)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Records progress of a running download. Does nothing unless the row is `DOWNLOADING`,
/// so a late write cannot disturb a row that has already finished or failed.
pub async fn update_download_progress(
    pool: &SqlitePool,
    id: &str,
    progress: Option<f64>,
    file_size: Option<i64>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE downloads
        SET progress = COALESCE(?1, progress),
            file_size = COALESCE(?2, file_size),
            updated_at = ?3
        WHERE id = ?4 AND status = 'DOWNLOADING'
        "#,
    )
    .bind(progress.map(clamp_progress))
    .bind(file_size)
    .bind(current_unix_millis())
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Atomically turns the oldest `QUEUED` row into `DOWNLOADING` and returns it.
pub async fn claim_next_queued_download(
    pool: &SqlitePool,
) -> Result<Option<DownloadRecord>, AppError> {
    let sql = format!(
        r#"
        UPDATE downloads
        SET status = 'DOWNLOADING', progress = 0.0, error_msg = NULL, updated_at = ?1
        WHERE id = (
            SELECT id FROM downloads
            WHERE status = 'QUEUED'
            ORDER BY created_at ASC, rowid ASC
            LIMIT 1
        )
        RETURNING {}
        "#,
        DOWNLOAD_COLUMNS
    );

    let row = sqlx::query(&sql)
        .bind(current_unix_millis())
        .fetch_optional(pool)
        .await?;

    Ok(row.map(map_download_row))
}

/// For startup, before any download runs: rows still `DOWNLOADING` belong to a previous run
/// of the app that was interrupted, so they go back to `QUEUED` for the queue worker.
/// Returns how many rows were reset.
pub async fn reconcile_stale_downloads(pool: &SqlitePool) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        UPDATE downloads
        SET status = 'QUEUED', progress = 0.0, file_size = NULL, error_msg = NULL, updated_at = ?1
        WHERE status = 'DOWNLOADING'
        "#,
    )
    .bind(current_unix_millis())
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

fn map_download_row(r: SqliteRow) -> DownloadRecord {
    DownloadRecord {
        id: r.get("id"),
        anilist_id: r.get("anilist_id"),
        episode_num: r.get("episode_num"),
        episode_title: r.get("episode_title"),
        series_title: r.get("series_title"),
        series_cover: r.get("series_cover"),
        provider: r.get("provider"),
        category: r.get("category"),
        quality: r.get("quality"),
        status: r.get("status"),
        progress: r.get::<Option<f64>, _>("progress").unwrap_or(0.0),
        file_path: r.get("file_path"),
        file_size: r.get("file_size"),
        duration_s: r.get("duration_s"),
        error_msg: r.get("error_msg"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        audio_lang: r.get("audio_lang"),
        sub_lang: r.get("sub_lang"),
    }
}

pub async fn get_downloads(
    pool: &SqlitePool,
    anilist_id: i64,
) -> Result<Vec<DownloadRecord>, AppError> {
    let sql = format!(
        "SELECT {} FROM downloads WHERE anilist_id = ?1 ORDER BY episode_num ASC",
        DOWNLOAD_COLUMNS
    );
    let rows = sqlx::query(&sql).bind(anilist_id).fetch_all(pool).await?;

    Ok(rows.into_iter().map(map_download_row).collect())
}

pub async fn get_all_downloads(pool: &SqlitePool) -> Result<Vec<DownloadRecord>, AppError> {
    let sql = format!(
        "SELECT {} FROM downloads ORDER BY updated_at DESC",
        DOWNLOAD_COLUMNS
    );
    let rows = sqlx::query(&sql).fetch_all(pool).await?;

    Ok(rows.into_iter().map(map_download_row).collect())
}

pub async fn delete_download(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM downloads WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn get_download_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<DownloadRecord>, AppError> {
    let sql = format!("SELECT {} FROM downloads WHERE id = ?1", DOWNLOAD_COLUMNS);
    let row = sqlx::query(&sql).bind(id).fetch_optional(pool).await?;

    Ok(row.map(map_download_row))
}

/// Bells saved before release notifications existed have no switch-on time. They get the
/// time of the upgrade, so nothing that aired before it is announced.
async fn migrate_notification_preferences(conn: &mut SqliteConnection) -> Result<(), AppError> {
    let info_rows = sqlx::query("PRAGMA table_info(notification_preferences)")
        .fetch_all(&mut *conn)
        .await?;
    if info_rows.iter().any(|r| r.get::<String, _>("name") == "enabled_at") {
        return Ok(());
    }

    sqlx::query("ALTER TABLE notification_preferences ADD COLUMN enabled_at INTEGER NOT NULL DEFAULT 0")
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE notification_preferences SET enabled_at = ?1")
        .bind(current_unix_timestamp())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn save_notification_preference(
    pool: &SqlitePool,
    pref: &NotificationPreference,
) -> Result<(), AppError> {
    save_notification_preference_at(pool, pref, current_unix_timestamp()).await
}

/// Saves a bell. `now` (Unix seconds) becomes its switch-on time when the bell goes from off
/// to on; saving a bell that is already on keeps the earlier time.
pub async fn save_notification_preference_at(
    pool: &SqlitePool,
    pref: &NotificationPreference,
    now: i64,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO notification_preferences (media_id, enabled, media_title, cover_image, enabled_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(media_id) DO UPDATE SET
            enabled_at = CASE
                WHEN excluded.enabled = 1 AND notification_preferences.enabled = 0 THEN excluded.enabled_at
                ELSE notification_preferences.enabled_at
            END,
            enabled = excluded.enabled,
            media_title = excluded.media_title,
            cover_image = excluded.cover_image
        "#,
    )
    .bind(pref.media_id)
    .bind(if pref.enabled { 1i32 } else { 0i32 })
    .bind(&pref.media_title)
    .bind(&pref.cover_image)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

/// An anime the user wants release notifications for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationBell {
    pub media_id: i64,
    pub media_title: String,
    pub cover_image: Option<String>,
    /// Unix seconds at which the bell was last switched on.
    pub enabled_at: i64,
}

/// Every bell that is switched on.
pub async fn list_notification_bells(pool: &SqlitePool) -> Result<Vec<NotificationBell>, AppError> {
    let rows = sqlx::query(
        "SELECT media_id, media_title, cover_image, enabled_at FROM notification_preferences \
         WHERE enabled = 1 AND media_id NOT IN (SELECT stand_in FROM catalogue_merged_ids) ORDER BY media_id",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| NotificationBell {
            media_id: r.get("media_id"),
            media_title: r.get("media_title"),
            cover_image: r.get("cover_image"),
            enabled_at: r.get("enabled_at"),
        })
        .collect())
}

/// Records that an episode has been dealt with: `shown` when a notification went out, not
/// `shown` when it was skipped on purpose. Returns false if the episode was already recorded,
/// in which case nothing changes.
pub async fn record_notified_episode(
    pool: &SqlitePool,
    media_id: i64,
    episode: i32,
    airing_at: i64,
    notified_at: i64,
    shown: bool,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        r#"
        INSERT OR IGNORE INTO notified_episodes (media_id, episode, airing_at, notified_at, shown)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
    )
    .bind(media_id)
    .bind(episode)
    .bind(airing_at)
    .bind(notified_at)
    .bind(if shown { 1i32 } else { 0i32 })
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// `(media_id, episode)` of every episode recorded by `record_notified_episode`.
pub async fn list_notified_episodes(pool: &SqlitePool) -> Result<Vec<(i64, i32)>, AppError> {
    let rows = sqlx::query("SELECT media_id, episode FROM notified_episodes")
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|r| (r.get("media_id"), r.get("episode")))
        .collect())
}

pub async fn get_notifier_state(pool: &SqlitePool, key: &str) -> Result<Option<i64>, AppError> {
    let row = sqlx::query("SELECT value FROM notifier_state WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| r.get("value")))
}

pub async fn set_notifier_state(pool: &SqlitePool, key: &str, value: i64) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO notifier_state (key, value) VALUES (?1, ?2)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_notification_preference(
    pool: &SqlitePool,
    media_id: i64,
) -> Result<Option<NotificationPreference>, AppError> {
    let row = sqlx::query(
        "SELECT media_id, enabled, media_title, cover_image FROM notification_preferences WHERE media_id = ?1",
    )
    .bind(media_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| NotificationPreference {
        media_id: r.get("media_id"),
        enabled: r.get::<i32, _>("enabled") != 0,
        media_title: r.get("media_title"),
        cover_image: r.get("cover_image"),
    }))
}

pub async fn list_notification_preferences(
    pool: &SqlitePool,
) -> Result<Vec<NotificationPreference>, AppError> {
    let rows = sqlx::query(
        "SELECT media_id, enabled, media_title, cover_image FROM notification_preferences WHERE enabled = 1 \
         AND media_id NOT IN (SELECT stand_in FROM catalogue_merged_ids)",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| NotificationPreference {
            media_id: r.get("media_id"),
            enabled: r.get::<i32, _>("enabled") != 0,
            media_title: r.get("media_title"),
            cover_image: r.get("cover_image"),
        })
        .collect())
}

pub async fn get_cached_schedule(
    pool: &SqlitePool,
    from_ts: i64,
    to_ts: i64,
) -> Result<Option<Vec<AiringEntry>>, AppError> {
    let now = current_unix_timestamp();
    let row = sqlx::query(
        "SELECT data, cached_at FROM schedule_cache WHERE from_ts = ?1 AND to_ts = ?2",
    )
    .bind(from_ts)
    .bind(to_ts)
    .fetch_optional(pool)
    .await?;

    if let Some(r) = row {
        let cached_at: i64 = r.get("cached_at");
        if now - cached_at < SCHEDULE_CACHE_TTL_S {
            let data_str: String = r.get("data");
            if let Ok(entries) = serde_json::from_str::<Vec<AiringEntry>>(&data_str) {
                return Ok(Some(entries));
            }
        }
    }

    Ok(None)
}

/// Stores one schedule window and drops every window that has expired. The key changes
/// daily, so without the cleanup the table would gain a row per day forever.
pub async fn cache_schedule(
    pool: &SqlitePool,
    from_ts: i64,
    to_ts: i64,
    entries: &[AiringEntry],
) -> Result<(), AppError> {
    let now = current_unix_timestamp();
    let json_data = serde_json::to_string(entries)
        .map_err(|e| AppError::Other(e.to_string()))?;

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM schedule_cache WHERE cached_at <= ?1")
        .bind(now - SCHEDULE_CACHE_TTL_S)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO schedule_cache (from_ts, to_ts, data, cached_at)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(from_ts, to_ts) DO UPDATE SET
            data = excluded.data,
            cached_at = excluded.cached_at
        "#,
    )
    .bind(from_ts)
    .bind(to_ts)
    .bind(json_data)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn get_cached_filler_episodes(
    pool: &SqlitePool,
    mal_id: i32,
) -> Result<Option<Vec<i32>>, AppError> {
    let now = current_unix_timestamp();
    let row = sqlx::query("SELECT filler_json, cached_at FROM filler_cache WHERE mal_id = ?1")
        .bind(mal_id)
        .fetch_optional(pool)
        .await?;

    if let Some(r) = row {
        let cached_at: i64 = r.get("cached_at");
        if now - cached_at < 604800 {
            let json_str: String = r.get("filler_json");
            if let Ok(episodes) = serde_json::from_str::<Vec<i32>>(&json_str) {
                return Ok(Some(episodes));
            }
        }
    }

    Ok(None)
}

pub async fn cache_filler_episodes(
    pool: &SqlitePool,
    mal_id: i32,
    episodes: &[i32],
) -> Result<(), AppError> {
    let now = current_unix_timestamp();
    let json_str = serde_json::to_string(episodes)
        .map_err(|e| AppError::Other(e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO filler_cache (mal_id, filler_json, cached_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(mal_id) DO UPDATE SET
            filler_json = excluded.filler_json,
            cached_at = excluded.cached_at
        "#,
    )
    .bind(mal_id)
    .bind(json_str)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

static PLAYLIST_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// `pl-<ms since epoch>-<per-process counter>-<random>`, all in hex.
fn new_playlist_id() -> String {
    use std::hash::{BuildHasher, Hasher};

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let counter = PLAYLIST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    // `RandomState` is seeded from the operating system's random source.
    let random = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();

    format!("pl-{:x}-{:x}-{:016x}", millis, counter, random)
}

pub async fn create_custom_playlist(
    pool: &SqlitePool,
    name: &str,
    description: Option<&str>,
) -> Result<CustomPlaylist, AppError> {
    let now = current_unix_timestamp();
    let id = new_playlist_id();
    sqlx::query(
        r#"
        INSERT INTO custom_playlists (id, name, description, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
    )
    .bind(&id)
    .bind(name)
    .bind(description)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(CustomPlaylist {
        id,
        name: name.to_string(),
        description: description.map(|s| s.to_string()),
        created_at: now,
        updated_at: now,
        item_count: 0,
        covers: vec![],
    })
}

pub async fn list_custom_playlists(
    pool: &SqlitePool,
) -> Result<Vec<CustomPlaylist>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT 
            p.id, p.name, p.description, p.created_at, p.updated_at,
            COUNT(i.id) as item_count,
            GROUP_CONCAT(DISTINCT i.series_cover) as covers_str
        FROM custom_playlists p
        LEFT JOIN custom_playlist_items i ON p.id = i.playlist_id
        GROUP BY p.id
        ORDER BY p.updated_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut playlists = Vec::new();
    for r in rows {
        let covers_str: Option<String> = r.get("covers_str");
        let covers = covers_str
            .map(|s| {
                s.split(',')
                    .filter(|c| !c.is_empty())
                    .take(4)
                    .map(|c| c.to_string())
                    .collect()
            })
            .unwrap_or_default();

        playlists.push(CustomPlaylist {
            id: r.get("id"),
            name: r.get("name"),
            description: r.get("description"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
            item_count: r.get("item_count"),
            covers,
        });
    }

    Ok(playlists)
}

pub async fn get_custom_playlist_items(
    pool: &SqlitePool,
    playlist_id: &str,
) -> Result<Vec<CustomPlaylistItem>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, playlist_id, anilist_id, mal_id, episode_num, episode_title,
               series_title, series_cover, category, sort_order, added_at
        FROM custom_playlist_items
        WHERE playlist_id = ?1
        ORDER BY sort_order ASC, added_at ASC
        "#,
    )
    .bind(playlist_id)
    .fetch_all(pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| CustomPlaylistItem {
            id: r.get("id"),
            playlist_id: r.get("playlist_id"),
            anilist_id: r.get("anilist_id"),
            mal_id: r.get("mal_id"),
            episode_num: r.get("episode_num"),
            episode_title: r.get("episode_title"),
            series_title: r.get("series_title"),
            series_cover: r.get("series_cover"),
            category: r.get("category"),
            sort_order: r.get("sort_order"),
            added_at: r.get("added_at"),
        })
        .collect();

    Ok(items)
}

/// Adds every item or none: the inserts share one transaction, and the first failure rolls
/// all of them back and is returned. Returns the number of items added.
pub async fn add_items_to_custom_playlist(
    pool: &SqlitePool,
    items: &[CustomPlaylistItem],
) -> Result<usize, AppError> {
    if items.is_empty() {
        return Ok(0);
    }

    let now = current_unix_timestamp();
    let mut tx = pool.begin().await?;

    for item in items {
        sqlx::query(
            r#"
            INSERT INTO custom_playlist_items
            (id, playlist_id, anilist_id, mal_id, episode_num, episode_title, series_title, series_cover, category, sort_order, added_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
        )
        .bind(&item.id)
        .bind(&item.playlist_id)
        .bind(item.anilist_id)
        .bind(item.mal_id)
        .bind(item.episode_num)
        .bind(&item.episode_title)
        .bind(&item.series_title)
        .bind(&item.series_cover)
        .bind(&item.category)
        .bind(item.sort_order)
        .bind(item.added_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            AppError::Database(format!(
                "could not add \"{}\" episode {} to the playlist: {}",
                item.series_title, item.episode_num, e
            ))
        })?;
    }

    let mut playlist_ids: Vec<&str> = items.iter().map(|i| i.playlist_id.as_str()).collect();
    playlist_ids.sort_unstable();
    playlist_ids.dedup();
    for playlist_id in playlist_ids {
        sqlx::query("UPDATE custom_playlists SET updated_at = ?1 WHERE id = ?2")
            .bind(now)
            .bind(playlist_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(items.len())
}

pub async fn add_item_to_custom_playlist(
    pool: &SqlitePool,
    item: CustomPlaylistItem,
) -> Result<(), AppError> {
    add_items_to_custom_playlist(pool, std::slice::from_ref(&item))
        .await
        .map(|_| ())
}

pub async fn remove_item_from_custom_playlist(
    pool: &SqlitePool,
    item_id: &str,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM custom_playlist_items WHERE id = ?1")
        .bind(item_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_custom_playlist(
    pool: &SqlitePool,
    playlist_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM custom_playlist_items WHERE playlist_id = ?1")
        .bind(playlist_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM custom_playlists WHERE id = ?1")
        .bind(playlist_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
