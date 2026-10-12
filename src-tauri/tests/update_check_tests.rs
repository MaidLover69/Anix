//! The update check against the website's `version.json`, with a loopback server as the site.

mod plugin_support;

use anix_lib::clients::update::{
    default_download_url, fetch_update_info, parse_version_document, version_document_url, UpdateInfo, SITE_URL,
};
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
