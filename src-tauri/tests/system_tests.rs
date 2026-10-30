//! Tests of the offline AniList cache, the autostart entry, the background release checks' units
//! and lock hand-off, the hardware report's parsing and
//! tips, the tray's "Airing next" list, and the plugin health and directory helpers.

use anix_lib::db;
use anix_lib::error::AppError;
use anix_lib::models::AiringEntry;
use anix_lib::notifier::{Airing, Bell};
use anix_lib::offline::{self, Kind};
use anix_lib::plugins::health;
use anix_lib::system::{autostart, background, hwaccel, tray, SystemSettings};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-system-test-{}-{}-{}",
            name,
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ------------------------------------------------------------------ offline cache

#[tokio::test]
async fn cache_round_trips_and_replaces() {
    let pool = db::init_db_in_memory().await.unwrap();
    assert_eq!(offline::load(&pool, Kind::Details, "21").await.unwrap(), None);

    offline::store_at(&pool, Kind::Details, "21", &json!({ "id": 21, "title": "old" }), 1_000).await.unwrap();
    offline::store_at(&pool, Kind::Details, "21", &json!({ "id": 21, "title": "new" }), 2_000).await.unwrap();
    let (value, at) = offline::load(&pool, Kind::Details, "21").await.unwrap().unwrap();
    assert_eq!(value["title"], "new");
    assert_eq!(at, 2_000);

    // Kinds do not share keys.
    assert_eq!(offline::load(&pool, Kind::Search, "21").await.unwrap(), None);
}

#[tokio::test]
async fn cache_keeps_only_the_newest_rows_of_a_kind() {
    let pool = db::init_db_in_memory().await.unwrap();
    let limit = Kind::Search.max_rows();
    for i in 0..(limit + 5) {
        offline::store_at(&pool, Kind::Search, &format!("q{}", i), &json!([i]), 10_000 + i).await.unwrap();
    }
    // A different kind is not pruned by search writes.
    offline::store_at(&pool, Kind::Details, "1", &json!({}), 10_000).await.unwrap();

    let rows = offline::load_all(&pool, Kind::Search).await.unwrap();
    assert_eq!(rows.len() as i64, limit);
    assert_eq!(rows[0].0, format!("q{}", limit + 4), "newest first");
    assert!(offline::load(&pool, Kind::Search, "q0").await.unwrap().is_none(), "oldest dropped");
    assert!(offline::load(&pool, Kind::Details, "1").await.unwrap().is_some());
}

#[tokio::test]
async fn prune_drops_rows_past_the_age_limit() {
    let pool = db::init_db_in_memory().await.unwrap();
    let now = 100 * 24 * 3600;
    offline::store_at(&pool, Kind::Details, "old", &json!({}), now - offline::MAX_AGE_S - 1).await.unwrap();
    offline::store_at(&pool, Kind::Details, "fresh", &json!({}), now - 60).await.unwrap();
    offline::prune(&pool, now).await.unwrap();
    assert!(offline::load(&pool, Kind::Details, "old").await.unwrap().is_none());
    assert!(offline::load(&pool, Kind::Details, "fresh").await.unwrap().is_some());
}

#[tokio::test]
async fn migrations_create_the_cache_table_on_an_existing_database() {
    let dir = TempDir::new("migrate");
    let path = dir.0.join("anix.db");
    let pool = db::init_db_at(&path).await.unwrap();
    offline::store(&pool, Kind::Home, "home", &json!({ "trending": [] })).await.unwrap();
    pool.close().await;
    // Opening again runs the migrations a second time and keeps the row.
    let pool = db::init_db_at(&path).await.unwrap();
    assert!(offline::load(&pool, Kind::Home, "home").await.unwrap().is_some());
}

#[test]
fn only_connection_failures_serve_the_cache() {
    assert!(offline::is_offline_error(&AppError::Unreachable("graphql.anilist.co: dns".into())));
    assert!(offline::is_offline_error(&AppError::Network("graphql.anilist.co timed out".into())));
    assert!(offline::is_offline_error(&AppError::RateLimited { retry_after_s: 60 }));
    assert!(!offline::is_offline_error(&AppError::Parse("Anime with ID 1 not found".into())));
    assert!(!offline::is_offline_error(&AppError::AuthRequired));
}

#[test]
fn search_keys_ignore_case_spacing_and_genre_order() {
    let a = offline::search_key(Some(" Frieren "), &["Drama".into(), "Action".into()], None, Some(""), None, 1, 20, true);
    let b = offline::search_key(Some("frieren"), &["action".into(), "drama".into()], None, None, None, 1, 20, true);
    assert_eq!(a, b);
    let other_page = offline::search_key(Some("frieren"), &[], None, None, None, 2, 20, true);
    let adult = offline::search_key(Some("frieren"), &[], None, None, None, 1, 20, false);
    let plain = offline::search_key(Some("frieren"), &[], None, None, None, 1, 20, true);
    assert_ne!(plain, other_page);
    assert_ne!(plain, adult);
}

fn airing(id: i64, at: i64) -> AiringEntry {
    AiringEntry {
        id,
        airing_at: at,
        episode: 1,
        media_id: id * 10,
        media_title: format!("Anime {}", id),
        ..AiringEntry::default()
    }
}

#[test]
fn saved_schedules_are_merged_for_the_requested_window() {
    let saved = vec![
        ("200-300".to_string(), json!([airing(3, 250), airing(4, 290)]), 5_000),
        ("100-200".to_string(), json!([airing(1, 120), airing(2, 180), airing(3, 250)]), 4_000),
        ("0-50".to_string(), json!([airing(9, 10)]), 1_000),
    ];
    let (entries, saved_at) = offline::saved_schedule(saved, 150, 260);
    let ids: Vec<i64> = entries.iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![2, 3]);
    assert_eq!(saved_at, Some(4_000), "the oldest schedule that contributed");

    let (none, at) = offline::saved_schedule(vec![("0-50".into(), json!([airing(9, 10)]), 1)], 100, 200);
    assert!(none.is_empty());
    assert_eq!(at, None);
}

#[test]
fn saved_schedule_prefers_anilist_over_an_estimate() {
    // A newer estimated schedule (negative ids) overlaps AniList's: each episode once, AniList's.
    let estimate = AiringEntry { id: -300_001, airing_at: 255, ..airing(3, 255) };
    let saved = vec![
        ("200-300".to_string(), json!([estimate]), 5_000),
        ("100-260".to_string(), json!([airing(3, 250)]), 4_000),
    ];
    let (entries, _) = offline::saved_schedule(saved, 150, 260);
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].id, entries[0].airing_at), (3, 250));
}

// ------------------------------------------------------------------ autostart

#[test]
fn autostart_entry_names_the_program_and_starts_minimised() {
    let entry = autostart::desktop_entry(Path::new("/opt/Anix/anix"));
    assert!(entry.starts_with("[Desktop Entry]\n"));
    assert!(entry.contains("\nType=Application\n"));
    assert!(entry.contains("\nName=Anix\n"));
    assert!(entry.contains("\nExec=/opt/Anix/anix --minimized\n"));
    assert!(entry.contains("\nX-GNOME-Autostart-enabled=true\n"));
    assert!(entry.contains("\nX-Anix-Autostart=true\n"));
}

#[test]
fn exec_arguments_are_quoted_as_the_desktop_entry_spec_asks() {
    assert_eq!(autostart::quote_exec_arg("/usr/bin/anix"), "/usr/bin/anix");
    assert_eq!(
        autostart::quote_exec_arg("/run/media/me/Extra Volume/Anix.AppImage"),
        "\"/run/media/me/Extra Volume/Anix.AppImage\""
    );
    // `$` and `"` are escaped inside the quotes, then every backslash is doubled.
    assert_eq!(autostart::quote_exec_arg("/a/$b\"c"), "\"/a/\\\\$b\\\\\"c\"");
    assert_eq!(autostart::quote_exec_arg("/a/100%"), "/a/100%%");
}

#[test]
fn the_appimage_is_started_rather_than_its_mounted_binary() {
    let exe = Some(PathBuf::from("/tmp/.mount_AnixAB/usr/bin/anix"));
    assert_eq!(
        autostart::exec_target(Some("/home/u/Apps/Anix.AppImage".into()), exe.clone()),
        Some(PathBuf::from("/home/u/Apps/Anix.AppImage"))
    );
    assert_eq!(autostart::exec_target(Some("".into()), exe.clone()), exe);
    assert_eq!(autostart::exec_target(None, exe.clone()), exe);
}

#[test]
fn autostart_entry_is_written_replaced_and_removed() {
    let dir = TempDir::new("autostart");
    let path = autostart::entry_path(&dir.0);
    assert!(!autostart::is_installed(&dir.0));

    autostart::set_enabled(&dir.0, Some(Path::new("/opt/a/anix"))).unwrap();
    assert!(autostart::is_installed(&dir.0));
    assert!(std::fs::read_to_string(&path).unwrap().contains("Exec=/opt/a/anix --minimized"));

    autostart::set_enabled(&dir.0, Some(Path::new("/opt/b/anix"))).unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("Exec=/opt/b/anix --minimized"));

    autostart::set_enabled(&dir.0, None).unwrap();
    assert!(!path.exists());
    // Removing again is fine.
    autostart::set_enabled(&dir.0, None).unwrap();
}

#[test]
fn a_hand_made_autostart_file_is_left_alone() {
    let dir = TempDir::new("autostart-foreign");
    let path = autostart::entry_path(&dir.0);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[Desktop Entry]\nExec=anix --something\n").unwrap();
    assert!(autostart::set_enabled(&dir.0, Some(Path::new("/opt/anix"))).is_err());
    autostart::set_enabled(&dir.0, None).unwrap();
    assert!(path.exists(), "not removed");
    assert!(!autostart::is_installed(&dir.0));
}

#[test]
fn system_settings_default_to_off() {
    assert_eq!(SystemSettings::from_json(&json!({})), SystemSettings::default());
    let on = SystemSettings::from_json(&json!({ "minimize_to_tray": true, "start_with_desktop": true }));
    assert!(on.minimize_to_tray && on.start_with_desktop);
    let junk = SystemSettings::from_json(&json!({ "minimize_to_tray": "yes" }));
    assert!(!junk.minimize_to_tray);
}

// ------------------------------------------------------------------ hardware report

const VAINFO_INTEL: &str = "Trying display: wayland
vainfo: VA-API version: 1.22 (libva 2.22.0)
vainfo: Driver version: Intel iHD driver for Intel(R) Gen Graphics - 24.3.4 ()
vainfo: Supported profile and entrypoints
      VAProfileNone                   :\tVAEntrypointVideoProc
      VAProfileMPEG2Simple            :\tVAEntrypointVLD
      VAProfileH264Main               :\tVAEntrypointVLD
      VAProfileH264Main               :\tVAEntrypointEncSliceLP
      VAProfileH264High               :\tVAEntrypointVLD
      VAProfileHEVCMain               :\tVAEntrypointVLD
      VAProfileVP9Profile0            :\tVAEntrypointVLD
      VAProfileAV1Profile0            :\tVAEntrypointVLD
";

const VAINFO_FAILED: &str = "libva info: VA-API version 1.23.0
libva info: Trying to open /usr/lib64/dri/iHD_drv_video.so
libva info: va_openDriver() returns -1
libva error: /usr/lib64/dri/i965_drv_video.so init failed
libva info: va_openDriver() returns -1
vaInitialize failed with error code -1 (unknown libva error),exit
Trying display: wayland
";

#[test]
fn vainfo_output_is_read() {
    let info = hwaccel::parse_vainfo(VAINFO_INTEL).expect("a driver");
    assert_eq!(info.vaapi_version.as_deref(), Some("1.22 (libva 2.22.0)"));
    assert!(info.driver.as_deref().unwrap().starts_with("Intel iHD driver"));
    assert_eq!(info.decode, vec!["H.264", "HEVC", "VP9", "AV1", "MPEG-2"]);
    assert!(info.profiles.contains(&"VAProfileH264Main: VAEntrypointEncSliceLP".to_string()));
    assert_eq!(info.profiles.len(), 8);
}

#[test]
fn a_failed_vainfo_run_reports_its_error_lines() {
    assert_eq!(hwaccel::parse_vainfo(VAINFO_FAILED), None);
    let reason = hwaccel::vainfo_failure(VAINFO_FAILED);
    assert!(reason.contains("init failed"));
    assert!(reason.contains("vaInitialize failed"));
    assert!(!reason.contains("Trying display"));
    assert_eq!(hwaccel::vainfo_failure(""), "vainfo did not answer");
}

#[test]
fn session_kind_follows_the_environment() {
    assert_eq!(hwaccel::session_kind(None, Some("wayland-0"), Some("wayland"), Some(":0")), "wayland");
    assert_eq!(hwaccel::session_kind(None, None, Some("x11"), Some(":0")), "x11");
    assert_eq!(hwaccel::session_kind(Some("x11"), Some("wayland-0"), Some("wayland"), Some(":0")), "x11");
    assert_eq!(hwaccel::session_kind(None, None, None, None), "unknown");
}

#[test]
fn gstreamer_folders_follow_the_environment() {
    let defaults = ["/usr/lib64/gstreamer-1.0"];
    assert_eq!(hwaccel::gst_plugin_dirs(None, None, &defaults), vec![PathBuf::from("/usr/lib64/gstreamer-1.0")]);
    // The AppImage sets the system path, which replaces the defaults.
    assert_eq!(
        hwaccel::gst_plugin_dirs(Some("/x:/y"), Some("/app/gst:/x"), &defaults),
        vec![PathBuf::from("/x"), PathBuf::from("/y"), PathBuf::from("/app/gst")]
    );
}

fn report_with(vendor: &str, vaapi: hwaccel::VaapiCheck) -> hwaccel::HardwareReport {
    hwaccel::HardwareReport {
        session: "wayland".into(),
        gpu_vendor: Some(vendor.into()),
        render_node: Some("/dev/dri/renderD128".into()),
        vaapi,
        gstreamer: hwaccel::GstPlugins {
            va: Some("/usr/lib64/gstreamer-1.0/libgstva.so".into()),
            libav: Some("/usr/lib64/gstreamer-1.0/libgstlibav.so".into()),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn tips_name_the_driver_package_for_the_gpu() {
    let failed = hwaccel::VaapiCheck {
        vainfo_installed: true,
        ok: false,
        info: None,
        error: Some("init failed".into()),
    };
    let intel = hwaccel::tips(&report_with("intel", failed.clone()));
    assert!(intel.iter().any(|t| t.contains("intel-media-driver")));
    let amd = hwaccel::tips(&report_with("amd", failed));
    assert!(amd.iter().any(|t| t.contains("mesa-va-drivers")));

    let missing = hwaccel::tips(&report_with("intel", hwaccel::VaapiCheck::default()));
    assert!(missing.iter().any(|t| t.contains("libva-utils")));

    let working = hwaccel::VaapiCheck {
        vainfo_installed: true,
        ok: true,
        info: hwaccel::parse_vainfo(VAINFO_INTEL),
        error: None,
    };
    let ready = hwaccel::tips(&report_with("intel", working.clone()));
    assert_eq!(ready.len(), 1);
    assert!(ready[0].contains("looks ready"));

    let mut no_h264 = working.clone();
    no_h264.info.as_mut().unwrap().decode.retain(|c| c != "H.264");
    assert!(hwaccel::tips(&report_with("amd", no_h264)).iter().any(|t| t.contains("cannot decode H.264")));

    let mut dmabuf = report_with("intel", working);
    dmabuf.dmabuf_disabled = true;
    dmabuf.dmabuf_set_by_app = true;
    assert!(hwaccel::tips(&dmabuf).iter().any(|t| t.contains("WEBKIT_DISABLE_DMABUF_RENDERER=0")));
}

#[test]
fn tips_explain_a_bundled_gstreamer_without_va() {
    let mut report = report_with("intel", hwaccel::VaapiCheck::default());
    report.gstreamer.va = None;
    report.running_from_appimage = true;
    assert!(hwaccel::tips(&report).iter().any(|t| t.contains("come with the AppImage")));
    report.running_from_appimage = false;
    assert!(hwaccel::tips(&report).iter().any(|t| t.contains("gstreamer1-plugins-bad-free")));
}

// ------------------------------------------------------------------ tray

fn bell(id: i64, title: &str) -> Bell {
    Bell {
        media_id: id,
        media_title: title.into(),
        cover_image: None,
        enabled_at: 0,
    }
}

#[test]
fn airing_next_lists_future_episodes_of_belled_anime_soonest_first() {
    let bells = vec![bell(1, "First"), bell(2, "Second")];
    let airings = vec![
        Airing { media_id: 2, episode: 5, airing_at: 3_000 },
        Airing { media_id: 1, episode: 7, airing_at: 2_000 },
        Airing { media_id: 1, episode: 6, airing_at: 500 },
        Airing { media_id: 3, episode: 1, airing_at: 1_500 },
        Airing { media_id: 1, episode: 7, airing_at: 2_000 },
        Airing { media_id: 2, episode: 6, airing_at: 9_000 },
    ];
    let list = tray::pick_upcoming(&airings, &bells, 1_000, 2);
    assert_eq!(
        list.iter().map(|u| (u.media_id, u.episode)).collect::<Vec<_>>(),
        vec![(1, 7), (2, 5)]
    );
    assert_eq!(list[0].title, "First");
}

#[test]
fn airing_next_labels_are_short() {
    let long = tray::Upcoming {
        media_id: 1,
        title: "A".repeat(80),
        episode: 3,
        airing_at: 1_000_000,
    };
    let label = tray::upcoming_label(&long, 1_000_000 - 60);
    assert!(label.starts_with(&format!("{}…", "A".repeat(39))));
    assert!(label.contains("· Ep 3 · "));
}

// ------------------------------------------------------------------ plugins

#[test]
fn directory_is_read_and_cleaned() {
    let data = br#"{ "lists": [
        { "name": "Anix extensions", "description": "Made for Anix.", "url": "https://anix-linux.vercel.app/extensions", "languages": ["en", "JA", "toolong", "1"] },
        { "name": "No https", "url": "http://example.com/x.json" },
        { "name": "", "url": "https://example.com/empty-name.json" },
        { "name": "Twice", "url": "https://anix-linux.vercel.app/extensions" },
        { "url": "https://example.com/no-name.json" },
        "not an object",
        { "name": "Minimal", "url": "https://example.com/list.json" }
    ] }"#;
    let lists = health::parse_directory(data).unwrap();
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0].name, "Anix extensions");
    assert_eq!(lists[0].languages, vec!["en", "ja"]);
    assert_eq!(lists[1].name, "Minimal");
    assert_eq!(lists[1].description, "");
    assert!(lists[1].languages.is_empty());

    assert!(health::parse_directory(b"not json").is_err());
    assert!(health::parse_directory(br#"{ "other": [] }"#).is_err());
    assert_eq!(health::directory_url("https://anix-linux.vercel.app/"), "https://anix-linux.vercel.app/directory.json");
}

#[test]
fn the_sites_own_directory_parses() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../website/directory.json");
    let Ok(data) = std::fs::read(&path) else {
        return;
    };
    let lists = health::parse_directory(&data).unwrap();
    assert!(lists.iter().any(|l| l.url == "https://anix-linux.vercel.app/extensions" && l.name == "Anix extensions"));
}

#[test]
fn health_results_are_classified() {
    let ms = Duration::from_millis(420);
    let ok = health::classify("p", Ok(json!([{ "id": "1" }, { "id": "2" }])), ms);
    assert_eq!((ok.status.as_str(), ok.results, ok.latency_ms), ("ok", 2, 420));
    assert_eq!(health::classify("p", Ok(json!([])), ms).status, "empty");
    assert_eq!(health::classify("p", Ok(json!({})), ms).status, "error");
    assert_eq!(health::classify("p", Err(AppError::CaptchaRequired), ms).status, "verification");
    let timeout = health::classify("p", Err(AppError::provider("plugin", "search timed out")), ms);
    assert_eq!(timeout.status, "timeout");
    let down = health::classify("p", Err(AppError::Unreachable("site: dns".into())), ms);
    assert_eq!(down.status, "error");
    assert!(down.error.unwrap().contains("connection failed"));
}

#[test]
fn the_test_title_is_a_complete_media_object() {
    let media = health::test_media();
    for key in ["id", "idMal", "englishTitle", "romajiTitle", "synonyms", "startDate", "isAdult", "format"] {
        assert!(media.get(key).is_some(), "{} is set", key);
    }
}

// ------------------------------------------------------------------ background release checks

const APPIMAGE: &str = "/run/media/me/Extra Volume/Anix.AppImage";

#[test]
fn background_switch_is_read_from_settings() {
    assert!(!SystemSettings::from_json(&json!({})).background_alerts);
    assert!(SystemSettings::from_json(&json!({ "background_alerts": true })).background_alerts);
    assert_eq!(background::starter(true), background::Starter::SystemdTimer);
    assert_eq!(background::starter(false), background::Starter::Autostart);
}

#[test]
fn background_units_start_the_appimage_without_a_window() {
    let service = background::service_unit(Path::new(APPIMAGE));
    assert!(service.contains("ExecStart=\"/run/media/me/Extra Volume/Anix.AppImage\" --background\n"));
    assert!(service.contains("Type=simple"));
    // Stopping the unit must not take down an app window opened from one of its alerts.
    assert!(service.contains("KillMode=process\n"));
    assert!(service.contains("X-Anix-Background=true"));

    let timer = background::timer_unit();
    assert!(timer.contains("OnStartupSec=1min"));
    assert!(timer.contains("OnUnitInactiveSec=15min"));
    assert!(timer.contains("Unit=anix-release-check.service"));
    assert!(timer.contains("WantedBy=timers.target"));

    let entry = background::autostart_entry(Path::new(APPIMAGE));
    assert!(entry.contains("Exec=\"/run/media/me/Extra Volume/Anix.AppImage\" --background\n"));
    assert!(entry.contains("NoDisplay=true"));

    assert_eq!(background::quote_systemd_arg("/usr/bin/anix"), "/usr/bin/anix");
    assert_eq!(background::quote_systemd_arg("/a/100%/$HOME"), "/a/100%%/$$HOME");
    assert_eq!(background::quote_systemd_arg("/a b/\"c\\"), "\"/a b/\\\"c\\\\\"");
}

#[test]
fn background_switch_writes_and_removes_its_files() {
    let dir = TempDir::new("background");
    let exec = Path::new(APPIMAGE);
    let (service, timer) = background::unit_paths(&dir.0);
    let entry = background::autostart_path(&dir.0);
    assert!(service.ends_with("systemd/user/anix-release-check.service"));
    assert!(timer.ends_with("systemd/user/anix-release-check.timer"));
    assert!(entry.ends_with("autostart/anix-release-check.desktop"));

    // On with systemd: the two units, and they changed.
    assert!(background::install_files(&dir.0, Some(exec), background::Starter::SystemdTimer).unwrap());
    assert_eq!(std::fs::read_to_string(&service).unwrap(), background::service_unit(exec));
    assert_eq!(std::fs::read_to_string(&timer).unwrap(), background::timer_unit());
    assert!(!entry.exists());
    assert!(background::is_installed(&dir.0));
    // Writing the same again changes nothing.
    assert!(!background::install_files(&dir.0, Some(exec), background::Starter::SystemdTimer).unwrap());

    // Without systemd the autostart entry replaces the units.
    background::install_files(&dir.0, Some(exec), background::Starter::Autostart).unwrap();
    assert_eq!(std::fs::read_to_string(&entry).unwrap(), background::autostart_entry(exec));
    assert!(!service.exists() && !timer.exists());

    // Off: everything goes.
    background::install_files(&dir.0, None, background::Starter::SystemdTimer).unwrap();
    assert!(!entry.exists() && !service.exists() && !timer.exists());
    assert!(!background::is_installed(&dir.0));

    // A file of the same name that Anix did not write is left alone.
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(&entry, "[Desktop Entry]\nExec=mine\n").unwrap();
    assert!(background::install_files(&dir.0, Some(exec), background::Starter::Autostart).is_err());
    background::install_files(&dir.0, None, background::Starter::Autostart).unwrap();
    assert_eq!(std::fs::read_to_string(&entry).unwrap(), "[Desktop Entry]\nExec=mine\n");
}

#[test]
fn locks_are_exclusive_until_dropped() {
    let dir = TempDir::new("locks");
    let path = dir.0.join(background::BACKGROUND_LOCK);
    let first = background::try_lock(&path).unwrap();
    assert!(first.is_some());
    // A second background process (another open of the file) does not get it.
    assert!(background::try_lock(&path).unwrap().is_none());
    assert!(background::is_held(&path));
    drop(first);
    assert!(!background::is_held(&path));
    assert!(background::try_lock(&path).unwrap().is_some());
}

#[tokio::test]
async fn exactly_one_process_owns_the_checking() {
    let dir = TempDir::new("handoff");
    let checker = dir.0.join(background::CHECKER_LOCK);

    // Only the background process runs: it checks.
    let mut handoff = background::Handoff::new(&dir.0);
    assert!(handoff.may_check());
    assert!(handoff.holds_checker());

    // The app starts: it takes its own lock at once, but waits for the checker lock.
    let claim_dir = dir.0.clone();
    let app = tokio::spawn(async move { background::claim_in(&claim_dir, Duration::from_millis(20)).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!app.is_finished(), "the app must not check while the background process does");
    assert!(background::is_held(&dir.0.join(background::APP_LOCK)));

    // The background process's next step stands by and lets go; the app takes over.
    assert!(!handoff.may_check());
    assert!(!handoff.holds_checker());
    let held = tokio::time::timeout(Duration::from_secs(2), app).await.unwrap().unwrap();
    assert_eq!(held.len(), 2);
    assert!(background::is_held(&checker));
    // While the app runs, the background process keeps standing by.
    assert!(!handoff.may_check());

    // The app quits: the background process checks again.
    drop(held);
    assert!(handoff.may_check());
    assert!(handoff.holds_checker());
}

#[tokio::test]
async fn an_idle_background_checker_does_not_hold_up_the_app() {
    let dir = TempDir::new("idle-handoff");

    // The background process checked, found no bells, and goes into its long sleep.
    let mut handoff = background::Handoff::new(&dir.0);
    assert!(handoff.may_check());
    assert!(handoff.idle(), "it held the checker lock");
    assert!(!handoff.holds_checker(), "nothing to check: the checker lock is let go of");

    // The app starts during that sleep and takes over at once, not after the sleep.
    let held = tokio::time::timeout(
        Duration::from_millis(500),
        background::claim_in(&dir.0, Duration::from_millis(20)),
    )
    .await
    .expect("the app's notifier is not stalled by an idle background process");
    assert_eq!(held.len(), 2);

    // When the background process wakes, it stands by.
    assert!(!handoff.may_check());
    drop(held);
    assert!(handoff.may_check(), "with the app gone, it checks again");
}

// ---- web page crash recovery ------------------------------------------------------------

use anix_lib::system::page_crash::{allow_reload, route_from_url, RELOADS_PER_MINUTE};

#[test]
fn a_crashed_page_is_reloaded_at_most_three_times_a_minute() {
    let mut crashes = Vec::new();
    for i in 0..RELOADS_PER_MINUTE as u64 {
        assert!(allow_reload(&mut crashes, 100 + i), "crash {i}");
    }
    // One more within the minute is a crash loop: no further reload.
    assert!(!allow_reload(&mut crashes, 110));
    assert!(!allow_reload(&mut crashes, 150));
    // A minute after the last crashes, a new crash is reloaded again.
    assert!(allow_reload(&mut crashes, 300));
}

#[test]
fn the_route_to_reopen_comes_from_the_hash_route_of_the_page() {
    assert_eq!(route_from_url("tauri://localhost/#/watch/21/3?cat=sub").as_deref(), Some("/watch/21/3?cat=sub"));
    assert_eq!(route_from_url("http://localhost:1420/#/settings").as_deref(), Some("/settings"));
    // The start page and anything that is not an in-app route are not reopened.
    assert_eq!(route_from_url("tauri://localhost/"), None);
    assert_eq!(route_from_url("tauri://localhost/#//evil.example"), None);
    assert_eq!(route_from_url("not a url"), None);
}

// ---------------------------------------------------------------------------------------
// Restart arguments
// ---------------------------------------------------------------------------------------

#[test]
fn a_restart_drops_the_minimised_background_and_link_arguments_but_keeps_other_flags() {
    use std::ffi::OsString;
    let args: Vec<OsString> = [
        "/opt/Anix.AppImage",
        "--minimized",
        "--web-player",
        "anix://anime/123",
        "ANILILI://anime/-300000099",
        "--background",
        "--verbose",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let kept: Vec<OsString> = ["/opt/Anix.AppImage", "--web-player", "--verbose"].iter().map(OsString::from).collect();
    assert_eq!(anix_lib::system::restart_args(&args), kept);
}

#[test]
fn a_restart_keeps_the_program_name_even_when_it_looks_like_a_flag() {
    use std::ffi::OsString;
    let args = vec![OsString::from("--minimized")];
    assert_eq!(anix_lib::system::restart_args(&args), args, "argv[0] is the program, never filtered");
    assert!(anix_lib::system::restart_args(&[]).is_empty());
}

// ---------------------------------------------------------------------------------------
// Clicks on alerts from the background process
// ---------------------------------------------------------------------------------------

#[test]
fn a_background_alert_click_starts_the_app_on_its_anime() {
    use anix_lib::notifier::Click;
    assert_eq!(background::launch_args(Click::Nothing), None, "a dismissed alert starts nothing");
    assert_eq!(background::launch_args(Click::Focus), Some(vec![]), "no anime: just start or raise the app");
    assert_eq!(background::launch_args(Click::OpenAnime(21)), Some(vec!["anix://anime/21".to_string()]));
    assert_eq!(
        background::launch_args(Click::OpenAnime(-300000099)),
        Some(vec!["anix://anime/-300000099".to_string()]),
        "stand-in ids are anime too"
    );
}

// ---------------------------------------------------------------------------------------
// anix:// handler for the AppImage
// ---------------------------------------------------------------------------------------

use anix_lib::system::url_handler;

#[test]
fn the_link_handler_entry_opens_both_schemes_with_the_appimage() {
    let entry = url_handler::handler_entry(Path::new("/home/x/Apps/Anix 1.3.AppImage")).unwrap();
    assert!(entry.starts_with("[Desktop Entry]\n"));
    assert!(entry.contains("\nExec=\"/home/x/Apps/Anix 1.3.AppImage\" %u\n"), "{entry}");
    assert!(entry.contains("\nMimeType=x-scheme-handler/anix;x-scheme-handler/anilili;\n"));
    assert!(entry.contains("\nNoDisplay=true\n"));
    assert!(entry.contains("\nX-Anix-AppImage=/home/x/Apps/Anix 1.3.AppImage\n"));
    // A line break in the path would add keys of its own to the file.
    assert!(url_handler::handler_entry(Path::new("/tmp/a\nExec=evil")).is_none());
    // A path that is not UTF-8 would be recorded mangled, never match, and be rewritten on
    // every start.
    use std::os::unix::ffi::OsStrExt;
    let not_utf8 = Path::new(std::ffi::OsStr::from_bytes(b"/apps/Anix-\xff.AppImage"));
    assert!(url_handler::handler_entry(not_utf8).is_none());
}

#[test]
fn the_link_handler_is_written_once_and_again_only_when_its_appimage_is_gone() {
    let ours = url_handler::handler_entry(Path::new("/apps/Anix.AppImage")).unwrap();
    let exists = |p: &Path| p == Path::new("/apps/Anix.AppImage");
    assert!(url_handler::needs_writing(None, exists), "first run");
    assert!(!url_handler::needs_writing(Some(&ours), exists), "ours, still pointing at a real file");
    assert!(url_handler::needs_writing(Some(&ours), |_| false), "ours, but the AppImage was moved or deleted");
    let hand_made = "[Desktop Entry]\nType=Application\nExec=/opt/other %u\nMimeType=x-scheme-handler/anix;\n";
    assert!(!url_handler::needs_writing(Some(hand_made), |_| false), "a file Anix did not write is never touched");
}

#[test]
fn registering_the_link_handler_writes_into_the_data_home_once() {
    let data_home = TempDir::new("url-handler");
    let appimage = data_home.0.join("Anix.AppImage");
    std::fs::write(&appimage, b"").unwrap();

    assert!(url_handler::register(&data_home.0, &appimage).unwrap(), "written on the first run");
    let path = url_handler::handler_path(&data_home.0);
    assert_eq!(path, data_home.0.join("applications").join(url_handler::FILE_NAME));
    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written, url_handler::handler_entry(&appimage).unwrap());
    assert!(!url_handler::register(&data_home.0, &appimage).unwrap(), "left alone on later runs");

    // The AppImage moved: the next run points the handler at the new place.
    let moved = data_home.0.join("Anix-moved.AppImage");
    std::fs::rename(&appimage, &moved).unwrap();
    assert!(url_handler::register(&data_home.0, &moved).unwrap());
    assert!(std::fs::read_to_string(&path).unwrap().contains("Anix-moved.AppImage"));

    // A hand-made file of the same name stays as it is.
    std::fs::write(&path, "[Desktop Entry]\nExec=/opt/mine %u\n").unwrap();
    std::fs::remove_file(&moved).unwrap();
    assert!(!url_handler::register(&data_home.0, &appimage).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[Desktop Entry]\nExec=/opt/mine %u\n");
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "no temp files left behind");

    // So does one that cannot be read as text.
    std::fs::write(&path, b"[Desktop Entry]\nName=\xff\n").unwrap();
    assert!(!url_handler::register(&data_home.0, &appimage).unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), b"[Desktop Entry]\nName=\xff\n");
}
