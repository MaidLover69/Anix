use anix_lib::cache::L1Cache;
use anix_lib::clients::anilist::{interpret_graphql_response, parse_media_hint, parse_media_json};
use anix_lib::clients::aniskip::{skip_times_from_response, AniSkipResponse};
use anix_lib::clients::app_settings::{DohProvider, NetworkSettings};
use anix_lib::clients::http::parse_doh_answer;
use anix_lib::clients::imdb::{imdb_season_for_title, parse_season_page, season_matches_expected};
use anix_lib::clients::jikan::retry_delay;
use anix_lib::commands::providers::{episode_key, released_episode_cap, ProviderEpisodeMap};
use anix_lib::error::AppError;
use anix_lib::models::{CoverImage, Media, MediaTitle, ProviderEpisodes, SourcesResult, StreamItem};
use serde_json::json;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_l1_cache() {
    let cache = L1Cache::<String, String>::new(10, 60);
    cache.insert("key1".to_string(), "val1".to_string()).await;
    assert_eq!(cache.get(&"key1".to_string()).await, Some("val1".to_string()));
    assert_eq!(cache.get(&"unknown".to_string()).await, None);
}

#[tokio::test]
async fn l1_cache_loads_once_and_does_not_cache_errors() {
    let cache = Arc::new(L1Cache::<u32, String>::new(10, 60));
    let loads = Arc::new(AtomicUsize::new(0));

    let mut callers = Vec::new();
    for _ in 0..8 {
        let (cache, loads) = (cache.clone(), loads.clone());
        callers.push(tokio::spawn(async move {
            cache
                .try_get_with(7, async {
                    loads.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok::<_, String>("loaded".to_string())
                })
                .await
        }));
    }
    for caller in callers {
        assert_eq!(caller.await.unwrap().unwrap(), "loaded");
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1, "concurrent callers must share one load");

    let failed = cache.try_get_with(8, async { Err::<String, _>("boom".to_string()) }).await;
    assert_eq!(*failed.unwrap_err(), "boom");
    assert_eq!(cache.get(&8).await, None, "an error must not be cached");
}

#[test]
fn test_media_model_serialization() {
    let media = Media {
        id: 1,
        id_mal: Some(10),
        title: MediaTitle {
            romaji: Some("Cowboy Bebop".to_string()),
            english: Some("Cowboy Bebop".to_string()),
            native: None,
            user_preferred: Some("Cowboy Bebop".to_string()),
        },
        cover_image: CoverImage {
            extra_large: Some("https://example.com/cover.jpg".to_string()),
            large: None,
            color: None,
        },
        banner_image: None,
        description: Some("Space western".to_string()),
        format: Some("TV".to_string()),
        status: Some("FINISHED".to_string()),
        episodes: Some(26),
        duration: Some(24),
        season: Some("SPRING".to_string()),
        season_year: Some(1998),
        average_score: Some(89),
        mean_score: Some(89),
        popularity: Some(100000),
        favourites: Some(50000),
        genres: vec!["Action".to_string(), "Sci-Fi".to_string()],
        is_adult: false,
        ..Default::default()
    };

    let json_str = serde_json::to_string(&media).expect("serialize media");
    let deserialized: Media = serde_json::from_str(&json_str).expect("deserialize media");
    assert_eq!(deserialized.id, 1);
    assert_eq!(deserialized.title.english, Some("Cowboy Bebop".to_string()));
    assert_eq!(deserialized.display_title(), "Cowboy Bebop");
}

#[test]
fn test_stream_item_and_sources() {
    let stream = StreamItem {
        url: "https://example.com/master.m3u8".to_string(),
        stream_type: "hls".to_string(),
        quality: Some("1080p".to_string()),
        audio: Some("sub".to_string()),
        subtitle_variant: None,
        referer: Some("https://example.com/".to_string()),
        is_active: true,
        origin: Some("https://example.com".to_string()),
        headers: None,
    };

    let sources = SourcesResult {
        streams: vec![stream.clone()],
        subtitles: vec![],
        skip: None,
        provider: Some("kaa".to_string()),
        language: None,
    };
    assert_eq!(sources.streams.len(), 1);
    assert_eq!(sources.streams[0].url, "https://example.com/master.m3u8");

    // The serving provider travels with the result; older payloads without it still load.
    let value = serde_json::to_value(&sources).unwrap();
    assert_eq!(value["provider"], "kaa");
    let legacy: SourcesResult = serde_json::from_value(json!({ "streams": [] })).unwrap();
    assert_eq!(legacy.provider, None);
}

// ---------------------------------------------------------------- AniList

#[test]
fn anilist_media_carries_no_invented_mal_or_imdb_values() {
    let media = parse_media_json(&json!({
        "id": 21,
        "idMal": 21,
        "title": { "romaji": "One Piece", "english": "One Piece" },
        "coverImage": { "large": "https://img.example/l.jpg" },
        "description": "Pirates.",
        "averageScore": 88,
        "isAdult": false,
        "genres": ["Action"]
    }))
    .unwrap();
    assert_eq!(media.average_score, Some(88));
    assert_eq!(media.description.as_deref(), Some("Pirates."));
    assert_eq!(media.score_mal, None);
    assert_eq!(media.rating_imdb, None);
    assert_eq!(media.synopsis_mal, None);
    assert_eq!(media.synopsis_imdb, None);

    assert!(parse_media_json(&json!({ "title": {} })).is_err());
}

#[test]
fn anilist_status_and_graphql_errors_are_reported_as_what_they_are() {
    let ok = interpret_graphql_response(200, None, r#"{"data":{"Page":{"media":[]}}}"#).unwrap();
    assert!(ok["Page"]["media"].is_array());

    assert!(matches!(
        interpret_graphql_response(429, Some(42), r#"{"errors":[{"message":"Too Many Requests.","status":429}],"data":null}"#),
        Err(AppError::RateLimited { retry_after_s: 42 })
    ));
    assert!(matches!(
        interpret_graphql_response(429, None, "rate limited"),
        Err(AppError::RateLimited { retry_after_s: 60 })
    ));
    // Some rate-limit answers arrive with a 200 status and the code inside the body.
    assert!(matches!(
        interpret_graphql_response(200, None, r#"{"errors":[{"message":"Too Many Requests.","status":429}],"data":null}"#),
        Err(AppError::RateLimited { .. })
    ));

    match interpret_graphql_response(500, None, r#"{"errors":[{"message":"Internal Server Error","status":500}]}"#) {
        Err(AppError::Network(message)) => assert_eq!(message, "AniList HTTP 500: Internal Server Error"),
        other => panic!("unexpected: {:?}", other),
    }
    match interpret_graphql_response(503, None, "<html>maintenance</html>") {
        Err(AppError::Network(message)) => assert_eq!(message, "AniList HTTP 503"),
        other => panic!("unexpected: {:?}", other),
    }
    match interpret_graphql_response(200, None, r#"{"errors":[{"message":"Syntax Error","status":400}],"data":null}"#) {
        Err(AppError::Network(message)) => assert_eq!(message, "AniList error: Syntax Error"),
        other => panic!("unexpected: {:?}", other),
    }

    assert!(matches!(
        interpret_graphql_response(400, None, r#"{"errors":[{"message":"Invalid token","status":400}],"data":null}"#),
        Err(AppError::AuthRequired)
    ));
    assert!(matches!(interpret_graphql_response(401, None, ""), Err(AppError::AuthRequired)));

    // A missing record is a 404 whose data names the null field; the caller reports it.
    let missing = interpret_graphql_response(404, None, r#"{"errors":[{"message":"Not Found.","status":404}],"data":{"Media":null}}"#).unwrap();
    assert!(missing["Media"].is_null());
}

#[test]
fn anilist_hint_lists_titles_year_and_episode_count() {
    let hint = parse_media_hint(&json!({
        "idMal": 52991,
        "title": { "romaji": "Sousou no Frieren", "english": "Frieren: Beyond Journey's End" },
        "synonyms": ["Frieren at the Funeral", "sousou no frieren", ""],
        "seasonYear": null,
        "startDate": { "year": 2023 },
        "episodes": 28,
        "format": "TV"
    }));
    assert_eq!(hint.id_mal, Some(52991));
    assert_eq!(
        hint.titles,
        vec!["Sousou no Frieren", "Frieren: Beyond Journey's End", "Frieren at the Funeral"]
    );
    assert_eq!(hint.year, Some(2023));
    assert_eq!(hint.episodes, Some(28));
    assert_eq!(hint.format.as_deref(), Some("TV"));
}

// ---------------------------------------------------------------- DNS over HTTPS and settings

fn ips(list: &[&str]) -> Vec<IpAddr> {
    list.iter().map(|ip| ip.parse().unwrap()).collect()
}

#[test]
fn doh_json_answers_are_parsed() {
    // Cloudflare / Google shape.
    let google = json!({
        "Status": 0, "TC": false, "RD": true, "RA": true,
        "Question": [{ "name": "www.miruro.to.", "type": 1 }],
        "Answer": [
            { "name": "www.miruro.to.", "type": 1, "TTL": 300, "data": "172.67.215.104" },
            { "name": "www.miruro.to.", "type": 1, "TTL": 218, "data": "104.21.61.215" }
        ]
    });
    let records = parse_doh_answer(&google).unwrap();
    assert_eq!(records.addrs, ips(&["172.67.215.104", "104.21.61.215"]));
    assert_eq!(records.ttl_s, Some(218), "the smallest TTL bounds the cache entry");

    // AdGuard shape, with a CNAME hop before the addresses and an AAAA answer.
    let adguard = json!({
        "Question": [{ "name": "example.org.", "type": 28 }],
        "Answer": [
            { "name": "example.org.", "data": "edge.example.net.", "TTL": 60, "type": 5, "class": 1 },
            { "name": "edge.example.net.", "data": "2606:4700:3034::6815:3dd7", "TTL": 120, "type": 28, "class": 1 },
            { "name": "edge.example.net.", "data": "2606:4700:3034::6815:3dd7", "TTL": 120, "type": 28, "class": 1 }
        ],
        "Extra": null, "Status": 0
    });
    let records = parse_doh_answer(&adguard).unwrap();
    assert_eq!(records.addrs, ips(&["2606:4700:3034::6815:3dd7"]), "CNAME rows and duplicates are dropped");
    assert_eq!(records.ttl_s, Some(120));

    // NXDOMAIN and an answer-less response are empty, not errors; SERVFAIL is an error.
    assert!(parse_doh_answer(&json!({ "Status": 3 })).unwrap().addrs.is_empty());
    assert!(parse_doh_answer(&json!({ "Status": 0, "Question": [] })).unwrap().addrs.is_empty());
    assert!(parse_doh_answer(&json!({ "Status": 2 })).is_err());
    assert!(parse_doh_answer(&json!({ "Status": 0, "Answer": [{ "type": 1, "data": "not-an-ip" }] }))
        .unwrap()
        .addrs
        .is_empty());
}

#[test]
fn network_settings_default_on_and_follow_the_file() {
    let defaults = NetworkSettings::from_json(&json!({}));
    assert!(defaults.enable_doh);
    assert!(defaults.hide_adult_content);
    assert_eq!(defaults.doh_provider, DohProvider::Cloudflare);

    let custom = NetworkSettings::from_json(&json!({
        "enable_doh": false,
        "doh_provider": "AdGuard",
        "hide_adult_content": false
    }));
    assert!(!custom.enable_doh);
    assert!(!custom.hide_adult_content);
    assert_eq!(custom.doh_provider, DohProvider::AdGuard);

    assert_eq!(DohProvider::from_setting("google"), DohProvider::Google);
    assert_eq!(DohProvider::from_setting("something else"), DohProvider::Cloudflare);
    // A value of the wrong type keeps the default instead of switching the feature off.
    assert!(NetworkSettings::from_json(&json!({ "enable_doh": "yes" })).enable_doh);
}

// ---------------------------------------------------------------- AniSkip, Jikan, IMDb

#[test]
fn aniskip_v2_response_is_read_with_its_camel_case_fields() {
    // Trimmed copy of a real api.aniskip.com/v2 answer.
    let body = r#"{"found":true,"results":[{"interval":{"startTime":310.571,"endTime":400.571},"skipType":"op","skipId":"d4f34b6d","episodeLength":1443.984},{"interval":{"startTime":1396.006,"endTime":1434},"skipType":"ed","skipId":"5751c57a","episodeLength":1434.985}],"message":"Successfully found skip times","statusCode":200}"#;
    let parsed: AniSkipResponse = serde_json::from_str(body).expect("AniSkip response");
    let times = skip_times_from_response(parsed);
    assert_eq!(times.intro_start, Some(310.571));
    assert_eq!(times.intro_end, Some(400.571));
    assert_eq!(times.outro_start, Some(1396.006));
    assert_eq!(times.outro_end, Some(1434.0));

    let none: AniSkipResponse = serde_json::from_str(r#"{"found":false,"results":[]}"#).unwrap();
    assert_eq!(skip_times_from_response(none).intro_start, None);
}

#[test]
fn jikan_rate_limit_waits_are_bounded() {
    assert_eq!(retry_delay(0, None), Duration::from_secs(1));
    assert_eq!(retry_delay(2, None), Duration::from_secs(3));
    assert_eq!(retry_delay(0, Some(2)), Duration::from_secs(2));
    assert_eq!(retry_delay(0, Some(0)), Duration::from_secs(1));
    assert_eq!(retry_delay(0, Some(600)), Duration::from_secs(4), "never wait longer than the cap");
    assert_eq!(retry_delay(30, None), Duration::from_secs(4));
}

#[test]
fn imdb_season_is_only_assumed_when_the_title_allows_it() {
    assert_eq!(imdb_season_for_title("Sousou no Frieren"), Some(("Sousou no Frieren".to_string(), 1)));
    assert_eq!(
        imdb_season_for_title("Shingeki no Kyojin Season 2"),
        Some(("Shingeki no Kyojin".to_string(), 2))
    );
    assert_eq!(
        imdb_season_for_title("Boku no Hero Academia 3rd Season"),
        Some(("Boku no Hero Academia".to_string(), 3))
    );
    for unclear in [
        "Shingeki no Kyojin: The Final Season",
        "Mob Psycho 100 II",
        "Shingeki no Kyojin Season 3 Part 2",
        "Dr. Stone: New World Part 2",
        "One Piece Film: Red",
        "Kizumonogatari Movie",
        "Gintama 2",
        "",
    ] {
        assert_eq!(imdb_season_for_title(unclear), None, "{:?} has no knowable IMDb season", unclear);
    }

    assert!(season_matches_expected(12, Some(12)));
    assert!(season_matches_expected(13, Some(12)));
    assert!(!season_matches_expected(26, Some(12)));
    assert!(!season_matches_expected(12, None), "an unknown episode count confirms nothing");
    assert!(!season_matches_expected(0, Some(0)));
}

#[test]
fn imdb_season_page_fixture() {
    let next_data = json!({ "props": { "pageProps": { "contentData": { "section": { "episodes": { "items": [
        { "episode": "1", "plot": "The journey begins." },
        { "episode": 2, "plot": "  " },
        { "episode": "3", "plot": "A third plot." }
    ]}}}}}});
    let html = format!(
        "<html><script id=\"__NEXT_DATA__\" type=\"application/json\">{}</script></html>",
        next_data
    );
    let season = parse_season_page(&html);
    assert_eq!(season.episode_count, 3);
    assert_eq!(season.synopses.len(), 2);
    assert_eq!(season.synopses[&1], "The journey begins.");
    assert_eq!(season.synopses[&3], "A third plot.");

    let empty = parse_season_page("<html>blocked</html>");
    assert_eq!(empty.episode_count, 0);
    assert!(empty.synopses.is_empty());
}

// ---------------------------------------------------------------- episode commands

#[test]
fn fractional_episodes_get_their_own_keys() {
    assert_ne!(episode_key(12.0), episode_key(12.5));
    assert_eq!(episode_key(12.0), 1200);
    assert_eq!(episode_key(12.5), 1250);
    assert_eq!(episode_key(0.1 + 0.2), episode_key(0.3));
}

#[test]
fn unaired_shows_have_no_released_episodes() {
    // Next to air is episode 1: nothing has aired, so nothing may be invented.
    assert_eq!(released_episode_cap(Some("RELEASING"), Some(1)), Some(0));
    assert_eq!(released_episode_cap(Some("NOT_YET_RELEASED"), None), Some(0));
    assert_eq!(released_episode_cap(Some("not_yet_released"), None), Some(0));
    assert_eq!(released_episode_cap(Some("RELEASING"), Some(8)), Some(7));
    assert_eq!(released_episode_cap(Some("RELEASING"), Some(0)), Some(0));
    // Without a schedule the count cannot be derived from the status alone.
    assert_eq!(released_episode_cap(Some("RELEASING"), None), None);
    assert_eq!(released_episode_cap(Some("FINISHED"), None), None);
    assert_eq!(released_episode_cap(None, None), None);
}

#[test]
fn provider_map_serialises_in_provider_order() {
    let map = ProviderEpisodeMap(vec![
        ("zeta".to_string(), ProviderEpisodes::default()),
        ("alpha".to_string(), ProviderEpisodes::default()),
        ("Mid".to_string(), ProviderEpisodes::default()),
    ]);
    let text = serde_json::to_string(&map).unwrap();
    let positions: Vec<usize> = ["\"zeta\"", "\"alpha\"", "\"Mid\""]
        .iter()
        .map(|key| text.find(key).expect("key present"))
        .collect();
    assert!(positions[0] < positions[1] && positions[1] < positions[2], "{}", text);
    assert!(text.starts_with("{\"zeta\":{\"sub\":[],\"dub\":[]}"));
}

// ---------------------------------------------------------------- unrelated models kept from before

#[test]
fn test_anime_themes_model() {
    use anix_lib::clients::animethemes::AnimeThemeEntry;
    let theme = AnimeThemeEntry {
        theme_type: "OP".to_string(),
        sequence: Some(1),
        slug: "OP1".to_string(),
        song_title: Some("Yuusha".to_string()),
        artist_name: Some("YOASOBI".to_string()),
        episodes: Some("1-16".to_string()),
        video_url: Some("https://v.animethemes.moe/SousouNoFrieren-OP1.webm".to_string()),
        audio_url: Some("https://a.animethemes.moe/SousouNoFrieren-OP1.ogg".to_string()),
        resolution: Some(1080),
    };
    assert_eq!(theme.slug, "OP1");
    assert_eq!(theme.resolution, Some(1080));
}

#[test]
fn test_local_anime_file_model() {
    use anix_lib::commands::local_scanner::LocalAnimeFile;
    let file = LocalAnimeFile {
        file_path: "/videos/Frieren - 01.mkv".to_string(),
        file_name: "Frieren - 01.mkv".to_string(),
        file_size: 1024 * 1024 * 500,
        parsed_title: "Frieren".to_string(),
        parsed_episode: Some(1.0),
    };
    assert_eq!(file.parsed_title, "Frieren");
    assert_eq!(file.parsed_episode, Some(1.0));
}

#[test]
fn errors_map_to_stable_short_codes() {
    use anix_lib::error::code_for_message;
    let cases = [
        (AppError::Network("graphql.anilist.co timed out".into()), "CAT-TIMEOUT"),
        (AppError::Network("AniList answered HTTP 403".into()), "CAT-403"),
        (AppError::Network("cdn.example: HTTP status 503".into()), "NET-503"),
        (AppError::Network("cdn.example: connection reset".into()), "NET-ERR"),
        (AppError::Unreachable("graphql.anilist.co: dns error: failed to lookup address".into()), "CAT-DNS"),
        (AppError::Unreachable("cdn.example: invalid peer certificate".into()), "NET-TLS"),
        (AppError::Unreachable("cdn.example: Connection refused".into()), "NET-CONNECT"),
        (AppError::Database("locked".into()), "DB-ERR"),
        (AppError::Parse("kitsu.io: expected value".into()), "CAT-PARSE"),
        (AppError::provider("hianime", "HTTP 404"), "SRC-404"),
        (AppError::CaptchaRequired, "SRC-CAPTCHA"),
        (AppError::VerificationRequired { site: "example".into(), origin: "https://example.org".into() }, "SRC-VERIFY"),
        (AppError::NoSourcesFound, "SRC-NONE"),
        (AppError::RateLimited { retry_after_s: 30 }, "NET-429"),
        (AppError::AuthRequired, "AUTH-LOGIN"),
        (AppError::Other("disk full".into()), "APP-ERR"),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{:?}", error);
        // The same code comes back from the text the frontend receives.
        assert_eq!(code_for_message(&error.to_string()), code, "{}", error);
    }
    assert_eq!(AppError::SourcesUnavailable("hianime: no stream".into()).code(), "SRC-UNAVAILABLE");
    // Plain failure messages (downloads, updates) still get a code from what they name.
    assert_eq!(code_for_message("The stream host answered HTTP 403 Forbidden."), "NET-403");
    assert_eq!(code_for_message("The stream host did not answer in time."), "APP-ERR");
    assert_eq!(code_for_message("Could not reach AniList: request timed out"), "CAT-TIMEOUT");
}

#[test]
fn gnutls_priority_is_set_only_for_an_appimage_without_a_policy_or_user_value() {
    use anix_lib::system::gnutls::needs_priority;
    use std::ffi::OsStr;
    let appimage = Some(OsStr::new("/home/u/Anix.AppImage"));
    assert!(needs_priority(appimage, false, None));
    assert!(!needs_priority(appimage, true, None), "Fedora ships the policy file");
    assert!(!needs_priority(appimage, false, Some(OsStr::new("PERFORMANCE"))), "a user value wins");
    assert!(!needs_priority(None, false, None), "not an AppImage");
    assert!(!needs_priority(Some(OsStr::new("")), false, None));
}

#[test]
fn the_diagnostics_report_leaves_out_private_data() {
    use anix_lib::diagnostics::{redact, report_settings};
    let text = "\
Download failed: https://cdn.example/v/ep3.m3u8?token=abc123&expires=99#t=5
Logged in as someone.name+tag@example.co.uk
Authorization: Bearer abc.def-ghi
settings: {\"anilist_token\": \"s3cr3t-value\", \"session=xyz\"}
jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl here
key 0123456789abcdef0123456789abcdef01 end
Saved to /home/someone/Videos/Anime/Show.mkv and /run/media/someone/Disk/a.mkv
A folder at /media/other/stuff and file:///home/someone/x
Kept: https://anilist.co/anime/21 and a 16-char tag 0123456789abcdef";
    let out = redact(text, Some("/home/someone"));

    for gone in ["abc123", "expires", "someone.name", "example.co.uk", "abc.def-ghi", "s3cr3t-value", "xyz", "eyJhbGci", "0123456789abcdef0123456789abcdef01", "someone", "other"] {
        assert!(!out.contains(gone), "{:?} is still in:\n{}", gone, out);
    }
    assert!(out.contains("https://cdn.example/v/ep3.m3u8?<removed>"), "{}", out);
    assert!(out.contains("<email>"));
    assert!(out.contains("~/Videos/Anime/Show.mkv"), "{}", out);
    assert!(out.contains("/run/media/<user>/Disk/a.mkv"), "{}", out);
    assert!(out.contains("/media/<user>/stuff"), "{}", out);
    assert!(out.contains("https://anilist.co/anime/21"), "links without a query stay: {}", out);
    assert!(out.contains("0123456789abcdef"), "short ids stay readable: {}", out);

    let settings = serde_json::json!({
        "theme": "dark", "download_dir": "/home/someone/Anime", "anilist_token": "t", "mal_refresh_token": "r",
        "comick_session": "c", "plugin_password": "p",
    });
    assert_eq!(
        report_settings(&settings),
        serde_json::json!({ "theme": "dark", "download_dir": "/home/someone/Anime" })
    );
}
