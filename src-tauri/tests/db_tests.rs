//! Tests of the real schema and migrations in `db.rs`.
//!
//! Every test uses a private in-memory database or a file in a fresh temp directory; none
//! touches the user's database.

use anix_lib::db;
use anix_lib::models::{
    AiringEntry, CustomPlaylistItem, DownloadRecord, HistoryEntry, NotificationPreference,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh directory under the system temp dir, removed again when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-db-test-{}-{}-{}",
            name,
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn db_path(&self) -> PathBuf {
        self.0.join("anix.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn memory_db() -> SqlitePool {
    db::init_db_in_memory().await.expect("in-memory database")
}

/// Opens `path` without running the app's migrations, to build an old-layout database.
async fn raw_pool(path: &PathBuf) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).create_if_missing(true))
        .await
        .expect("raw pool")
}

/// The `watch_history` table exactly as shipped before per-episode rows.
const OLD_WATCH_HISTORY: &str = r#"
    CREATE TABLE watch_history (
        anilist_id      INTEGER PRIMARY KEY,
        episode_number  REAL NOT NULL DEFAULT 1.0,
        episode_title   TEXT,
        provider        TEXT NOT NULL DEFAULT 'auto',
        category        TEXT NOT NULL DEFAULT 'sub',
        position_ms     INTEGER NOT NULL DEFAULT 0,
        duration_ms     INTEGER NOT NULL DEFAULT 0,
        updated_at      INTEGER NOT NULL DEFAULT 0,
        from_remote     INTEGER NOT NULL DEFAULT 0,
        title           TEXT NOT NULL DEFAULT '',
        cover           TEXT
    );
    CREATE INDEX idx_history_updated ON watch_history(updated_at DESC);
"#;

fn history(anilist_id: i32, episode: f64, position_ms: i64, duration_ms: i64, updated_at: i64) -> HistoryEntry {
    HistoryEntry {
        anilist_id,
        title: format!("Anime {}", anilist_id),
        cover: Some(format!("https://example.com/{}.jpg", anilist_id)),
        episode_number: episode,
        episode_title: Some(format!("Episode {}", episode)),
        provider: "kiwi".to_string(),
        category: "sub".to_string(),
        position_ms,
        duration_ms,
        updated_at,
        from_remote: false,
    }
}

async fn primary_key_columns(pool: &SqlitePool, table: &str) -> Vec<String> {
    let mut key: Vec<(i64, String)> = sqlx::query(&format!("PRAGMA table_info({})", table))
        .fetch_all(pool)
        .await
        .expect("table_info")
        .into_iter()
        .filter(|r| r.get::<i64, _>("pk") > 0)
        .map(|r| (r.get::<i64, _>("pk"), r.get::<String, _>("name")))
        .collect();
    key.sort();
    key.into_iter().map(|(_, name)| name).collect()
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query(sql).fetch_one(pool).await.expect("count").get::<i64, _>(0)
}

// ---------------------------------------------------------------------------------------
// Opening and migrating
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn new_database_file_uses_wal_and_the_per_episode_key() {
    let tmp = TempDir::new("fresh");
    let pool = db::init_db_at(&tmp.db_path()).await.expect("init");

    let journal: String = sqlx::query("PRAGMA journal_mode").fetch_one(&pool).await.unwrap().get(0);
    assert_eq!(journal.to_lowercase(), "wal");
    let synchronous: i64 = sqlx::query("PRAGMA synchronous").fetch_one(&pool).await.unwrap().get(0);
    assert_eq!(synchronous, 1, "synchronous must be NORMAL");

    assert_eq!(
        primary_key_columns(&pool, "watch_history").await,
        vec!["anilist_id".to_string(), "episode_number".to_string()]
    );

    for table in ["watchlist", "remote_library", "downloads", "notification_preferences",
                  "schedule_cache", "filler_cache", "custom_playlists", "custom_playlist_items"] {
        let n = count(&pool, &format!("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{}'", table)).await;
        assert_eq!(n, 1, "table {} must exist", table);
    }
    pool.close().await;
}

#[tokio::test]
async fn migration_from_one_row_per_anime_keeps_every_row() {
    let tmp = TempDir::new("migrate");
    let path = tmp.db_path();

    let old = raw_pool(&path).await;
    sqlx::query(OLD_WATCH_HISTORY).execute(&old).await.expect("old schema");
    // Two rows shaped like real ones, plus one that was watched to the end.
    sqlx::query(
        r#"
        INSERT INTO watch_history
            (anilist_id, episode_number, episode_title, provider, category, position_ms, duration_ms, updated_at, from_remote, title, cover)
        VALUES
            (196187,  9.0, 'Episode 9',  'hop',  'sub',   13580, 1456038, 1788549737000, 0, 'Show A', 'https://example.com/a.jpg'),
            (182205, 18.0, NULL,         'kiwi', 'dub',   10354, 1439855, 1787170353000, 0, 'Show B', NULL),
            (21,   1100.5, 'Special',    'bonk', 'sub', 1400000, 1440000, 1787000000000, 1, 'Show C', 'https://example.com/c.jpg')
        "#,
    )
    .execute(&old)
    .await
    .expect("old rows");
    old.close().await;

    let pool = db::init_db_at(&path).await.expect("migrate");

    assert_eq!(
        primary_key_columns(&pool, "watch_history").await,
        vec!["anilist_id".to_string(), "episode_number".to_string()]
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM watch_history").await, 3);

    let row = sqlx::query("SELECT * FROM watch_history WHERE anilist_id = 196187")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<f64, _>("episode_number"), 9.0);
    assert_eq!(row.get::<Option<String>, _>("episode_title").as_deref(), Some("Episode 9"));
    assert_eq!(row.get::<String, _>("provider"), "hop");
    assert_eq!(row.get::<String, _>("category"), "sub");
    assert_eq!(row.get::<i64, _>("position_ms"), 13580);
    assert_eq!(row.get::<i64, _>("duration_ms"), 1456038);
    assert_eq!(row.get::<i64, _>("updated_at"), 1788549737000);
    assert_eq!(row.get::<String, _>("title"), "Show A");
    assert_eq!(row.get::<Option<String>, _>("cover").as_deref(), Some("https://example.com/a.jpg"));
    assert_eq!(row.get::<i64, _>("completed"), 0);

    let row = sqlx::query("SELECT * FROM watch_history WHERE anilist_id = 182205")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("category"), "dub");
    assert_eq!(row.get::<Option<String>, _>("episode_title"), None);
    assert_eq!(row.get::<Option<String>, _>("cover"), None);

    let row = sqlx::query("SELECT * FROM watch_history WHERE anilist_id = 21")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<f64, _>("episode_number"), 1100.5);
    assert_eq!(row.get::<i64, _>("from_remote"), 1);
    assert_eq!(row.get::<i64, _>("completed"), 1, "a row past 85% is migrated as watched");

    // The migrated rows behave like new ones.
    assert_eq!(db::get_episode_progress(&pool, 196187, 9.0).await.unwrap(), Some((13580, 1456038)));
    assert_eq!(db::get_watched_episodes(&pool, 21).await.unwrap(), vec![1100.5]);
    assert!(db::get_watched_episodes(&pool, 196187).await.unwrap().is_empty());

    // A second episode of the same anime is now a second row.
    db::update_watch_progress(&pool, &history(196187, 10.0, 5000, 1456000, 1788549800000))
        .await
        .unwrap();
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM watch_history WHERE anilist_id = 196187").await, 2);
    pool.close().await;

    // Opening the migrated file again changes nothing.
    let pool = db::init_db_at(&path).await.expect("reopen");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM watch_history").await, 4);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM sqlite_master WHERE name = 'watch_history_migrating'").await, 0);
    pool.close().await;
}

#[tokio::test]
async fn migration_from_the_legacy_episode_column_copies_instead_of_dropping() {
    let tmp = TempDir::new("legacy");
    let path = tmp.db_path();

    let old = raw_pool(&path).await;
    sqlx::query(
        r#"
        CREATE TABLE watch_history (
            anilist_id  INTEGER PRIMARY KEY,
            episode     INTEGER NOT NULL,
            position_ms INTEGER NOT NULL DEFAULT 0,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            updated_at  INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO watch_history (anilist_id, episode, position_ms, duration_ms, updated_at)
        VALUES (101, 3, 600000, 1440000, 1700000000000),
               (202, 7, 1430000, 1440000, 1700000001000);
        "#,
    )
    .execute(&old)
    .await
    .expect("legacy schema");
    old.close().await;

    let pool = db::init_db_at(&path).await.expect("migrate legacy");

    assert_eq!(count(&pool, "SELECT COUNT(*) FROM watch_history").await, 2);
    assert_eq!(db::get_episode_progress(&pool, 101, 3.0).await.unwrap(), Some((600000, 1440000)));
    assert_eq!(db::get_episode_progress(&pool, 202, 7.0).await.unwrap(), Some((1430000, 1440000)));
    assert_eq!(db::get_watched_episodes(&pool, 202).await.unwrap(), vec![7.0]);

    // Columns the legacy table did not have get their defaults.
    let row = sqlx::query("SELECT provider, category, title, from_remote FROM watch_history WHERE anilist_id = 101")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("provider"), "auto");
    assert_eq!(row.get::<String, _>("category"), "sub");
    assert_eq!(row.get::<String, _>("title"), "");
    assert_eq!(row.get::<i64, _>("from_remote"), 0);

    let list = db::get_continue_watching(&pool).await.unwrap();
    assert_eq!(list.len(), 2);
    pool.close().await;
}

#[tokio::test]
async fn unknown_history_layout_is_set_aside_not_deleted() {
    let tmp = TempDir::new("unknown");
    let path = tmp.db_path();

    let old = raw_pool(&path).await;
    sqlx::query(
        r#"
        CREATE TABLE watch_history (media TEXT PRIMARY KEY, seconds INTEGER);
        INSERT INTO watch_history VALUES ('a', 1), ('b', 2);
        "#,
    )
    .execute(&old)
    .await
    .unwrap();
    old.close().await;

    let pool = db::init_db_at(&path).await.expect("init");

    assert_eq!(count(&pool, "SELECT COUNT(*) FROM watch_history").await, 0);
    let parked: String = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'watch_history_unrecognized_%'",
    )
    .fetch_one(&pool)
    .await
    .expect("the old table is kept under another name")
    .get(0);
    assert_eq!(count(&pool, &format!("SELECT COUNT(*) FROM {}", parked)).await, 2);
    pool.close().await;
}

#[tokio::test]
async fn opening_a_file_that_is_not_a_database_reports_the_path() {
    let tmp = TempDir::new("garbage");
    let path = tmp.db_path();
    std::fs::write(&path, vec![b'x'; 8192]).unwrap();

    let err = db::init_db_at(&path).await.expect_err("must fail");
    let message = err.to_string();
    assert!(message.contains(&path.display().to_string()), "message was: {}", message);
    assert!(message.starts_with("Database error: could not"), "message was: {}", message);

    // The file was not replaced or emptied.
    assert_eq!(std::fs::read(&path).unwrap(), vec![b'x'; 8192]);
}

#[tokio::test]
async fn old_download_rows_are_normalised_to_milliseconds_and_upper_case() {
    let pool = memory_db().await;
    sqlx::query(
        r#"
        INSERT INTO downloads (id, anilist_id, episode_num, series_title, provider, category, quality, status, created_at, updated_at)
        VALUES ('secs', 1, 1, 'A', 'auto', 'sub', '720p', 'queued',    1790408146,    1790408157),
               ('ms',   1, 2, 'A', 'auto', 'sub', '720p', 'COMPLETED', 1790408146000, 1790408157000)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    db::run_migrations(&pool).await.expect("re-run migrations");

    let secs = db::get_download_by_id(&pool, "secs").await.unwrap().unwrap();
    assert_eq!(secs.created_at, 1790408146000);
    assert_eq!(secs.updated_at, 1790408157000);
    assert_eq!(secs.status, "QUEUED");

    let ms = db::get_download_by_id(&pool, "ms").await.unwrap().unwrap();
    assert_eq!(ms.created_at, 1790408146000);
    assert_eq!(ms.updated_at, 1790408157000);
    assert_eq!(ms.status, "COMPLETED");
}

// ---------------------------------------------------------------------------------------
// Watch history
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn progress_is_kept_per_episode() {
    let pool = memory_db().await;

    db::update_watch_progress(&pool, &history(101, 1.0, 120_000, 1_440_000, 1000)).await.unwrap();
    db::update_watch_progress(&pool, &history(101, 2.0, 450_000, 1_440_000, 2000)).await.unwrap();
    db::update_watch_progress(&pool, &history(101, 12.5, 30_000, 600_000, 3000)).await.unwrap();

    assert_eq!(db::get_episode_progress(&pool, 101, 1.0).await.unwrap(), Some((120_000, 1_440_000)));
    assert_eq!(db::get_episode_progress(&pool, 101, 2.0).await.unwrap(), Some((450_000, 1_440_000)));
    assert_eq!(db::get_episode_progress(&pool, 101, 12.5).await.unwrap(), Some((30_000, 600_000)));
    assert_eq!(db::get_episode_progress(&pool, 101, 3.0).await.unwrap(), None);
    assert_eq!(db::get_episode_progress(&pool, 999, 1.0).await.unwrap(), None);

    // Saving the same episode again updates its row instead of adding one.
    db::update_watch_progress(&pool, &history(101, 1.0, 300_000, 1_440_000, 4000)).await.unwrap();
    assert_eq!(db::get_episode_progress(&pool, 101, 1.0).await.unwrap(), Some((300_000, 1_440_000)));
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM watch_history WHERE anilist_id = 101").await, 3);

    // A save without a known duration keeps the duration already stored.
    db::update_watch_progress(&pool, &history(101, 1.0, 310_000, 0, 5000)).await.unwrap();
    assert_eq!(db::get_episode_progress(&pool, 101, 1.0).await.unwrap(), Some((310_000, 1_440_000)));
}

#[tokio::test]
async fn watched_means_at_least_85_percent() {
    assert!(!db::is_episode_watched(1_000, 1_440_000));
    assert!(!db::is_episode_watched(1_223_999, 1_440_000));
    assert!(db::is_episode_watched(1_224_000, 1_440_000));
    assert!(db::is_episode_watched(1_440_000, 1_440_000));
    assert!(!db::is_episode_watched(5_000, 0), "an unknown duration never counts");
    assert!(!db::is_episode_watched(0, 0));

    let pool = memory_db().await;
    let duration = 1_440_000;

    db::update_watch_progress(&pool, &history(7, 1.0, 1_000, duration, 1)).await.unwrap();
    db::update_watch_progress(&pool, &history(7, 2.0, 1_223_999, duration, 2)).await.unwrap();
    db::update_watch_progress(&pool, &history(7, 3.0, 1_224_000, duration, 3)).await.unwrap();
    db::update_watch_progress(&pool, &history(7, 4.0, 1_439_000, duration, 4)).await.unwrap();
    db::update_watch_progress(&pool, &history(7, 5.0, 90_000, 0, 5)).await.unwrap();

    assert_eq!(
        db::get_watched_episodes(&pool, 7).await.unwrap(),
        vec![3.0, 4.0],
        "one second of playback, 84.9% and an unknown duration are not watched"
    );
    assert!(db::get_watched_episodes(&pool, 8).await.unwrap().is_empty());

    // Crossing the threshold later marks the episode.
    db::update_watch_progress(&pool, &history(7, 1.0, 1_300_000, duration, 6)).await.unwrap();
    assert_eq!(db::get_watched_episodes(&pool, 7).await.unwrap(), vec![1.0, 3.0, 4.0]);

    // Replaying a watched episode from the start does not un-watch it, but its saved
    // position follows the replay.
    db::update_watch_progress(&pool, &history(7, 3.0, 5_000, duration, 7)).await.unwrap();
    assert_eq!(db::get_watched_episodes(&pool, 7).await.unwrap(), vec![1.0, 3.0, 4.0]);
    assert_eq!(db::get_episode_progress(&pool, 7, 3.0).await.unwrap(), Some((5_000, duration)));
}

#[tokio::test]
async fn continue_watching_returns_one_entry_per_anime_newest_first() {
    let pool = memory_db().await;
    let d = 1_440_000;

    // Anime 1: episode 1 finished, episode 2 in progress (the usual case).
    db::update_watch_progress(&pool, &history(1, 1.0, 1_400_000, d, 100)).await.unwrap();
    db::update_watch_progress(&pool, &history(1, 2.0, 300_000, d, 200)).await.unwrap();
    // Anime 2: only a finished episode, nothing later started.
    db::update_watch_progress(&pool, &history(2, 5.0, 1_430_000, d, 300)).await.unwrap();
    // Anime 3: episode 10 in progress, then an old episode rewatched to the end.
    db::update_watch_progress(&pool, &history(3, 10.0, 500_000, d, 400)).await.unwrap();
    db::update_watch_progress(&pool, &history(3, 3.0, 1_435_000, d, 500)).await.unwrap();
    // Anime 4: opened but never played.
    db::update_watch_progress(&pool, &history(4, 1.0, 0, d, 600)).await.unwrap();
    // Anime 5: an earlier episode left unfinished, a later one finished most recently.
    db::update_watch_progress(&pool, &history(5, 1.0, 200_000, d, 50)).await.unwrap();
    db::update_watch_progress(&pool, &history(5, 2.0, 1_439_000, d, 60)).await.unwrap();

    let list = db::get_continue_watching(&pool).await.unwrap();
    let picked: Vec<(i32, f64)> = list.iter().map(|h| (h.anilist_id, h.episode_number)).collect();

    assert_eq!(
        picked,
        vec![(3, 10.0), (2, 5.0), (1, 2.0), (5, 2.0)],
        "one entry per anime, ordered by the anime's most recent playback"
    );

    let anime3 = &list[0];
    assert_eq!(anime3.position_ms, 500_000, "the unfinished later episode replaces the finished rewatch");
    let anime2 = &list[1];
    assert_eq!(anime2.position_ms, 1_430_000, "a finished episode is listed when nothing later is in progress");
    let anime1 = &list[2];
    assert_eq!(anime1.title, "Anime 1");
    assert_eq!(anime1.episode_title.as_deref(), Some("Episode 2"));
    assert_eq!(anime1.provider, "kiwi");
    assert_eq!(anime1.category, "sub");
    assert_eq!(anime1.duration_ms, d);
    assert_eq!(anime1.updated_at, 200);
    assert!(!anime1.from_remote);
}

#[tokio::test]
async fn continue_watching_is_limited_to_twenty_anime() {
    let pool = memory_db().await;
    for id in 1..=25 {
        db::update_watch_progress(&pool, &history(id, 1.0, 60_000, 1_440_000, id as i64)).await.unwrap();
    }
    let list = db::get_continue_watching(&pool).await.unwrap();
    assert_eq!(list.len(), 20);
    assert_eq!(list[0].anilist_id, 25);
    assert_eq!(list[19].anilist_id, 6);
}

// ---------------------------------------------------------------------------------------
// Watchlist, notifications, caches
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn watchlist_operations() {
    let pool = memory_db().await;

    assert!(!db::is_in_watchlist(&pool, 202).await.unwrap());

    let before = db::current_unix_millis();
    db::add_to_watchlist(&pool, 202, "Steins;Gate", Some("https://example.com/sg.jpg"), Some("TV"), Some(91))
        .await
        .unwrap();

    assert!(db::is_in_watchlist(&pool, 202).await.unwrap());

    let list = db::get_watchlist(&pool).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "Steins;Gate");
    assert_eq!(list[0].average_score, Some(91));
    assert!(list[0].added_at >= before, "added_at is in milliseconds");

    db::remove_from_watchlist(&pool, 202).await.unwrap();
    assert!(!db::is_in_watchlist(&pool, 202).await.unwrap());
}

#[tokio::test]
async fn notification_preferences_and_filler_cache() {
    let pool = memory_db().await;

    let pref = NotificationPreference {
        media_id: 404,
        enabled: true,
        media_title: "Jujutsu Kaisen".to_string(),
        cover_image: None,
    };
    db::save_notification_preference(&pool, &pref).await.unwrap();
    let fetched = db::get_notification_preference(&pool, 404).await.unwrap().unwrap();
    assert!(fetched.enabled);
    assert_eq!(fetched.media_title, "Jujutsu Kaisen");
    assert_eq!(db::list_notification_preferences(&pool).await.unwrap().len(), 1);

    db::cache_filler_episodes(&pool, 505, &[10, 11, 12, 13]).await.unwrap();
    let fillers = db::get_cached_filler_episodes(&pool, 505).await.unwrap().unwrap();
    assert_eq!(fillers, vec![10, 11, 12, 13]);
}

#[tokio::test]
async fn schedule_cache_serves_fresh_rows_and_deletes_expired_ones_on_write() {
    let pool = memory_db().await;
    let entries: Vec<AiringEntry> = Vec::new();
    let now = db::current_unix_timestamp();

    db::cache_schedule(&pool, 100, 200, &entries).await.unwrap();
    assert!(db::get_cached_schedule(&pool, 100, 200).await.unwrap().is_some());
    assert!(db::get_cached_schedule(&pool, 100, 201).await.unwrap().is_none());

    // Three windows from earlier days: two expired, one still inside the TTL.
    for (from_ts, age) in [(1, db::SCHEDULE_CACHE_TTL_S + 10), (2, 5 * 86_400), (3, db::SCHEDULE_CACHE_TTL_S - 60)] {
        sqlx::query("INSERT INTO schedule_cache (from_ts, to_ts, data, cached_at) VALUES (?1, ?2, '[]', ?3)")
            .bind(from_ts)
            .bind(from_ts + 1)
            .bind(now - age)
            .execute(&pool)
            .await
            .unwrap();
    }
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM schedule_cache").await, 4);

    // An expired row is not served even before it is deleted.
    assert!(db::get_cached_schedule(&pool, 1, 2).await.unwrap().is_none());
    assert!(db::get_cached_schedule(&pool, 3, 4).await.unwrap().is_some());

    db::cache_schedule(&pool, 300, 400, &entries).await.unwrap();

    let left: Vec<i64> = sqlx::query("SELECT from_ts FROM schedule_cache ORDER BY from_ts")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.get::<i64, _>(0))
        .collect();
    assert_eq!(left, vec![3, 100, 300], "writing drops exactly the expired windows");
}

// ---------------------------------------------------------------------------------------
// Playlists
// ---------------------------------------------------------------------------------------

fn playlist_item(id: &str, playlist_id: &str, episode: f64) -> CustomPlaylistItem {
    CustomPlaylistItem {
        id: id.to_string(),
        playlist_id: playlist_id.to_string(),
        anilist_id: 1,
        mal_id: Some(2),
        episode_num: episode,
        episode_title: None,
        series_title: "Frieren".to_string(),
        series_cover: Some("https://example.com/f.jpg".to_string()),
        category: "sub".to_string(),
        sort_order: episode as i64,
        added_at: 1_700_000_000,
    }
}

#[tokio::test]
async fn playlists_created_in_the_same_instant_get_different_ids() {
    let pool = memory_db().await;

    let mut ids = std::collections::HashSet::new();
    for i in 0..200 {
        let playlist = db::create_custom_playlist(&pool, &format!("List {}", i), None)
            .await
            .expect("creating playlists back to back must not collide");
        assert!(playlist.id.starts_with("pl-"));
        assert!(ids.insert(playlist.id));
    }
    assert_eq!(db::list_custom_playlists(&pool).await.unwrap().len(), 200);
}

#[tokio::test]
async fn adding_many_playlist_items_is_all_or_nothing() {
    let pool = memory_db().await;
    let playlist = db::create_custom_playlist(&pool, "Queue", Some("to watch")).await.unwrap();

    let good: Vec<CustomPlaylistItem> = (1..=5)
        .map(|n| playlist_item(&format!("item-{}", n), &playlist.id, n as f64))
        .collect();
    assert_eq!(db::add_items_to_custom_playlist(&pool, &good).await.unwrap(), 5);
    assert_eq!(db::get_custom_playlist_items(&pool, &playlist.id).await.unwrap().len(), 5);

    // The third item repeats an existing id, so none of the batch may be stored.
    let bad = vec![
        playlist_item("item-6", &playlist.id, 6.0),
        playlist_item("item-7", &playlist.id, 7.0),
        playlist_item("item-3", &playlist.id, 8.0),
        playlist_item("item-9", &playlist.id, 9.0),
    ];
    let err = db::add_items_to_custom_playlist(&pool, &bad).await.expect_err("duplicate id must fail");
    assert!(err.to_string().contains("Frieren"), "error was: {}", err);
    assert_eq!(db::get_custom_playlist_items(&pool, &playlist.id).await.unwrap().len(), 5);

    // Items for a playlist that does not exist are refused as well.
    let orphan = vec![playlist_item("item-10", "pl-missing", 1.0)];
    assert!(db::add_items_to_custom_playlist(&pool, &orphan).await.is_err());
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM custom_playlist_items").await, 5);

    assert_eq!(db::add_items_to_custom_playlist(&pool, &[]).await.unwrap(), 0);

    // The single-item entry point reports failures too.
    assert!(db::add_item_to_custom_playlist(&pool, playlist_item("item-1", &playlist.id, 1.0)).await.is_err());
    db::add_item_to_custom_playlist(&pool, playlist_item("item-11", &playlist.id, 11.0)).await.unwrap();

    let listed = db::list_custom_playlists(&pool).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].item_count, 6);

    db::remove_item_from_custom_playlist(&pool, "item-11").await.unwrap();
    assert_eq!(db::get_custom_playlist_items(&pool, &playlist.id).await.unwrap().len(), 5);

    db::delete_custom_playlist(&pool, &playlist.id).await.unwrap();
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM custom_playlist_items").await, 0);
    assert!(db::list_custom_playlists(&pool).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------------------

fn download(id: &str, episode: f64) -> DownloadRecord {
    DownloadRecord {
        id: id.to_string(),
        anilist_id: 303,
        episode_num: episode,
        episode_title: Some(format!("Episode {}", episode)),
        series_title: "Frieren".to_string(),
        series_cover: None,
        provider: "auto".to_string(),
        category: "sub".to_string(),
        quality: "1080p".to_string(),
        status: "QUEUED".to_string(),
        progress: 0.0,
        file_path: None,
        file_size: None,
        duration_s: None,
        error_msg: None,
        created_at: 0,
        updated_at: 0,
        ..Default::default()
    }
}

#[tokio::test]
async fn download_timestamps_are_written_by_the_backend_in_milliseconds() {
    let pool = memory_db().await;
    let before = db::current_unix_millis();

    // The three callers used to send seconds, milliseconds, or nothing useful.
    let mut seconds = download("dl-seconds", 1.0);
    seconds.created_at = 1_790_408_146;
    seconds.updated_at = 1_790_408_146;
    let mut millis = download("dl-millis", 2.0);
    millis.created_at = 1_790_408_146_000;
    millis.updated_at = 1_790_408_146_000;
    let zero = download("dl-zero", 3.0);

    for record in [&seconds, &millis, &zero] {
        assert_eq!(db::create_download(&pool, record).await.unwrap(), record.id);
    }
    let after = db::current_unix_millis();

    for id in ["dl-seconds", "dl-millis", "dl-zero"] {
        let stored = db::get_download_by_id(&pool, id).await.unwrap().unwrap();
        assert!(stored.created_at >= before && stored.created_at <= after, "{} created_at {}", id, stored.created_at);
        assert!(stored.updated_at >= before && stored.updated_at <= after, "{} updated_at {}", id, stored.updated_at);
    }
}

#[tokio::test]
async fn download_record_from_the_playlist_tab_is_accepted() {
    // No `progress`, a lower-case status, second timestamps and an extra `mal_id`.
    let record: DownloadRecord = serde_json::from_value(serde_json::json!({
        "id": "8f1c0f0e-0000-4000-8000-000000000001",
        "anilist_id": 9,
        "mal_id": 12,
        "series_title": "Show",
        "series_cover": null,
        "episode_num": 4,
        "episode_title": null,
        "category": "dub",
        "provider": "auto",
        "quality": "1080p",
        "status": "queued",
        "file_path": null,
        "file_size": null,
        "duration_s": null,
        "error_msg": null,
        "created_at": 1790408146,
        "updated_at": 1790408146
    }))
    .expect("the record must deserialize without `progress`");

    let pool = memory_db().await;
    db::create_download(&pool, &record).await.unwrap();

    let stored = db::get_download_by_id(&pool, &record.id).await.unwrap().unwrap();
    assert_eq!(stored.status, "QUEUED");
    assert_eq!(stored.progress, 0.0);
    assert_eq!(stored.category, "dub");
}

#[tokio::test]
async fn download_status_transitions() {
    let pool = memory_db().await;
    db::create_download(&pool, &download("dl-1", 5.0)).await.unwrap();

    let listed = db::get_downloads(&pool, 303).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, "QUEUED");

    // Progress written for a row that is not running is ignored.
    db::update_download_progress(&pool, "dl-1", Some(0.5), Some(1000)).await.unwrap();
    let row = db::get_download_by_id(&pool, "dl-1").await.unwrap().unwrap();
    assert_eq!((row.progress, row.file_size), (0.0, None));

    // QUEUED -> DOWNLOADING by claiming.
    let claimed = db::claim_next_queued_download(&pool).await.unwrap().expect("one queued row");
    assert_eq!(claimed.id, "dl-1");
    assert_eq!(claimed.status, "DOWNLOADING");
    assert!(db::claim_next_queued_download(&pool).await.unwrap().is_none(), "a row is claimed once");

    db::update_download_progress(&pool, "dl-1", Some(0.5), Some(1000)).await.unwrap();
    db::update_download_progress(&pool, "dl-1", Some(7.0), None).await.unwrap();
    let row = db::get_download_by_id(&pool, "dl-1").await.unwrap().unwrap();
    assert_eq!(row.status, "DOWNLOADING");
    assert_eq!(row.progress, 1.0, "progress is a fraction and is clamped");
    assert_eq!(row.file_size, Some(1000));

    // DOWNLOADING -> FAILED keeps the reason for the UI.
    db::update_download_status(&pool, "dl-1", "FAILED", None, None, None, None, Some("ffmpeg failed (exit code 8): 403"))
        .await
        .unwrap();
    let row = db::get_download_by_id(&pool, "dl-1").await.unwrap().unwrap();
    assert_eq!(row.status, "FAILED");
    assert_eq!(row.error_msg.as_deref(), Some("ffmpeg failed (exit code 8): 403"));

    // A failed row no longer takes progress and is not handed out by the queue.
    db::update_download_progress(&pool, "dl-1", Some(0.1), Some(5)).await.unwrap();
    assert_eq!(db::get_download_by_id(&pool, "dl-1").await.unwrap().unwrap().file_size, Some(1000));
    assert!(db::claim_next_queued_download(&pool).await.unwrap().is_none());

    // FAILED -> QUEUED (retry) -> DOWNLOADING -> COMPLETED; the old error is cleared.
    db::update_download_status(&pool, "dl-1", "queued", None, None, None, None, None).await.unwrap();
    let claimed = db::claim_next_queued_download(&pool).await.unwrap().expect("retry is claimed");
    assert_eq!(claimed.error_msg, None);
    assert_eq!(claimed.progress, 0.0);

    db::update_download_status(&pool, "dl-1", "COMPLETED", Some(1.0), Some("/videos/frieren.mkv"), Some(350_000_000), Some(1420.0), None)
        .await
        .unwrap();
    let row = db::get_download_by_id(&pool, "dl-1").await.unwrap().unwrap();
    assert_eq!(row.status, "COMPLETED");
    assert_eq!(row.progress, 1.0);
    assert_eq!(row.file_path.as_deref(), Some("/videos/frieren.mkv"));
    assert_eq!(row.file_size, Some(350_000_000));
    assert_eq!(row.duration_s, Some(1420.0));

    assert!(db::update_download_status(&pool, "dl-1", "PAUSED", None, None, None, None, None).await.is_err());
    assert_eq!(db::get_download_by_id(&pool, "dl-1").await.unwrap().unwrap().status, "COMPLETED");

    db::delete_download(&pool, "dl-1").await.unwrap();
    assert!(db::get_download_by_id(&pool, "dl-1").await.unwrap().is_none());
    assert!(db::get_all_downloads(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn queue_hands_out_rows_oldest_first() {
    let pool = memory_db().await;
    for (id, episode) in [("a", 1.0), ("b", 2.0), ("c", 3.0)] {
        db::create_download(&pool, &download(id, episode)).await.unwrap();
    }
    // Make the order explicit rather than relying on the clock ticking between inserts.
    sqlx::query("UPDATE downloads SET created_at = CASE id WHEN 'a' THEN 30 WHEN 'b' THEN 10 ELSE 20 END")
        .execute(&pool)
        .await
        .unwrap();

    let mut order = Vec::new();
    while let Some(record) = db::claim_next_queued_download(&pool).await.unwrap() {
        order.push(record.id);
    }
    assert_eq!(order, vec!["b", "c", "a"]);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM downloads WHERE status = 'DOWNLOADING'").await, 3);
}

#[tokio::test]
async fn startup_reconciliation_requeues_interrupted_downloads_only() {
    let pool = memory_db().await;
    for (id, episode) in [("running", 1.0), ("waiting", 2.0), ("done", 3.0), ("broken", 4.0)] {
        db::create_download(&pool, &download(id, episode)).await.unwrap();
    }
    db::update_download_status(&pool, "running", "DOWNLOADING", Some(0.4), None, Some(123_456), None, None).await.unwrap();
    db::update_download_status(&pool, "done", "COMPLETED", Some(1.0), Some("/videos/3.mkv"), Some(999), None, None).await.unwrap();
    db::update_download_status(&pool, "broken", "FAILED", None, None, None, None, Some("HTTP 403")).await.unwrap();

    assert_eq!(db::reconcile_stale_downloads(&pool).await.unwrap(), 1);

    let running = db::get_download_by_id(&pool, "running").await.unwrap().unwrap();
    assert_eq!(running.status, "QUEUED");
    assert_eq!(running.progress, 0.0);
    assert_eq!(running.file_size, None);

    assert_eq!(db::get_download_by_id(&pool, "waiting").await.unwrap().unwrap().status, "QUEUED");
    let done = db::get_download_by_id(&pool, "done").await.unwrap().unwrap();
    assert_eq!((done.status.as_str(), done.file_path.as_deref()), ("COMPLETED", Some("/videos/3.mkv")));
    let broken = db::get_download_by_id(&pool, "broken").await.unwrap().unwrap();
    assert_eq!((broken.status.as_str(), broken.error_msg.as_deref()), ("FAILED", Some("HTTP 403")));

    assert_eq!(db::reconcile_stale_downloads(&pool).await.unwrap(), 0, "nothing left to reconcile");
}

// ---------------------------------------------------------------------------------------
// List statuses on the watchlist
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn watchlist_rows_from_1_2_0_become_plan_to_watch() {
    let tmp = TempDir::new("watchlist-status");
    let path = tmp.db_path();
    let old = raw_pool(&path).await;
    sqlx::query(
        r#"
        CREATE TABLE watchlist (
            anilist_id      INTEGER PRIMARY KEY,
            title           TEXT NOT NULL,
            cover           TEXT,
            format          TEXT,
            average_score   INTEGER,
            added_at        INTEGER NOT NULL
        );
        INSERT INTO watchlist VALUES (1, 'One', NULL, 'TV', 70, 1000), (2, 'Two', 'c.jpg', NULL, NULL, 2000);
        "#,
    )
    .execute(&old)
    .await
    .unwrap();
    old.close().await;

    let pool = db::init_db_at(&path).await.expect("migrate");
    let list = db::get_watchlist(&pool).await.unwrap();
    assert_eq!(list.len(), 2);
    for row in &list {
        assert_eq!((row.status.as_str(), row.synced), ("PLANNING", true), "{}", row.title);
    }
    assert_eq!(db::get_watchlist_status(&pool, 2).await.unwrap(), Some(("PLANNING".to_string(), true)));
    pool.close().await;

    // Running the migration again changes nothing.
    let pool = db::init_db_at(&path).await.expect("reopen");
    assert_eq!(db::get_watchlist(&pool).await.unwrap().len(), 2);
    pool.close().await;
}

#[tokio::test]
async fn watchlist_status_set_get_and_sync_flag() {
    let pool = memory_db().await;
    assert_eq!(db::get_watchlist_status(&pool, 5).await.unwrap(), None);

    db::set_watchlist_status(&pool, 5, "Five", None, Some("TV"), Some(60), "CURRENT", false).await.unwrap();
    assert_eq!(db::get_watchlist_status(&pool, 5).await.unwrap(), Some(("CURRENT".to_string(), false)));
    assert!(db::is_in_watchlist(&pool, 5).await.unwrap());
    assert_eq!(db::unsynced_watchlist(&pool).await.unwrap(), vec![(5, "CURRENT".to_string())]);

    // A remote status does not overwrite one still waiting to be sent.
    assert!(!db::take_remote_status(&pool, 5, "DROPPED").await.unwrap());
    // Marking an older status as sent does nothing once it changed again.
    db::set_watchlist_status(&pool, 5, "Five", None, None, None, "COMPLETED", false).await.unwrap();
    db::mark_watchlist_synced(&pool, 5, "CURRENT").await.unwrap();
    assert_eq!(db::get_watchlist_status(&pool, 5).await.unwrap(), Some(("COMPLETED".to_string(), false)));
    db::mark_watchlist_synced(&pool, 5, "COMPLETED").await.unwrap();
    assert!(db::unsynced_watchlist(&pool).await.unwrap().is_empty());
    assert!(db::take_remote_status(&pool, 5, "PAUSED").await.unwrap());
    // Remote statuses never add anime to the watchlist.
    assert!(!db::take_remote_status(&pool, 6, "PAUSED").await.unwrap());

    // The row keeps its title and added time; an unknown status is refused.
    let row = &db::get_watchlist(&pool).await.unwrap()[0];
    assert_eq!((row.title.as_str(), row.status.as_str(), row.average_score), ("Five", "PAUSED", Some(60)));
    assert!(db::set_watchlist_status(&pool, 5, "Five", None, None, None, "REPEATING", false).await.is_err());
    // A plain bookmark is Plan to watch and in sync.
    db::add_to_watchlist(&pool, 7, "Seven", None, None, None).await.unwrap();
    assert_eq!(db::get_watchlist_status(&pool, 7).await.unwrap(), Some(("PLANNING".to_string(), true)));
}

#[tokio::test]
async fn watch_times_keep_the_days_a_rewatch_moves() {
    let pool = memory_db().await;
    // Episode 1 was watched on an earlier day (a recorded watch, then the row's time moved).
    let earlier = 1_700_000_000_000_i64;
    sqlx::query("INSERT INTO watch_events (anilist_id, episode_number, day, watched_at) VALUES (101, 1.0, ?1, ?2)")
        .bind(earlier / 86_400_000)
        .bind(earlier)
        .execute(&pool)
        .await
        .unwrap();
    // Rewatched now, twice (progress saves near the end): one new event for today.
    let now = db::current_unix_millis();
    db::update_watch_progress(&pool, &history(101, 1.0, 1_400_000, 1_440_000, now)).await.unwrap();
    db::update_watch_progress(&pool, &history(101, 1.0, 1_430_000, 1_440_000, now)).await.unwrap();
    // Not finished: no event.
    db::update_watch_progress(&pool, &history(101, 2.0, 30_000, 1_440_000, now)).await.unwrap();
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM watch_events").await, 2);

    let times = db::get_watch_times(&pool).await.unwrap();
    assert!(times.contains(&earlier), "the earlier day is kept");
    assert!(times.iter().any(|t| *t >= now), "today's watch is there");

    // Marking by hand counts as watching today, once.
    db::set_episodes_watched(&pool, 202, "Other", None, &[1.0, 2.0], true).await.unwrap();
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM watch_events WHERE anilist_id = 202").await, 2);
}
