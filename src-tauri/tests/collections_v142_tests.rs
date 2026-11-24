//! RED tests for the rest of issue #6 (collections), part 1: what the app keeps for when no
//! catalogue can be reached. The studio filter against the catalogues is in `catalogue_tests.rs`
//! (names start with `studio_`); the TypeScript rules are in `scripts/collections-test.mjs`.
//! The expected API is listed in `collections_v142_API.md`. Every test here fails until it is built.

use anix_lib::accounts::sync;
use anix_lib::catalogue::model::SearchQuery;
use anix_lib::clients::app_settings;
use anix_lib::commands::settings::{read_settings_file, write_settings_file};
use anix_lib::db;
use anix_lib::image_cache;
use anix_lib::offline::{self, Kind};
use serde_json::json;
use sqlx::Row;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("anix-v142-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ------------------------------------------------------------------ smart collection keeps its result

#[tokio::test]
async fn a_smart_collections_last_result_is_kept_for_when_every_catalogue_is_down() {
    // Arrange: a result saved under the collection's id, then more than 40 other searches
    // (Kind::Search keeps only 40) and a run of cache pruning.
    let pool = db::init_db_in_memory().await.unwrap();
    let result = json!([{ "id": 154587, "title": { "romaji": "Sousou no Frieren" } }]);
    offline::store_at(&pool, Kind::Collection, "pl-smart-1", &result, 1_000).await.unwrap();
    for i in 0..60 {
        offline::store_at(&pool, Kind::Search, &format!("q{}", i), &json!([i]), 2_000 + i).await.unwrap();
    }
    // Act: far past MAX_AGE_S; a collection's copy is its only copy and is never aged out.
    offline::prune(&pool, 1_000 + offline::MAX_AGE_S * 3).await.unwrap();
    // Assert
    let (value, saved_at) = offline::load(&pool, Kind::Collection, "pl-smart-1").await.unwrap().expect("kept");
    assert_eq!(value, result);
    assert_eq!(saved_at, 1_000);
}

#[tokio::test]
async fn a_collection_row_is_replaced_by_the_newer_result_of_the_same_collection() {
    let pool = db::init_db_in_memory().await.unwrap();
    offline::store_at(&pool, Kind::Collection, "pl-1", &json!([1]), 10).await.unwrap();
    offline::store_at(&pool, Kind::Collection, "pl-1", &json!([2]), 20).await.unwrap();
    let (value, at) = offline::load(&pool, Kind::Collection, "pl-1").await.unwrap().unwrap();
    assert_eq!((value, at), (json!([2]), 20));
}

#[tokio::test]
async fn a_franchise_collection_shows_the_covers_of_its_saved_franchise_in_its_collage() {
    // Arrange: a franchise collection, and the saved details of the anime it names (a stand-in
    // id, as from a fallback catalogue), in the AniList shape `fetch_anime_details` saves.
    let pool = db::init_db_in_memory().await.unwrap();
    let saved = db::create_custom_playlist(&pool, "Frieren", None, Some("franchise=-100000021")).await.unwrap();
    let never_opened = db::create_custom_playlist(&pool, "Unknown", None, Some("franchise=99")).await.unwrap();
    let node = |id: i64, format: &str, cover: &str| json!({ "id": id, "format": format, "coverImage": { "large": cover } });
    let details = json!({
        "id": -100000021,
        "coverImage": { "large": "https://img.example/root.jpg" },
        "relations": { "edges": [
            { "relationType": "ADAPTATION", "node": node(5, "MANGA", "https://img.example/manga.jpg") },
            { "relationType": "PREQUEL", "node": node(1, "TV", "https://img.example/prequel.jpg") },
            { "relationType": "CHARACTER", "node": node(7, "TV", "https://img.example/crossover.jpg") },
            { "relationType": "SEQUEL", "node": node(2, "MOVIE", "https://img.example/sequel.jpg") },
        ] }
    });
    offline::store_at(&pool, Kind::Details, "-100000021", &details, 1_000).await.unwrap();
    // Act
    let listed = db::list_custom_playlists(&pool).await.unwrap();
    // Assert: the anime and the entries of its watch order; no print entries or crossovers.
    let covers = |id: &str| listed.iter().find(|p| p.id == id).unwrap().covers.clone();
    assert_eq!(
        covers(&saved.id),
        ["https://img.example/root.jpg", "https://img.example/prequel.jpg", "https://img.example/sequel.jpg"]
    );
    assert!(covers(&never_opened.id).is_empty(), "nothing saved yet: the placeholder tile");
}

// ------------------------------------------------------------------ saved page note: source and age

#[tokio::test]
async fn a_saved_page_note_carries_its_source_and_age() {
    let pool = db::init_db_in_memory().await.unwrap();
    // Saved while MyAnimeList was serving (AniList down).
    offline::store_from_at(&pool, Kind::Search, "k", &json!([1]), Some("MyAnimeList"), 1_000).await.unwrap();
    let page = offline::load_saved(&pool, Kind::Search, "k", 1_000 + 3 * 3600).await.unwrap().expect("saved");
    assert_eq!(page.value, json!([1]));
    assert_eq!(page.source.as_deref(), Some("MyAnimeList"));
    assert_eq!(page.saved_at, 1_000);
    assert_eq!(page.age_s, 3 * 3600);
    // The clock going backwards never gives a negative age.
    let page = offline::load_saved(&pool, Kind::Search, "k", 500).await.unwrap().unwrap();
    assert_eq!(page.age_s, 0);
    // A row saved by 1.4.x code (no source) still loads, with no source named.
    offline::store_at(&pool, Kind::Search, "old", &json!([]), 1_000).await.unwrap();
    assert_eq!(offline::load_saved(&pool, Kind::Search, "old", 2_000).await.unwrap().unwrap().source, None);
    assert!(offline::load_saved(&pool, Kind::Search, "nothing", 2_000).await.unwrap().is_none());
}

#[test]
fn a_search_key_tells_studios_apart_and_keeps_old_keys_for_plain_searches() {
    let plain = SearchQuery { query: Some("Frieren".into()), genres: vec!["Drama".into()], page: 1, per_page: 24, hide_adult: true, ..Default::default() };
    // Rows saved by 1.4.x stay findable: a search without the newer filters has the old key.
    let old = offline::search_key(Some("Frieren"), &["Drama".to_string()], None, None, None, 1, 24, true);
    assert_eq!(offline::query_key(&plain), old);

    let mappa = SearchQuery { studio: Some("MAPPA".into()), ..plain.clone() };
    let mappa_again = SearchQuery { studio: Some("  mappa ".into()), ..plain.clone() };
    let bones = SearchQuery { studio: Some("Bones".into()), ..plain.clone() };
    assert_ne!(offline::query_key(&mappa), offline::query_key(&plain));
    assert_ne!(offline::query_key(&mappa), offline::query_key(&bones));
    assert_eq!(offline::query_key(&mappa), offline::query_key(&mappa_again), "case and spaces do not matter");
    // The older extra filters are in the key too.
    let scored = SearchQuery { min_score: Some(80), year: Some(2024), ..plain.clone() };
    assert_ne!(offline::query_key(&scored), offline::query_key(&plain));
}

// ------------------------------------------------------------------ library keeps title, cover, episodes

#[tokio::test]
async fn library_entries_keep_title_cover_and_episode_count_past_the_details_limit() {
    let pool = db::init_db_in_memory().await.unwrap();
    let total = Kind::Details.max_rows() + 50;
    for id in 1..=total {
        let id32 = id as i32;
        db::add_to_watchlist(&pool, id32, &format!("Show {}", id), Some(&format!("https://img.example/{}.jpg", id)), Some("TV"), Some(70))
            .await
            .unwrap();
        db::set_watchlist_episodes(&pool, id32, Some(12)).await.unwrap();
        offline::store_at(&pool, Kind::Details, &id.to_string(), &json!({ "id": id }), 10_000 + id).await.unwrap();
    }
    // The details cache is capped; the oldest details are gone.
    assert_eq!(offline::load_all(&pool, Kind::Details).await.unwrap().len() as i64, Kind::Details.max_rows());
    assert!(offline::load(&pool, Kind::Details, "1").await.unwrap().is_none());

    // The library still has everything it needs to draw the entry.
    let list = db::get_watchlist(&pool).await.unwrap();
    assert_eq!(list.len() as i64, total);
    let first = list.iter().find(|e| e.anilist_id == 1).unwrap();
    assert_eq!(first.title, "Show 1");
    assert_eq!(first.cover.as_deref(), Some("https://img.example/1.jpg"));
    assert_eq!(first.episodes, Some(12));
}

#[tokio::test]
async fn an_unknown_episode_count_stays_unknown_and_a_later_count_replaces_it() {
    let pool = db::init_db_in_memory().await.unwrap();
    db::add_to_watchlist(&pool, 7, "Long runner", None, None, None).await.unwrap();
    assert_eq!(db::get_watchlist(&pool).await.unwrap()[0].episodes, None);
    db::set_watchlist_episodes(&pool, 7, Some(26)).await.unwrap();
    db::set_watchlist_episodes(&pool, 7, None).await.unwrap(); // a source that does not know must not erase it
    assert_eq!(db::get_watchlist(&pool).await.unwrap()[0].episodes, Some(26));
    // An anime that is not in the library is not invented by this call.
    db::set_watchlist_episodes(&pool, 8, Some(12)).await.unwrap();
    assert_eq!(db::get_watchlist(&pool).await.unwrap().len(), 1);
}

// ------------------------------------------------------------------ every list change is queued

#[tokio::test]
async fn every_list_change_made_while_offline_is_counted_as_waiting_to_sync() {
    let pool = db::init_db_in_memory().await.unwrap();
    assert_eq!(sync::waiting_changes(&pool).await.unwrap(), 0);
    // A status chosen here (stored with synced = 0) ...
    sync::set_status(&pool, 1, "One", None, None, None, "COMPLETED").await.unwrap();
    // ... a watched episode (queued for each service) ...
    sync::queue_watched_episode(&pool, 2, 3.0).await.unwrap();
    // ... and a second change to anime 1 counts once, per anime.
    sync::queue_watched_episode(&pool, 1, 5.0).await.unwrap();
    assert_eq!(sync::waiting_changes(&pool).await.unwrap(), 2);
    // A stand-in id that was merged into its real anime is not a change of its own.
    sync::set_status(&pool, -100000007, "Seven", None, None, None, "PLANNING").await.unwrap();
    assert_eq!(sync::waiting_changes(&pool).await.unwrap(), 3);
    sqlx::query("INSERT INTO catalogue_merged_ids (stand_in, anilist_id, merged_at) VALUES (-100000007, 7, 1)")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(sync::waiting_changes(&pool).await.unwrap(), 2);
}

// ------------------------------------------------------------------ covers and banners on disk

const MIB: u64 = 1024 * 1024;

#[test]
fn the_cover_limit_is_200_mb_by_default_and_follows_the_setting() {
    assert_eq!(image_cache::DEFAULT_LIMIT_MB, 200);
    assert_eq!(image_cache::SETTING_KEY, "image_cache_mb");
    assert_eq!(image_cache::limit_bytes(&json!({})), 200 * MIB);
    assert_eq!(image_cache::limit_bytes(&json!({ "image_cache_mb": 50 })), 50 * MIB);
    // A wrong type keeps the default instead of switching the cache off.
    assert_eq!(image_cache::limit_bytes(&json!({ "image_cache_mb": "lots" })), 200 * MIB);
    // Out of range values are held to the allowed range.
    assert_eq!(image_cache::limit_bytes(&json!({ "image_cache_mb": 0 })), image_cache::MIN_LIMIT_MB * MIB);
    assert_eq!(image_cache::limit_bytes(&json!({ "image_cache_mb": 9_999_999 })), image_cache::MAX_LIMIT_MB * MIB);
    const { assert!(image_cache::MIN_LIMIT_MB < 200 && 200 < image_cache::MAX_LIMIT_MB) };
}

#[test]
fn the_cover_limit_round_trips_through_the_settings_file() {
    let dir = scratch("settings");
    let path = dir.join("settings.json");
    write_settings_file(&path, &json!({ "image_cache_mb": 75, "hide_adult_content": true })).unwrap();
    let read = read_settings_file(&path).unwrap();
    assert_eq!(read["image_cache_mb"], json!(75));
    assert_eq!(image_cache::limit_bytes(&read), 75 * MIB);
    // Other settings are not disturbed by the new key.
    assert!(app_settings::NetworkSettings::from_json(&read).hide_adult_content);
    let _ = fs::remove_dir_all(dir);
}

fn put(dir: &Path, name: &str, bytes: usize, age_s: u64) {
    let path = dir.join(name);
    fs::write(&path, vec![7u8; bytes]).unwrap();
    let when = SystemTime::now() - Duration::from_secs(age_s);
    fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
}

#[test]
fn pruning_drops_the_oldest_images_first_until_the_folder_fits_the_limit() {
    let dir = scratch("prune");
    put(&dir, "oldest.img", 400, 5000);
    put(&dir, "old.img", 400, 4000);
    put(&dir, "newer.img", 400, 3000);
    put(&dir, "newest.img", 400, 1000);
    assert_eq!(image_cache::used_bytes(&dir), 1600);

    let report = image_cache::prune(&dir, 1000).unwrap();
    assert_eq!((report.removed, report.freed, report.kept_bytes), (2, 800, 800));
    assert!(!dir.join("oldest.img").exists() && !dir.join("old.img").exists());
    assert!(dir.join("newer.img").exists() && dir.join("newest.img").exists());
    assert_eq!(image_cache::used_bytes(&dir), 800);

    // Already inside the limit: nothing is touched. A missing folder is empty, not an error.
    assert_eq!(image_cache::prune(&dir, 1000).unwrap().removed, 0);
    assert_eq!(image_cache::prune(&dir.join("nope"), 1000).unwrap().removed, 0);
    // A limit of zero empties it.
    assert_eq!(image_cache::prune(&dir, 0).unwrap().kept_bytes, 0);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn saving_an_image_stays_within_the_limit_and_keeps_the_new_one() {
    let dir = scratch("save");
    let a = image_cache::save(&dir, "https://img.example/a.jpg", &[1u8; 600], 1000).unwrap();
    assert_eq!(image_cache::find(&dir, "https://img.example/a.jpg"), Some(a.clone()));
    // Same address, same file; another address, another file; never a path outside the folder.
    assert_eq!(image_cache::path_for(&dir, "https://img.example/a.jpg"), a);
    let evil = image_cache::path_for(&dir, "https://x.example/../../etc/passwd?a=/b");
    assert_ne!(evil, a);
    assert_eq!(evil.parent(), Some(dir.as_path()));
    // Saving another pushes the folder over the limit: the older goes, the new one stays.
    std::thread::sleep(Duration::from_millis(20));
    let b = image_cache::save(&dir, "https://img.example/b.jpg", &[2u8; 600], 1000).unwrap();
    assert!(b.is_file() && !a.exists());
    assert_eq!(image_cache::find(&dir, "https://img.example/a.jpg"), None);
    assert!(image_cache::used_bytes(&dir) <= 1000);
    assert_eq!(fs::read(&b).unwrap(), vec![2u8; 600]);
    let _ = fs::remove_dir_all(dir);
}

// ------------------------------------------------------------------ additive migration on a real 1.4.x database

/// The read-only copy of a real 1.4.x database (git-ignored, on the owner's machine only).
fn real_db_original() -> Option<PathBuf> {
    let mut dir = Some(Path::new(env!("CARGO_MANIFEST_DIR")));
    while let Some(d) = dir {
        let candidate = d.join("testing/anix-v15/db-1.4.x/anix.db");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

async fn row_counts(path: &Path) -> Vec<(String, i64)> {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    let pool = SqlitePoolOptions::new().connect_with(SqliteConnectOptions::new().filename(path)).await.unwrap();
    let tables: Vec<String> = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get("name"))
        .collect();
    let mut counts = Vec::new();
    for table in tables {
        let n: i64 = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) AS n FROM \"{}\"", table))).fetch_one(&pool).await.unwrap().get("n");
        counts.push((table, n));
    }
    pool.close().await;
    counts
}

#[tokio::test]
async fn the_new_migrations_only_add_to_a_real_1_4_database() {
    let Some(original) = real_db_original() else {
        eprintln!("skipped: no testing/anix-v15/db-1.4.x/anix.db on this machine");
        return;
    };
    // Arrange: a copy (database, journal and shared memory), never the original.
    let dir = scratch("real-db");
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{}", original.display(), suffix));
        if from.is_file() {
            fs::copy(&from, dir.join(format!("anix.db{}", suffix))).unwrap();
        }
    }
    let copy = dir.join("anix.db");
    let before = row_counts(&copy).await;
    assert!(before.iter().any(|(t, n)| t == "watchlist" || *n > 0), "the copy has data: {:?}", before);

    // Act: open with the new migrations, twice (they must be repeatable).
    for _ in 0..2 {
        let pool = db::init_db_at(&copy).await.unwrap();
        // Assert: the new columns exist ...
        let has = |table: &'static str, column: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({})", table)))
                    .fetch_all(&pool)
                    .await
                    .unwrap()
                    .iter()
                    .any(|r| r.get::<String, _>("name") == column)
            }
        };
        assert!(has("watchlist", "episodes").await, "watchlist.episodes");
        assert!(has("anilist_cache", "source").await, "anilist_cache.source");
        pool.close().await;
    }
    // ... and no row of any old table was lost.
    let after = row_counts(&copy).await;
    for (table, n) in &before {
        let now = after.iter().find(|(t, _)| t == table).map(|(_, n)| *n);
        assert!(now.is_some_and(|now| now >= *n), "{} lost rows: {} -> {:?}", table, n, now);
    }
    let _ = fs::remove_dir_all(dir);
}

// ------------------------------------------------------------------ pictures are fetched safely

fn serve_once(content_type: &'static str, body: Vec<u8>) -> String {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let header = tiny_http::Header::from_bytes("Content-Type", content_type).unwrap();
            let _ = request.respond(tiny_http::Response::from_data(body.clone()).with_header(header));
        }
    });
    format!("http://127.0.0.1:{port}/cover.jpg")
}

#[tokio::test]
async fn a_private_or_loopback_picture_address_is_refused_before_any_request() {
    let url = serve_once("image/jpeg", vec![1, 2, 3]);
    assert!(image_cache::fetch_public(&url, 1024).await.is_err());
    assert!(image_cache::fetch_public("http://192.168.1.10/a.jpg", 1024).await.is_err());
}

#[tokio::test]
async fn an_oversized_or_non_picture_answer_is_not_kept() {
    let client = reqwest::Client::new();
    let ok = serve_once("image/jpeg", vec![7; 100]);
    assert_eq!(image_cache::fetch_bytes(&client, &ok, 1024).await.unwrap().map(|b| b.len()), Some(100));
    assert_eq!(image_cache::fetch_bytes(&client, &ok, 50).await.unwrap(), None, "over the limit");
    let html = serve_once("text/html", vec![7; 10]);
    assert_eq!(image_cache::fetch_bytes(&client, &html, 1024).await.unwrap(), None, "not a picture");
}
