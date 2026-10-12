//! The move of an install made under the app's earlier name (Anilili) to the current one.
//!
//! Every test builds its own home under the system temp directory; none looks at the real one.

use anix_lib::db;
use anix_lib::migration::{
    copy_then_remove, migrate, repoint_download_paths, BaseDirs, Outcome, IDENTIFIER, LEGACY_IDENTIFIER,
};
use anix_lib::models::DownloadRecord;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A made-up home directory with the usual layout, removed again when dropped.
struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-migration-test-{}-{}-{}",
            name,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        for sub in [".local/share", ".config", ".cache"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        Self(dir)
    }

    fn data(&self) -> PathBuf {
        self.0.join(".local/share")
    }
    fn config(&self) -> PathBuf {
        self.0.join(".config")
    }
    fn cache(&self) -> PathBuf {
        self.0.join(".cache")
    }

    fn bases(&self) -> BaseDirs {
        BaseDirs {
            data: Some(self.data()),
            config: Some(self.config()),
            cache: Some(self.cache()),
            local_data: Some(self.data()),
            home: Some(self.0.clone()),
        }
    }

    /// Everything an Anilili install leaves behind.
    fn with_old_install(self) -> Self {
        let data = self.data().join("anilili");
        write(&data.join("anilili.db"), "database");
        write(&data.join("anilili.db-wal"), "write-ahead log");
        write(&data.join("anilili.db-shm"), "shared memory");
        write(&data.join("anilili.log"), "log");
        write(&data.join("downloads/Show/Show - EP 01.mp4"), "video");
        write(&data.join("plugins/state.json"), "{}");
        write(&data.join("plugins/payloads/site.js"), "class Provider {}");
        write(&self.config().join("anilili/settings.json"), r#"{"autoplay":true}"#);
        write(&self.cache().join("anilili/notification-covers/1.jpg"), "cover");
        let webview = self.data().join(LEGACY_IDENTIFIER);
        write(&webview.join("site-sessions/cookies"), "site cookies");
        write(&webview.join("site-sessions.json"), "[]");
        write(&webview.join("localstorage/tauri_localhost_0.localstorage"), "player settings");
        std::os::unix::fs::symlink("site-sessions.json", webview.join("link")).unwrap();
        self
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e))
}

#[test]
fn the_identifier_is_the_one_tauri_is_configured_with() {
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert_eq!(config["identifier"], IDENTIFIER);
    assert_eq!(config["productName"], "Anix");
    assert_eq!(config["version"], env!("CARGO_PKG_VERSION"));
    let schemes = config["plugins"]["deep-link"]["desktop"]["schemes"].as_array().unwrap();
    assert!(schemes.contains(&"anix".into()) && schemes.contains(&"anilili".into()));
}

#[test]
fn a_fresh_install_has_nothing_to_move_and_gets_nothing_created() {
    let home = Home::new("fresh");
    let report = migrate(&home.bases());
    assert!(report.is_empty(), "{:?}", report);
    assert!(fs::read_dir(home.data()).unwrap().next().is_none());
    assert!(fs::read_dir(home.config()).unwrap().next().is_none());
    assert!(fs::read_dir(home.cache()).unwrap().next().is_none());

    // Directories that are unknown are simply not looked at.
    assert!(migrate(&BaseDirs::default()).is_empty());
}

#[test]
fn an_earlier_install_is_moved_to_the_current_names() {
    let home = Home::new("old-only").with_old_install();
    let report = migrate(&home.bases());
    assert!(report.failures().is_empty(), "{:?}", report);
    assert_eq!(report.outcome_for(&home.data().join("anilili")), Some(&Outcome::Moved));
    assert_eq!(report.outcome_for(&home.config().join("anilili")), Some(&Outcome::Moved));
    assert_eq!(report.outcome_for(&home.cache().join("anilili")), Some(&Outcome::Moved));
    assert_eq!(report.outcome_for(&home.data().join(LEGACY_IDENTIFIER)), Some(&Outcome::Moved));

    let data = home.data().join("anix");
    assert_eq!(read(&data.join("anix.db")), "database");
    assert_eq!(read(&data.join("anix.db-wal")), "write-ahead log");
    assert_eq!(read(&data.join("anix.db-shm")), "shared memory");
    assert_eq!(read(&data.join("anix.log")), "log");
    assert!(!data.join("anilili.db").exists() && !data.join("anilili.db-wal").exists() && !data.join("anilili.log").exists());
    assert_eq!(read(&data.join("downloads/Show/Show - EP 01.mp4")), "video");
    assert_eq!(read(&data.join("plugins/payloads/site.js")), "class Provider {}");
    assert_eq!(read(&home.config().join("anix/settings.json")), r#"{"autoplay":true}"#);
    assert_eq!(read(&home.cache().join("anix/notification-covers/1.jpg")), "cover");
    let webview = home.data().join(IDENTIFIER);
    assert_eq!(read(&webview.join("site-sessions/cookies")), "site cookies");
    assert_eq!(read(&webview.join("site-sessions.json")), "[]");
    assert_eq!(read(&webview.join("localstorage/tauri_localhost_0.localstorage")), "player settings");

    for gone in [home.data().join("anilili"), home.config().join("anilili"), home.cache().join("anilili"), home.data().join(LEGACY_IDENTIFIER)] {
        assert!(!gone.exists(), "{} is still there", gone.display());
    }
    assert_eq!(db::database_path(&data), data.join("anix.db"));

    // The next start finds nothing left to do.
    assert!(migrate(&home.bases()).is_empty());
}

#[test]
fn existing_current_data_is_never_replaced() {
    let home = Home::new("both").with_old_install();
    write(&home.data().join("anix/anix.db"), "new database");
    write(&home.config().join("anix/settings.json"), "new settings");
    write(&home.data().join(IDENTIFIER).join("site-sessions.json"), "new sessions");

    let report = migrate(&home.bases());
    assert!(report.failures().is_empty(), "{:?}", report);
    assert_eq!(report.outcome_for(&home.data().join("anilili")), Some(&Outcome::KeptExisting));
    assert_eq!(report.outcome_for(&home.config().join("anilili")), Some(&Outcome::KeptExisting));
    assert_eq!(report.outcome_for(&home.data().join(LEGACY_IDENTIFIER)), Some(&Outcome::KeptExisting));
    // The cache folder had no current counterpart, so that one does move.
    assert_eq!(report.outcome_for(&home.cache().join("anilili")), Some(&Outcome::Moved));

    assert_eq!(read(&home.data().join("anix/anix.db")), "new database");
    assert_eq!(fs::read_dir(home.data().join("anix")).unwrap().count(), 1, "nothing was merged in");
    assert_eq!(read(&home.config().join("anix/settings.json")), "new settings");
    assert_eq!(read(&home.data().join(IDENTIFIER).join("site-sessions.json")), "new sessions");
    // The earlier install is left exactly as it was.
    assert_eq!(read(&home.data().join("anilili/anilili.db")), "database");
    assert_eq!(read(&home.config().join("anilili/settings.json")), r#"{"autoplay":true}"#);
    assert_eq!(read(&home.data().join(LEGACY_IDENTIFIER).join("site-sessions/cookies")), "site cookies");
}

#[test]
fn an_empty_current_folder_does_not_block_the_move() {
    let home = Home::new("empty-new").with_old_install();
    fs::create_dir_all(home.data().join("anix")).unwrap();
    fs::create_dir_all(home.data().join(IDENTIFIER)).unwrap();
    let report = migrate(&home.bases());
    assert!(report.failures().is_empty(), "{:?}", report);
    assert_eq!(read(&home.data().join("anix/anix.db")), "database");
    assert_eq!(read(&home.data().join(IDENTIFIER).join("site-sessions.json")), "[]");
}

#[test]
fn partial_earlier_data_is_moved_piece_by_piece() {
    // Only settings: the app was started once and closed before it had a database.
    let home = Home::new("partial-config");
    write(&home.config().join("anilili/settings.json"), "{}");
    let report = migrate(&home.bases());
    assert_eq!(report.steps.len(), 1, "{:?}", report);
    assert_eq!(read(&home.config().join("anix/settings.json")), "{}");
    assert!(!home.data().join("anix").exists() && !home.data().join(IDENTIFIER).exists());

    // A data folder with downloads and a log but no database, and a database without companions.
    let home = Home::new("partial-data");
    write(&home.data().join("anilili/downloads/a.mp4"), "video");
    write(&home.data().join("anilili/anilili.log"), "log");
    let report = migrate(&home.bases());
    assert!(report.failures().is_empty(), "{:?}", report);
    assert_eq!(read(&home.data().join("anix/downloads/a.mp4")), "video");
    assert_eq!(read(&home.data().join("anix/anix.log")), "log");
    assert!(!home.data().join("anix/anix.db").exists());

    let home = Home::new("partial-db");
    write(&home.data().join("anilili/anilili.db"), "database");
    assert!(migrate(&home.bases()).failures().is_empty());
    assert_eq!(read(&home.data().join("anix/anix.db")), "database");
    assert!(!home.data().join("anix/anix.db-wal").exists());

    // A move that stopped half way (folder moved, database not yet renamed) is finished.
    let home = Home::new("partial-rename");
    write(&home.data().join("anix/anilili.db"), "database");
    write(&home.data().join("anix/anilili.db-wal"), "wal");
    assert_eq!(db::database_path(&home.data().join("anix")), home.data().join("anix/anilili.db"));
    assert!(migrate(&home.bases()).failures().is_empty());
    assert_eq!(read(&home.data().join("anix/anix.db")), "database");
    assert_eq!(read(&home.data().join("anix/anix.db-wal")), "wal");

    // A file that happens to carry the earlier folder name is not a folder to move.
    let home = Home::new("partial-file");
    write(&home.data().join("anilili"), "not a directory");
    assert!(migrate(&home.bases()).is_empty());
    assert!(home.data().join("anilili").is_file());
}

#[test]
fn a_data_folder_outside_the_xdg_directory_is_moved_where_it_is() {
    // XDG_DATA_HOME points somewhere else, but the install kept using ~/.local/share.
    let home = Home::new("legacy-home").with_old_install();
    let elsewhere = home.0.join("xdg-data");
    fs::create_dir_all(&elsewhere).unwrap();
    let bases = BaseDirs { data: Some(elsewhere.clone()), ..home.bases() };
    let report = migrate(&bases);
    assert!(report.failures().is_empty(), "{:?}", report);
    assert_eq!(read(&home.data().join("anix/anix.db")), "database");
    assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
}

#[test]
fn a_target_that_cannot_be_written_leaves_everything_in_place() {
    if unsafe { libc::geteuid() } == 0 {
        return; // Permissions do not stop root.
    }
    let home = Home::new("unwritable").with_old_install();
    let locked = fs::Permissions::from_mode(0o555);
    fs::set_permissions(home.data(), locked.clone()).unwrap();
    fs::set_permissions(home.config(), locked).unwrap();

    let report = migrate(&home.bases());

    let open = fs::Permissions::from_mode(0o755);
    fs::set_permissions(home.data(), open.clone()).unwrap();
    fs::set_permissions(home.config(), open).unwrap();

    for stuck in [home.data().join("anilili"), home.config().join("anilili"), home.data().join(LEGACY_IDENTIFIER)] {
        assert!(matches!(report.outcome_for(&stuck), Some(Outcome::Failed(_))), "{:?}", report);
    }
    assert_eq!(report.failures().len(), 3);
    // What could be moved was; the rest is untouched and nothing half-made is left behind.
    assert_eq!(report.outcome_for(&home.cache().join("anilili")), Some(&Outcome::Moved));
    assert_eq!(read(&home.data().join("anilili/anilili.db")), "database");
    assert_eq!(read(&home.data().join("anilili/anilili.db-wal")), "write-ahead log");
    assert_eq!(read(&home.config().join("anilili/settings.json")), r#"{"autoplay":true}"#);
    let names: Vec<String> = fs::read_dir(home.data()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 2, "{:?}", names);
    report.log();
    // The app keeps working on the folders it could not move instead of starting empty.
    let data_dir = db::choose_app_dir(Some(home.data()), Some(home.data()), &[db::DB_FILE_NAME, db::LEGACY_DB_FILE_NAME]);
    assert_eq!(data_dir, Some(home.data().join("anilili")));
    assert_eq!(db::database_path(&data_dir.unwrap()), home.data().join("anilili/anilili.db"));
    let config_dir = db::choose_app_dir(Some(home.config()), None, &["settings.json"]);
    assert_eq!(config_dir, Some(home.config().join("anilili")));

    // Once the directory is writable the next start completes the move.
    let report = migrate(&home.bases());
    assert!(report.failures().is_empty(), "{:?}", report);
    assert_eq!(read(&home.data().join("anix/anix.db")), "database");
    let data_dir = db::choose_app_dir(Some(home.data()), Some(home.data()), &[db::DB_FILE_NAME, db::LEGACY_DB_FILE_NAME]);
    assert_eq!(data_dir, Some(home.data().join("anix")));
}

#[test]
fn the_app_folder_is_the_one_that_holds_the_data() {
    let home = Home::new("choose");
    let elsewhere = home.0.join("xdg-data");
    let markers = [db::DB_FILE_NAME, db::LEGACY_DB_FILE_NAME];
    let choose = || db::choose_app_dir(Some(elsewhere.clone()), Some(home.data()), &markers);
    // Nothing anywhere: the current name in the proper directory.
    assert_eq!(choose(), Some(elsewhere.join("anix")));
    assert_eq!(db::choose_app_dir(None, Some(home.data()), &markers), Some(home.data().join("anix")));
    assert_eq!(db::choose_app_dir(None, None, &markers), None);
    // An install that lives in ~/.local/share is kept there.
    write(&home.data().join("anilili/anilili.db"), "old");
    assert_eq!(choose(), Some(home.data().join("anilili")));
    write(&home.data().join("anix/anix.db"), "new");
    assert_eq!(choose(), Some(home.data().join("anix")));
    // The proper directory wins once it holds a database.
    write(&elsewhere.join("anix/anix.db"), "newest");
    assert_eq!(choose(), Some(elsewhere.join("anix")));
}

#[test]
fn a_database_that_cannot_be_renamed_keeps_its_companions_and_is_still_opened() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let home = Home::new("locked-db");
    let dir = home.data().join("anix");
    write(&dir.join("anilili.db"), "database");
    write(&dir.join("anilili.db-wal"), "wal");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let report = migrate(&home.bases());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(report.failures().len(), 1, "{:?}", report);
    assert_eq!(read(&dir.join("anilili.db")), "database");
    assert_eq!(read(&dir.join("anilili.db-wal")), "wal");
    assert!(!dir.join("anix.db").exists() && !dir.join("anix.db-wal").exists());
    assert_eq!(db::database_path(&dir), dir.join("anilili.db"));
}

#[test]
fn copying_stands_in_when_renaming_is_not_possible() {
    let home = Home::new("copy").with_old_install();
    let (from, to) = (home.data().join(LEGACY_IDENTIFIER), home.data().join(IDENTIFIER));
    assert!(copy_then_remove(&from, &to).unwrap(), "the original is removed after the copy");
    assert!(!from.exists());
    assert_eq!(read(&to.join("site-sessions/cookies")), "site cookies");
    assert_eq!(fs::read_link(to.join("link")).unwrap(), PathBuf::from("site-sessions.json"));
    let names: Vec<String> = fs::read_dir(home.data()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|name| !name.contains("migrating")), "{:?}", names);

    // It refuses a target that holds something.
    let from = home.data().join("anilili");
    let err = copy_then_remove(&from, &to).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(read(&from.join("anilili.db")), "database");
}

#[tokio::test]
async fn a_real_database_keeps_its_history_and_downloads_across_the_move() {
    let home = Home::new("real-db");
    let old_dir = home.data().join("anilili");
    fs::create_dir_all(&old_dir).unwrap();
    let old_file = old_dir.join("downloads/Show/Show - EP 01.mp4");
    let outside = home.0.join("Videos/Show - EP 02.mp4");
    write(&old_file, "video");
    write(&outside, "video");
    {
        // The earlier app: a database under the earlier name, closed without a checkpoint.
        let pool = db::init_db_at(&old_dir.join("anilili.db")).await.unwrap();
        for (id, path) in [("in-app", &old_file), ("elsewhere", &outside)] {
            let record = DownloadRecord {
                id: id.to_string(),
                series_title: "Show".to_string(),
                provider: "p".to_string(),
                category: "sub".to_string(),
                quality: "720p".to_string(),
                status: db::STATUS_COMPLETED.to_string(),
                file_path: Some(path.to_string_lossy().into_owned()),
                ..DownloadRecord::default()
            };
            db::create_download(&pool, &record).await.unwrap();
        }
        pool.close().await;
    }

    let bases = home.bases();
    let report = migrate(&bases);
    assert!(report.failures().is_empty(), "{:?}", report);
    let new_dir = home.data().join("anix");
    let pool = db::init_db_at(&db::database_path(&new_dir)).await.unwrap();

    assert_eq!(repoint_download_paths(&pool, &bases).await.unwrap(), 1);
    let rows = db::get_all_downloads(&pool).await.unwrap();
    let path_of = |id: &str| rows.iter().find(|row| row.id == id).unwrap().file_path.clone().unwrap();
    let moved = new_dir.join("downloads/Show/Show - EP 01.mp4");
    assert_eq!(path_of("in-app"), moved.to_string_lossy());
    assert!(moved.is_file());
    assert_eq!(path_of("elsewhere"), outside.to_string_lossy(), "files outside the data folder are not touched");
    // Doing it again changes nothing.
    assert_eq!(repoint_download_paths(&pool, &bases).await.unwrap(), 0);
    pool.close().await;
}
