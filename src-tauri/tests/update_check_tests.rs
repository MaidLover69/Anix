//! The update check against the website's `version.json`, with a loopback server as the site.

mod plugin_support;

use anix_lib::clients::update::{
    appimage_target, backup_path, default_download_url, download_and_install, fetch_update_info,
    first_bad_version_warning, install_verified, is_bad_version, is_newer_version, offered_release, parse_bad_versions,
    parse_version_document, record_update, roll_back, rollback_available, sha256_file, version_document_url,
    AppImageAsset, UpdateInfo, ROLLBACK_FILE, SITE_URL,
};
use std::path::{Path, PathBuf};
use plugin_support::{Req, Resp, Site};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// A site whose `/version.json` answers with whatever is put in the returned slot.
async fn site() -> (Site, Arc<Mutex<Resp>>) {
    let answer = Arc::new(Mutex::new(Resp::status(404)));
    let slot = answer.clone();
    let site = Site::start(move |request: &Req| match request.route() {
        "/version.json" => {
            let answer = slot.lock().unwrap();
            Resp { status: answer.status, headers: answer.headers.clone(), body: answer.body.clone() }
        }
        _ => Resp::status(404),
    })
    .await;
    (site, answer)
}

#[test]
fn the_site_address_is_a_web_address_without_a_trailing_slash_problem() {
    assert!(SITE_URL.starts_with("https://") || SITE_URL.starts_with("http://"));
    if option_env!("ANIX_SITE_URL").map_or(true, str::is_empty) {
        assert_eq!(SITE_URL, "https://anix-linux.vercel.app");
    }
    assert_eq!(version_document_url("https://anix-linux.vercel.app/"), "https://anix-linux.vercel.app/version.json");
    assert_eq!(version_document_url("https://anix-linux.vercel.app"), "https://anix-linux.vercel.app/version.json");
    assert_eq!(default_download_url("https://anix-linux.vercel.app/"), "https://anix-linux.vercel.app/download");
}

#[test]
fn the_version_document_is_read_leniently() {
    let site = "https://site.example";
    let full = parse_version_document(
        &json!({ "version": "1.1.0", "notes": "Themes", "published_at": "2026-10-10", "download_url": "https://dl.example/Anix.AppImage" }),
        site,
    );
    assert_eq!(
        full,
        Some(UpdateInfo::new("1.1.0".into(), "Themes".into(), "2026-10-10".into(), "https://dl.example/Anix.AppImage".into()))
    );

    // Only the version is required.
    let bare = parse_version_document(&json!({ "version": " v1.2.0 " }), site).unwrap();
    assert_eq!(bare.version, "1.2.0");
    assert_eq!(bare.changelog, "No changelog available.");
    assert_eq!(bare.published_at, "");
    assert_eq!(bare.release_url, "https://site.example/download");

    // Fields of the wrong type count as missing.
    let odd = parse_version_document(&json!({ "version": "2.0.0", "notes": 5, "published_at": null, "download_url": ["x"] }), site).unwrap();
    assert_eq!((odd.changelog.as_str(), odd.published_at.as_str()), ("No changelog available.", ""));
    assert_eq!(odd.release_url, "https://site.example/download");

    // Only web addresses are handed to the browser.
    for bad in ["file:///etc/passwd", "javascript:alert(1)", "anix://anime/1", "/download", "not a url", "", "https://"] {
        let info = parse_version_document(&json!({ "version": "1.0.0", "download_url": bad }), site).unwrap();
        assert_eq!(info.release_url, "https://site.example/download", "{:?}", bad);
    }
    let plain = parse_version_document(&json!({ "version": "1.0.0", "download_url": "http://mirror.example/a" }), site).unwrap();
    assert_eq!(plain.release_url, "http://mirror.example/a");

    // No version, no update.
    assert_eq!(parse_version_document(&json!({ "notes": "x" }), site), None);
    assert_eq!(parse_version_document(&json!({ "version": "" }), site), None);
    assert_eq!(parse_version_document(&json!({ "version": 3 }), site), None);
    assert_eq!(parse_version_document(&json!(["1.0.0"]), site), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_update_check_reads_version_json_from_the_site() {
    let (site, answer) = site().await;
    let base = site.base();

    // Nothing published yet.
    assert_eq!(fetch_update_info(&client(), &base).await, Ok(None));

    *answer.lock().unwrap() = Resp::json(json!({
        "version": "1.2.0", "notes": "Fixes", "published_at": "2026-11-01", "download_url": "https://dl.example/Anix_1.2.0.AppImage"
    }));
    let info = fetch_update_info(&client(), &format!("{}/", base)).await.unwrap().unwrap();
    assert_eq!(info, UpdateInfo::new("1.2.0".into(), "Fixes".into(), "2026-11-01".into(), "https://dl.example/Anix_1.2.0.AppImage".into()));
    assert!(site.hits().iter().all(|hit| hit == "GET /version.json"), "{:?}", site.hits());

    // Without a download address the site's download page is used.
    *answer.lock().unwrap() = Resp::json(json!({ "version": "1.2.1" }));
    let info = fetch_update_info(&client(), &base).await.unwrap().unwrap();
    assert_eq!(info.release_url, format!("{}/download", base));

    // The result has the shape the frontend reads.
    let shape = serde_json::to_value(&info).unwrap();
    let mut keys: Vec<&str> = shape.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["changelog", "published_at", "release_url", "version"]);

    // A site that answers every address with its own page publishes no update information.
    *answer.lock().unwrap() = Resp::html("<!doctype html><html><body>Anix</body></html>");
    assert_eq!(fetch_update_info(&client(), &base).await, Ok(None));
    *answer.lock().unwrap() = Resp::json(json!({ "notes": "no version here" }));
    assert_eq!(fetch_update_info(&client(), &base).await, Ok(None));

    // Real failures are errors, so the UI does not report "up to date".
    *answer.lock().unwrap() = Resp::status(500);
    let failed = fetch_update_info(&client(), &base).await.unwrap_err();
    assert!(failed.contains("HTTP 500"), "{}", failed);
    *answer.lock().unwrap() = Resp::new(200, "application/json", "{ \"version\": ");
    let garbled = fetch_update_info(&client(), &base).await.unwrap_err();
    assert!(garbled.contains("Unexpected response"), "{}", garbled);
    *answer.lock().unwrap() = Resp::new(200, "application/json", vec![b' '; 300 * 1024]);
    let huge = fetch_update_info(&client(), &base).await.unwrap_err();
    assert!(huge.contains("too large"), "{}", huge);

    // A site that is not there at all.
    drop(site);
    let unreachable = fetch_update_info(&client(), &base).await.unwrap_err();
    assert!(unreachable.contains("Could not reach the update server"), "{}", unreachable);
}

// ---------------------------------------------------------------------------------------
// The in-app AppImage updater
// ---------------------------------------------------------------------------------------

const HASH_OF_NEW: &str = "c2ab5b8a3a0f9a9b6c4b2a5f4c4f7b1a1d5c6e7f8091a2b3c4d5e6f708192a3b";

/// A temp folder with a stand-in "running" AppImage.
struct Install(PathBuf);

impl Install {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("anix-update-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Anix.AppImage"), b"old version").unwrap();
        Self(dir)
    }
    fn target(&self) -> PathBuf {
        self.0.join("Anix.AppImage")
    }
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(&self.0).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for Install {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sha256(bytes: &[u8]) -> String {
    let file = std::env::temp_dir().join(format!("anix-hash-{}-{}", std::process::id(), bytes.len()));
    std::fs::write(&file, bytes).unwrap();
    let hash = sha256_file(&file).unwrap();
    let _ = std::fs::remove_file(file);
    hash
}

#[test]
fn the_manifest_lists_the_appimage_with_its_checksum() {
    let site = "https://site.example";
    let hash = "AB".repeat(32);
    let info = parse_version_document(
        &json!({ "version": "1.3.0", "appimage": { "url": "https://dl.example/Anix_1.3.0.AppImage", "sha256": hash, "size": 9000 } }),
        site,
    )
    .unwrap();
    assert_eq!(
        info.appimage,
        Some(AppImageAsset { url: "https://dl.example/Anix_1.3.0.AppImage".into(), sha256: "ab".repeat(32), size: Some(9000) })
    );
    let shape = serde_json::to_value(&info).unwrap();
    assert_eq!(shape["appimage"]["sha256"], json!("ab".repeat(32)));

    // Without a usable checksum or address there is nothing to install.
    for bad in [
        json!({ "url": "https://dl.example/a", "sha256": "abc" }),
        json!({ "url": "https://dl.example/a", "sha256": "zz".repeat(32) }),
        json!({ "url": "file:///tmp/a", "sha256": "ab".repeat(32) }),
        json!({ "sha256": "ab".repeat(32) }),
        json!("https://dl.example/a"),
    ] {
        let info = parse_version_document(&json!({ "version": "1.3.0", "appimage": bad }), site).unwrap();
        assert_eq!(info.appimage, None, "{}", bad);
    }
    assert_eq!(parse_version_document(&json!({ "version": "1.3.0" }), site).unwrap().appimage, None);
}

#[test]
fn versions_compare_like_the_frontend() {
    assert!(is_newer_version("1.3.0", "1.2.0"));
    assert!(is_newer_version("v1.10.0", "1.9.9"));
    assert!(is_newer_version("1.2.1", "1.2"));
    assert!(!is_newer_version("1.2.0", "1.2.0"));
    assert!(!is_newer_version("1.2.0", "1.3.0"));
    assert!(!is_newer_version("1.3.0-beta", "1.3.0"));
}

#[test]
fn only_a_writable_appimage_of_a_normal_user_can_be_replaced() {
    let install = Install::new("target");
    let target = install.target();
    assert_eq!(appimage_target(Some(target.as_os_str()), 1000), Ok(target.clone()));
    assert!(appimage_target(None, 1000).unwrap_err().contains("not running as an AppImage"));
    assert!(appimage_target(Some(target.as_os_str()), 0).unwrap_err().contains("root"));
    assert!(appimage_target(Some(std::ffi::OsStr::new("Anix.AppImage")), 1000).is_err(), "relative");
    assert!(appimage_target(Some(install.0.join("missing").as_os_str()), 1000).is_err());

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&install.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    if unsafe { libc::geteuid() } != 0 {
        assert!(appimage_target(Some(target.as_os_str()), 1000).unwrap_err().contains("cannot write"));
    }
    std::fs::set_permissions(&install.0, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_mismatching_checksum_aborts_and_keeps_the_old_file() {
    let install = Install::new("mismatch");
    let downloaded = install.0.join(".Anix.AppImage.anix-update");
    std::fs::write(&downloaded, b"tampered").unwrap();

    let error = install_verified(&downloaded, &install.target(), HASH_OF_NEW).unwrap_err();

    assert!(error.contains("does not match its checksum"), "{}", error);
    assert_eq!(std::fs::read(install.target()).unwrap(), b"old version");
    assert_eq!(install.names(), vec!["Anix.AppImage"], "the download is deleted and no backup is made");
}

#[test]
fn a_verified_update_replaces_the_appimage_and_keeps_a_backup() {
    let install = Install::new("verified");
    let downloaded = install.0.join(".Anix.AppImage.anix-update");
    std::fs::write(&downloaded, b"new version").unwrap();

    let backup = install_verified(&downloaded, &install.target(), &sha256(b"new version").to_uppercase()).unwrap();

    assert_eq!(backup, backup_path(&install.target()));
    assert_eq!(std::fs::read(install.target()).unwrap(), b"new version");
    assert_eq!(std::fs::read(&backup).unwrap(), b"old version");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(install.target()).unwrap().permissions().mode() & 0o111, 0o111, "executable");
    assert_eq!(install.names(), vec!["Anix.AppImage", "Anix.AppImage.bak"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_update_is_downloaded_next_to_the_appimage_and_checked_before_it_replaces_it() {
    let site = Site::start(|request: &Req| match request.route() {
        "/Anix_1.3.0.AppImage" => Resp::new(200, "application/octet-stream", b"new version".to_vec()),
        _ => Resp::status(404),
    })
    .await;
    let install = Install::new("download");
    let good = AppImageAsset { url: site.url("/Anix_1.3.0.AppImage"), sha256: sha256(b"new version"), size: None };

    // A file that does not match the manifest never replaces the AppImage.
    let tampered = AppImageAsset { sha256: HASH_OF_NEW.to_string(), ..good.clone() };
    let error = download_and_install(&client(), &tampered, &install.target(), |_| {}).await.unwrap_err();
    assert!(error.contains("checksum"), "{}", error);
    assert_eq!(std::fs::read(install.target()).unwrap(), b"old version");
    assert_eq!(install.names(), vec!["Anix.AppImage"]);

    let missing = AppImageAsset { url: site.url("/gone.AppImage"), ..good.clone() };
    assert!(download_and_install(&client(), &missing, &install.target(), |_| {}).await.unwrap_err().contains("HTTP 404"));
    assert_eq!(install.names(), vec!["Anix.AppImage"]);

    let mut seen = Vec::new();
    download_and_install(&client(), &good, &install.target(), |f| seen.push(f)).await.unwrap();
    assert_eq!(std::fs::read(install.target()).unwrap(), b"new version");
    assert_eq!(std::fs::read(backup_path(&install.target())).unwrap(), b"old version");
    assert_eq!(seen.last().copied(), Some(1.0));
    assert!(Path::new(&install.0).join("Anix.AppImage.bak").exists());
    assert_eq!(install.names(), vec!["Anix.AppImage", "Anix.AppImage.bak"], "the partial download is gone");
}

#[test]
fn the_site_can_list_versions_with_a_known_problem() {
    let listed = parse_bad_versions(&json!(["1.4.0", "v1.4.1", " 1.4.2 ", 7, null, "", "1.4.0; rm -rf /", "x".repeat(40)]));
    assert_eq!(listed, vec!["1.4.0", "1.4.1", "1.4.2"], "only plain version strings are kept");
    assert!(parse_bad_versions(&json!("1.4.0")).is_empty(), "not a list");
    assert!(parse_bad_versions(&serde_json::Value::Null).is_empty());
    let many: Vec<String> = (0..80).map(|n| format!("1.0.{}", n)).collect();
    assert_eq!(parse_bad_versions(&json!(many)).len(), 50, "the list is capped");

    let info = parse_version_document(&json!({"version": "1.4.1", "bad_versions": ["1.4.0"]}), SITE_URL).unwrap();
    assert!(is_bad_version(&info, "1.4.0"));
    assert!(is_bad_version(&info, "v1.4.0"));
    assert!(!is_bad_version(&info, "1.4.1"));
    let none = parse_version_document(&json!({"version": "1.4.1"}), SITE_URL).unwrap();
    assert!(none.bad_versions.is_empty(), "older version files have no list");
}

#[test]
fn an_update_can_be_undone_while_its_backup_is_kept() {
    let install = Install::new("rollback");
    let data = install.0.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let downloaded = install.0.join(".Anix.AppImage.anix-update");
    std::fs::write(&downloaded, b"new version").unwrap();
    let backup = install_verified(&downloaded, &install.target(), &sha256(b"new version")).unwrap();

    assert!(rollback_available(&data, "1.4.0", &install.target()).is_none(), "nothing recorded yet");
    record_update(&data, "1.3.2", "1.4.0", &backup);
    assert!(rollback_available(&data, "1.3.2", &install.target()).is_none(), "only the version that was installed offers it");
    assert_eq!(rollback_available(&data, "1.4.0", &install.target()).unwrap().from, "1.3.2");

    let record = roll_back(&data, "1.4.0", &install.target()).unwrap();
    assert_eq!(record.from, "1.3.2");
    assert_eq!(std::fs::read(install.target()).unwrap(), b"old version", "the previous AppImage is back");
    assert_eq!(install.names(), vec!["Anix.AppImage", "data"], "the backup became the AppImage");
    assert!(!data.join(ROLLBACK_FILE).exists(), "the record is used up");
    assert!(roll_back(&data, "1.4.0", &install.target()).is_err(), "and cannot be used twice");
}

#[test]
fn going_back_needs_the_backup_file() {
    let install = Install::new("rollback-gone");
    let data = install.0.join("data");
    std::fs::create_dir_all(&data).unwrap();
    record_update(&data, "1.3.2", "1.4.0", &backup_path(&install.target()));
    assert!(!data.join(ROLLBACK_FILE).exists(), "no backup, no record");
    std::fs::write(data.join(ROLLBACK_FILE), br#"{"from":"1.3.2","to":"1.4.0","backup":"1:1.1"}"#).unwrap();
    // The user deleted the .bak (or never had one): no offer, and the AppImage stays.
    assert!(rollback_available(&data, "1.4.0", &install.target()).is_none());
    assert!(roll_back(&data, "1.4.0", &install.target()).is_err());
    assert_eq!(std::fs::read(install.target()).unwrap(), b"old version");
    // A broken record is no offer either.
    std::fs::write(data.join(ROLLBACK_FILE), b"{not json").unwrap();
    assert!(rollback_available(&data, "1.4.0", &install.target()).is_none());
}

#[test]
fn a_backup_made_after_the_record_is_not_offered() {
    let install = Install::new("rollback-stale");
    let data = install.0.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let downloaded = install.0.join(".Anix.AppImage.anix-update");
    // A different size from "old version", so the check does not hang on timestamp granularity.
    std::fs::write(&downloaded, b"version 1.4.0").unwrap();
    let backup = install_verified(&downloaded, &install.target(), &sha256(b"version 1.4.0")).unwrap();
    record_update(&data, "1.3.2", "1.4.0", &backup);
    assert!(rollback_available(&data, "1.4.0", &install.target()).is_some());

    // The next update backs up 1.4.0 and then fails before its rename: the .bak now holds the
    // running version, so the old record must not offer "Go back to 1.3.2".
    std::fs::remove_file(&backup).unwrap();
    std::fs::hard_link(install.target(), &backup).unwrap();
    assert!(rollback_available(&data, "1.4.0", &install.target()).is_none());
    assert!(roll_back(&data, "1.4.0", &install.target()).is_err());
    assert_eq!(std::fs::read(install.target()).unwrap(), b"version 1.4.0");
}

#[test]
fn a_release_the_site_marks_as_bad_is_not_offered() {
    let bad = parse_version_document(&json!({"version": "1.5.0", "bad_versions": ["1.5.0"]}), SITE_URL);
    assert!(offered_release(bad).is_none());
    let fine = parse_version_document(&json!({"version": "1.5.1", "bad_versions": ["1.5.0"]}), SITE_URL);
    assert_eq!(offered_release(fine).unwrap().version, "1.5.1");
    assert!(offered_release(None).is_none());
}

#[test]
fn the_bad_version_notice_comes_once_per_version() {
    let install = Install::new("bad-once");
    assert!(first_bad_version_warning(&install.0, "1.4.0"));
    assert!(!first_bad_version_warning(&install.0, "1.4.0"), "not again on the next start");
    assert!(first_bad_version_warning(&install.0, "1.4.1"), "a new listed version warns again");
}
