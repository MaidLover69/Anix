//! The update check against the website's `version.json`, with a loopback server as the site.

mod plugin_support;

use anix_lib::clients::update::{
    appimage_target, backup_path, default_download_url, download_and_install, fetch_update_info, install_verified,
    is_newer_version, parse_version_document, sha256_file, version_document_url, AppImageAsset, UpdateInfo, SITE_URL,
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
}
