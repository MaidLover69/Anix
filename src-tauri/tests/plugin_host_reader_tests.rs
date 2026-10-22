//! Video-host readers of the first-party extensions, run in the app's plugin engine against a
//! loopback copy of the host's answer.

mod plugin_support;

use anix_lib::web_session::SessionUse;
use plugin_support::{runtime, Req, Resp, Site};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

fn extension(id: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/src").join(id).join("provider.js");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e))
}

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[tokio::test]
async fn latanime_reads_filemoon_through_its_byse_api() {
    let answer = fixture("byse_video.json");
    let site = Site::start(move |request: &Req| match request.path.as_str() {
        "/api/videos/abc123" => Resp::json(answer.clone()),
        _ => Resp::status(404),
    })
    .await;
    let plugin = runtime("latanime", &extension("latanime"));
    let call = |method: &'static str, args: Vec<Value>| {
        let plugin = plugin.clone();
        async move { plugin.call(method, &args, Duration::from_secs(10), SessionUse::Existing).await }
    };

    assert_eq!(call("_serverOf", vec![json!("https://filemoon.sx/e/abc123")]).await.unwrap(), json!("Filemoon"));
    assert_eq!(call("_serverOf", vec![json!("https://bysezoxexe.com/e/abc123")]).await.unwrap(), json!("Filemoon"));
    assert_eq!(call("_serverOf", vec![json!("https://doodstream.com/e/abc123")]).await.unwrap(), json!(""));

    let video = call("_byse", vec![json!(site.url("/e/abc123"))]).await.expect("the Filemoon video");
    assert_eq!(video["url"], "https://cdn.example/v/master.m3u8");
    assert_eq!(video["type"], "m3u8");
    assert_eq!(video["headers"]["Referer"], format!("{}/", site.base()));

    let missing = call("_byse", vec![json!(site.url("/e/gone99"))]).await.unwrap_err();
    assert!(missing.reason().contains("not found on") || missing.reason().contains("Filemoon"), "{}", missing.reason());
}
