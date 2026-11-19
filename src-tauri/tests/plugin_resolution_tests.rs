//! A plugin's streams are put in resolution order before they are checked, so the one that
//! passes first is the wanted one (the check puts the first passing stream ahead of the rest).

mod english_ui;
mod plugin_support;

use anix_lib::plugins::registry::{PluginRegistry, RegistryOptions};
use anix_lib::providers::manager::ProviderManager;
use english_ui::english_ui;
use plugin_support::{NoSession, Req, Resp, Site};
use serde_json::json;
use std::sync::Arc;

const PLUGIN: &str = r#"
class Provider {
  getSettings() { return { episodeServers: ['main'], supportsDub: false }; }
  async search(opts) { return [{ id: 'show', title: opts.query, url: '', subOrDub: 'sub' }]; }
  async findEpisodes(id) { return [{ id: 'ep', number: 1, url: '' }]; }
  async findEpisodeServer(episode, server) {
    const media = $getUserPreference('media');
    return { server, headers: {}, videoSources: [
      { url: media + '/1080.mp4', type: 'mp4', quality: '1080p', subtitles: [] },
      { url: media + '/720.mp4', type: 'mp4', quality: '720p', subtitles: [] },
    ] };
  }
}
"#;

#[tokio::test]
async fn a_plugins_streams_are_ordered_by_resolution_before_they_are_checked() {
    english_ui();
    let media = Site::start(|_: &Req| Resp::new(206, "video/mp4", b"\0\0\0\x20ftypisom".to_vec()).with_header("Content-Range", "bytes 0-11/1000")).await;
    let media_base = media.base();
    let index = Site::start(move |request: &Req| match request.route() {
        "/index.json" => Resp::json(json!([{
            "id": "res", "name": "Res Plugin", "version": "1", "type": "onlinestream-provider",
            "language": "javascript", "payload": PLUGIN,
            "userConfig": { "version": 1, "fields": [{ "type": "text", "name": "media", "label": "Streams", "default": media_base }] }
        }])),
        _ => Resp::status(404),
    })
    .await;
    let dir = std::env::temp_dir().join(format!("anix-resolution-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let registry = PluginRegistry::open(
        dir,
        RegistryOptions { allow_insecure: true, allow_private_network: true, session: Arc::new(NoSession), ..RegistryOptions::default() },
    );
    registry.add_repo(&index.url("/index.json")).await.unwrap();

    let reg = registry.clone();
    let manager = ProviderManager::with_source_offline(Arc::new(move || reg.providers())).with_streaming_resolution("720");
    let found = manager
        .get_sources_for_episode(9, None, 1.0, "sub", Some("Res Show".to_string()), None)
        .await
        .expect("a source");
    assert!(found.streams[0].url.ends_with("/720.mp4"), "{:?}", found.streams.iter().map(|s| &s.url).collect::<Vec<_>>());

    // Without a wish the plugin's own order (1080p first) stands.
    let reg = registry.clone();
    let auto = ProviderManager::with_source_offline(Arc::new(move || reg.providers())).with_streaming_resolution("auto");
    let found = auto.get_sources_for_episode(9, None, 1.0, "sub", Some("Res Show".to_string()), None).await.expect("a source");
    assert!(found.streams[0].url.ends_with("/1080.mp4"));
}
