//! #10: the preferred streaming resolution. It orders the streams of the chosen source (exact,
//! then the closest lower, then the closest higher) and never overrides the language choice.
//! Downloads reuse `downloads::pick_stream`, which already falls back the same way.

mod english_ui;

use anix_lib::downloads::{downloaded_base_name, hls_variant_for, output_base_name, pick_stream};
use anix_lib::error::AppError;
use anix_lib::models::{AnimeQuery, Category, EpisodeItem, ProviderData, SourcesResult, StreamItem};
use anix_lib::providers::languages::{LanguagePrefs, Languages};
use anix_lib::providers::manager::ProviderManager;
use anix_lib::providers::resolution::{order_streams, wanted_height};
use anix_lib::providers::AnimeProvider;
use async_trait::async_trait;
use english_ui::english_ui;
use std::sync::Arc;

fn stream(url: &str, quality: Option<&str>) -> StreamItem {
    StreamItem::new(url.to_string(), "hls".to_string(), quality.map(str::to_string), None, None, true)
}

fn urls(streams: &[StreamItem]) -> Vec<&str> {
    streams.iter().map(|s| s.url.as_str()).collect()
}

#[test]
fn setting_reads_as_a_height() {
    for (setting, height) in [("720", Some(720)), ("720p", Some(720)), ("1080", Some(1080)), ("360p", Some(360)), ("480P", Some(480))] {
        assert_eq!(wanted_height(setting), height, "{setting}");
    }
    // Auto, best, empty and unknown values mean no constraint.
    for setting in ["auto", "best", "", "abc", "240"] {
        assert_eq!(wanted_height(setting), None, "{setting}");
    }
}

#[test]
fn streams_are_ordered_exact_then_lower_then_higher() {
    let mut streams = vec![
        stream("u1080", Some("1080p")),
        stream("u360", Some("360p")),
        stream("u720", Some("720p")),
        stream("unknown", None),
        stream("u480", Some("480p")),
    ];
    order_streams(&mut streams, Some(720));
    // exact 720, then lower (closest first: 480, 360), then higher (1080), unrated last.
    assert_eq!(urls(&streams), ["u720", "u480", "u360", "u1080", "unknown"]);

    let mut streams = vec![stream("u1080", Some("1080p")), stream("u720", Some("720p")), stream("u480", Some("480p"))];
    order_streams(&mut streams, Some(360));
    assert_eq!(urls(&streams), ["u480", "u720", "u1080"], "nothing lower: closest higher first");
}

#[test]
fn no_wish_keeps_the_provider_order() {
    let original = vec![stream("a", Some("360p")), stream("b", Some("1080p")), stream("c", None)];
    let mut streams = original.clone();
    order_streams(&mut streams, None);
    assert_eq!(streams, original);
    let mut streams = original.clone();
    order_streams(&mut streams, wanted_height("auto"));
    assert_eq!(streams, original);
}

#[test]
fn download_falls_back_closest_lower_then_higher() {
    let streams = vec![stream("u1080", Some("1080p")), stream("u480", Some("480p"))];
    let pick = |wanted: &str| pick_stream(&streams, wanted).map(|s| s.url.as_str());
    assert_eq!(pick("720p"), Some("u480"), "720 missing: closest lower");
    assert_eq!(pick("720"), Some("u480"), "bare numbers work too");
    assert_eq!(pick("360p"), Some("u480"), "nothing lower: closest higher, never a failure");
    assert_eq!(pick("best"), Some("u1080"));
}

/// A site with one stream per listed quality, in the language it declares.
struct Site {
    name: &'static str,
    languages: Languages,
    qualities: Vec<&'static str>,
}

#[async_trait]
impl AnimeProvider for Site {
    fn name(&self) -> &'static str {
        self.name
    }
    fn supports_dub(&self) -> bool {
        false
    }
    fn languages(&self) -> Option<Languages> {
        Some(self.languages.clone())
    }
    fn verifies_streams(&self) -> bool {
        true
    }
    async fn get_episodes(&self, _query: &AnimeQuery) -> Result<ProviderData, AppError> {
        let list = vec![EpisodeItem::new(format!("{}|1", self.name), 1.0, None, None, false)];
        Ok(ProviderData { name: self.name.to_string(), dub: Vec::new(), sub: list })
    }
    async fn get_sources(&self, _episode: &EpisodeItem, _category: Category) -> Result<SourcesResult, AppError> {
        let streams = self
            .qualities
            .iter()
            .map(|q| stream(&format!("https://{}.example/{}.m3u8", self.name, q), Some(q)))
            .collect();
        Ok(SourcesResult::new(streams))
    }
}

fn site(name: &'static str, language: &str, qualities: &[&'static str]) -> Arc<dyn AnimeProvider> {
    Arc::new(Site {
        name,
        languages: Languages { sub: vec![language.to_string()], dub: Vec::new() },
        qualities: qualities.to_vec(),
    })
}

async fn first_stream(manager: &ProviderManager) -> (String, String) {
    let found = manager
        .get_sources_for_episode(30, None, 1.0, "sub", Some("Frieren".to_string()), None)
        .await
        .expect("a source");
    (found.provider.clone().unwrap_or_default(), found.streams[0].url.clone())
}

#[tokio::test]
async fn the_source_starts_on_the_wanted_resolution() {
    english_ui();
    let lineup = vec![site("hianime", "en", &["1080p", "720p", "360p"])];
    let manager = ProviderManager::with_providers_and_languages(lineup.clone(), LanguagePrefs::default())
        .with_streaming_resolution("720");
    assert_eq!(first_stream(&manager).await.1, "https://hianime.example/720p.m3u8");

    let manager = ProviderManager::with_providers_and_languages(lineup.clone(), LanguagePrefs::default())
        .with_streaming_resolution("480");
    assert_eq!(first_stream(&manager).await.1, "https://hianime.example/360p.m3u8", "closest lower");

    let auto = ProviderManager::with_providers_and_languages(lineup, LanguagePrefs::default()).with_streaming_resolution("auto");
    assert_eq!(first_stream(&auto).await.1, "https://hianime.example/1080p.m3u8", "auto keeps the provider's order");
}

#[tokio::test]
async fn language_still_comes_before_resolution() {
    english_ui();
    // Spanish is wanted first; its only stream is 1080p. The English site has 720p, but is not used.
    let lineup = vec![site("hianime", "en", &["720p"]), site("latanime", "es", &["1080p"])];
    let prefs = LanguagePrefs { sub: vec!["es".into(), "en".into()], dub: vec!["en".into()], ..LanguagePrefs::default() };
    let manager = ProviderManager::with_providers_and_languages(lineup, prefs).with_streaming_resolution("720");
    let (provider, url) = first_stream(&manager).await;
    assert_eq!(provider, "latanime");
    assert_eq!(url, "https://latanime.example/1080p.m3u8");
}

#[test]
fn a_master_playlist_download_takes_the_wanted_variant() {
    let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nv1080.m3u8\n\
                  #EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720\nv720.m3u8\n\
                  #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=854x480\nv480.m3u8\n";
    let pick = |text: &str, wanted: Option<u32>| hls_variant_for(text, "https://cdn.example/m.m3u8", wanted).map(|u| u.rsplit('/').next().unwrap().to_string());
    assert_eq!(pick(master, Some(720)).as_deref(), Some("v720.m3u8"));
    assert_eq!(pick(master, Some(600)).as_deref(), Some("v480.m3u8"), "closest lower");
    assert_eq!(pick(master, None).as_deref(), Some("v1080.m3u8"), "best: no wish");
    let only_high = "#EXTM3U\n#EXT-X-STREAM-INF:RESOLUTION=1280x720\nv720.m3u8\n#EXT-X-STREAM-INF:RESOLUTION=854x480\nv480.m3u8\n";
    assert_eq!(pick(only_high, Some(360)).as_deref(), Some("v480.m3u8"), "nothing lower: closest higher");
    let no_resolution = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\na.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=9\nb.m3u8\n";
    assert_eq!(pick(no_resolution, Some(720)).as_deref(), Some("b.m3u8"), "no RESOLUTION: best");
}

#[test]
fn equal_variants_keep_the_first_and_the_file_is_named_by_the_height_got() {
    let tie = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5,RESOLUTION=1280x720\nfirst.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5,RESOLUTION=1280x720\nsecond.m3u8\n";
    for wanted in [Some(720), None] {
        assert_eq!(hls_variant_for(tie, "https://cdn.example/m.m3u8", wanted).as_deref(), Some("https://cdn.example/first.m3u8"), "{wanted:?}");
    }
    // 720p asked, 480p downloaded: named 480p; an exact match or an unknown height keeps the asked name.
    let name = |asked: &str, got: Option<u32>| downloaded_base_name("Show", 1.0, "sub", asked, got);
    assert_eq!(name("720p", Some(480)), output_base_name("Show", 1.0, "sub", "480p"));
    assert_eq!(name("720p", Some(720)), output_base_name("Show", 1.0, "sub", "720p"));
    assert_eq!(name("720p", None), output_base_name("Show", 1.0, "sub", "720p"));
    assert_eq!(name("best", Some(1080)), output_base_name("Show", 1.0, "sub", "1080p"));
}
