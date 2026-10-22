//! Backup and restore: a round trip into a fresh profile, the version check, and logins only
//! when sealed with a passphrase.

use anix_lib::backup;
use anix_lib::db;
use anix_lib::models::{CustomPlaylistItem, HistoryEntry};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Profile {
    dir: PathBuf,
    pool: SqlitePool,
}

impl Profile {
    async fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-backup-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir, pool: db::init_db_in_memory().await.unwrap() }
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    fn settings(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.settings_path()).unwrap_or("{}".into())).unwrap()
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn count(pool: &SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {}", table)).fetch_one(pool).await.unwrap()
}

async fn filled() -> Profile {
    let profile = Profile::new().await;
    let pool = &profile.pool;
    db::add_to_watchlist(pool, 1, "One", Some("c1.jpg"), Some("TV"), Some(70)).await.unwrap();
    db::set_watchlist_status(pool, 2, "Two", None, None, None, "COMPLETED", false).await.unwrap();
    db::set_watchlist_status(pool, -100_005_300, "Stand-in", None, None, None, "PAUSED", true).await.unwrap();
    for (episode, position) in [(1.0, 1_400_000), (2.0, 300_000), (12.5, 10)] {
        db::update_watch_progress(
            pool,
            &HistoryEntry {
                anilist_id: 1,
                title: "One".into(),
                episode_number: episode,
                provider: "auto".into(),
                category: "sub".into(),
                position_ms: position,
                duration_ms: 1_440_000,
                updated_at: db::current_unix_millis(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let playlist = db::create_custom_playlist(pool, "Weekend", Some("Two episodes")).await.unwrap();
    let item = |n: f64| CustomPlaylistItem {
        id: format!("item-{}", n),
        playlist_id: playlist.id.clone(),
        anilist_id: 1,
        mal_id: Some(5100),
        episode_num: n,
        episode_title: None,
        series_title: "One".into(),
        series_cover: None,
        category: "sub".into(),
        sort_order: n as i64,
        added_at: 0,
    };
    db::add_items_to_custom_playlist(pool, &[item(1.0), item(2.0)]).await.unwrap();

    let settings = json!({
        "theme": "mocha",
        "anilist_token": "secret-anilist",
        "mal_access_token": "secret-mal",
        "mal_refresh_token": "secret-refresh",
        "anilist_viewer_name": "viewer",
    });
    std::fs::write(profile.settings_path(), settings.to_string()).unwrap();
    profile
}

#[tokio::test]
async fn backup_round_trip_restores_the_same_counts_without_logins() {
    let source = filled().await;
    let made = backup::create(&source.pool, &source.settings_path(), json!(["frieren", "one piece"]), None)
        .await
        .unwrap();
    let bytes = backup::to_bytes(&made, true).unwrap();
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "gzip");
    let text = String::from_utf8_lossy(&backup::to_bytes(&made, false).unwrap()).to_string();
    // No token, and no login at all, in a file without a passphrase.
    for secret in ["secret-anilist", "secret-mal", "secret-refresh", "anilist_viewer_name"] {
        assert!(!text.contains(secret), "{} leaked", secret);
    }
    assert!(made.logins.is_none());

    let target = Profile::new().await;
    std::fs::write(target.settings_path(), json!({ "theme": "dark", "mal_access_token": "kept" }).to_string()).unwrap();
    let parsed = backup::from_bytes(&bytes).unwrap();
    let summary = backup::restore(&target.pool, &target.settings_path(), &parsed, None).await.unwrap();

    for table in backup::TABLES {
        assert_eq!(count(&target.pool, table).await, count(&source.pool, table).await, "{}", table);
        assert_eq!(summary.counts[table] as i64, count(&source.pool, table).await, "{}", table);
    }
    assert_eq!(count(&target.pool, "watchlist").await, 3);
    assert_eq!(count(&target.pool, "watch_history").await, 3);
    assert_eq!(db::get_watchlist(&target.pool).await.unwrap(), db::get_watchlist(&source.pool).await.unwrap());
    assert_eq!(db::get_watched_episodes(&target.pool, 1).await.unwrap(), vec![1.0]);
    assert_eq!(db::get_episode_progress(&target.pool, 1, 12.5).await.unwrap(), Some((10, 1_440_000)));
    let playlists = db::list_custom_playlists(&target.pool).await.unwrap();
    assert_eq!((playlists[0].name.as_str(), playlists[0].description.as_deref()), ("Weekend", Some("Two episodes")));
    assert_eq!(db::get_custom_playlist_items(&target.pool, &playlists[0].id).await.unwrap().len(), 2);
    assert_eq!(summary.search_history, json!(["frieren", "one piece"]));
    assert!(!summary.has_logins && !summary.logins_restored);

    // Settings come back; the target's own login stays, none comes from the file.
    let settings = target.settings();
    assert_eq!(settings["theme"], "mocha");
    assert_eq!(settings["mal_access_token"], "kept");
    assert!(settings.get("anilist_token").is_none());

    // Restoring twice does not duplicate anything.
    backup::restore(&target.pool, &target.settings_path(), &parsed, None).await.unwrap();
    assert_eq!(count(&target.pool, "watch_history").await, 3);
    assert_eq!(count(&target.pool, "custom_playlist_items").await, 2);
}

#[tokio::test]
async fn backup_restore_keeps_newer_items_and_progress() {
    let profile = filled().await;
    let pool = &profile.pool;
    let made = backup::create(pool, &profile.settings_path(), json!([]), None).await.unwrap();

    // After the backup: one more playlist item, and episode 2 finished.
    let playlist = db::list_custom_playlists(pool).await.unwrap().remove(0);
    let mut item = db::get_custom_playlist_items(pool, &playlist.id).await.unwrap().remove(0);
    item.id = "item-added-later".into();
    item.episode_num = 3.0;
    db::add_items_to_custom_playlist(pool, &[item]).await.unwrap();
    db::update_watch_progress(
        pool,
        &HistoryEntry {
            anilist_id: 1,
            title: "One".into(),
            episode_number: 2.0,
            provider: "auto".into(),
            category: "sub".into(),
            position_ms: 1_430_000,
            duration_ms: 1_440_000,
            updated_at: db::current_unix_millis() + 60_000,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    backup::restore(pool, &profile.settings_path(), &made, None).await.unwrap();

    assert_eq!(db::get_custom_playlist_items(pool, &playlist.id).await.unwrap().len(), 3);
    assert_eq!(db::get_episode_progress(pool, 1, 2.0).await.unwrap(), Some((1_430_000, 1_440_000)));
    assert_eq!(db::get_watched_episodes(pool, 1).await.unwrap(), vec![1.0, 2.0]);
}

#[tokio::test]
async fn backup_with_passphrase_seals_the_logins() {
    let source = filled().await;
    let made = backup::create(&source.pool, &source.settings_path(), json!([]), Some("correct horse")).await.unwrap();
    let text = String::from_utf8(backup::to_bytes(&made, false).unwrap()).unwrap();
    assert!(!text.contains("secret-anilist") && !text.contains("secret-refresh"));
    assert!(made.logins.is_some());
    let parsed = backup::from_bytes(text.as_bytes()).unwrap();

    // A wrong passphrase writes nothing.
    let target = Profile::new().await;
    let result = backup::restore(&target.pool, &target.settings_path(), &parsed, Some("wrong")).await;
    assert!(result.unwrap_err().to_string().contains("passphrase"));
    assert_eq!(count(&target.pool, "watchlist").await, 0);

    // No passphrase: everything but the logins.
    let summary = backup::restore(&target.pool, &target.settings_path(), &parsed, None).await.unwrap();
    assert!(summary.has_logins && !summary.logins_restored);
    assert!(target.settings().get("anilist_token").is_none());

    let summary = backup::restore(&target.pool, &target.settings_path(), &parsed, Some("correct horse")).await.unwrap();
    assert!(summary.logins_restored);
    let settings = target.settings();
    assert_eq!(settings["anilist_token"], "secret-anilist");
    assert_eq!(settings["mal_refresh_token"], "secret-refresh");
    assert_eq!(settings["anilist_viewer_name"], "viewer");
}

#[tokio::test]
async fn backup_refuses_newer_formats_and_other_files() {
    let source = filled().await;
    let made = backup::create(&source.pool, &source.settings_path(), json!([]), None).await.unwrap();
    let mut newer = serde_json::to_value(&made).unwrap();
    newer["version"] = json!(backup::VERSION + 1);
    let error = backup::from_bytes(newer.to_string().as_bytes()).unwrap_err().to_string();
    assert!(error.contains("newer version of Anix"), "{}", error);

    for not_backup in [&b"{\"format\":\"other\",\"version\":1}"[..], b"not json", b""] {
        let error = backup::from_bytes(not_backup).unwrap_err().to_string();
        assert!(error.contains("not an Anix backup"), "{}", error);
    }

    // Tokens written in plain settings by hand are never restored.
    let mut forged = made.clone();
    forged.settings["anilist_token"] = json!("forged");
    let target = Profile::new().await;
    backup::restore(&target.pool, &target.settings_path(), &forged, None).await.unwrap();
    assert!(target.settings().get("anilist_token").is_none());
}

#[test]
fn backup_pbkdf2_matches_the_rfc_7914_vector() {
    // PBKDF2-HMAC-SHA256, P = "passwd", S = "salt", c = 1 (first 32 bytes).
    let key = backup::pbkdf2(b"passwd", b"salt", 1);
    let hex: String = key.iter().map(|b| format!("{:02x}", b)).collect();
    assert_eq!(hex, "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc");
}
