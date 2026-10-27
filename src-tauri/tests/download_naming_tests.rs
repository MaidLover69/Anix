//! Pure-function tests for the download module: file names, request headers, the ffmpeg
//! argument list, stream and subtitle recognition. Nothing here runs ffmpeg or uses the
//! network.

use anix_lib::downloads::{
    build_ffmpeg_args, configured_download_dir, detect_subtitle_format, output_base_name,
    parse_hls_demuxer_help, pick_stream, preferred_provider, remove_stale_part_files,
    request_headers, sanitize_filename, remember_work_dir, remove_part_files_in_old_dirs, hls_source_stamp, truncate_to_bytes, validate_stream_url, work_tag,
    best_hls_variant, check_writable_dir, delete_finished_files, download_languages, hls_attribute, parse_content_range,
    plan_hls_download, DownloadSource, FfmpegInput, HlsDemuxerOptions, RequestHeaders, SubtitleFormat, SubtitleInput,
    BROWSER_USER_AGENT, MAX_FILE_NAME_BYTES, PART_SUFFIX,
};
use anix_lib::models::StreamItem;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn strings(args: &[std::ffi::OsString]) -> Vec<String> {
    args.iter().map(|a| a.to_string_lossy().to_string()).collect()
}

/// Index of `flag` in `args`; panics with the whole list when it is missing.
fn position(args: &[String], flag: &str) -> usize {
    args.iter()
        .position(|a| a == flag)
        .unwrap_or_else(|| panic!("{} not in {:?}", flag, args))
}

// ---------------------------------------------------------------------------------------
// File names
// ---------------------------------------------------------------------------------------

#[test]
fn base_name_carries_episode_category_and_quality() {
    assert_eq!(output_base_name("Frieren", 5.0, "sub", "1080p"), "Frieren - EP 05 [SUB 1080p]");
    assert_eq!(output_base_name("Frieren", 12.5, "dub", "720p"), "Frieren - EP 12.5 [DUB 720p]");
    assert_eq!(output_base_name("Frieren", 105.0, "sub", ""), "Frieren - EP 105 [SUB]");
    assert_eq!(output_base_name("", 1.0, "", ""), "Untitled - EP 01");
}

#[test]
fn sub_dub_and_qualities_of_one_episode_do_not_share_a_name() {
    let names = [
        output_base_name("Frieren", 5.0, "sub", "1080p"),
        output_base_name("Frieren", 5.0, "dub", "1080p"),
        output_base_name("Frieren", 5.0, "sub", "720p"),
        output_base_name("Frieren", 5.0, "dub", "720p"),
        output_base_name("Frieren", 6.0, "sub", "1080p"),
    ];
    let unique: std::collections::HashSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "{:?}", names);
}

#[test]
fn names_are_safe_path_components() {
    assert_eq!(sanitize_filename("Re:Zero / Part 2?"), "Re_Zero _ Part 2_");
    assert_eq!(sanitize_filename("a\\b|c<d>e\"f*g"), "a_b_c_d_e_f_g");
    assert_eq!(sanitize_filename("line\nbreak\ttab"), "line_break_tab");
    assert_eq!(sanitize_filename("..").as_str(), "");
    assert_eq!(sanitize_filename(".hidden"), "hidden");
    assert_eq!(sanitize_filename("  spaced  "), "spaced");

    let name = output_base_name("../../etc/passwd", 1.0, "sub/../x", "10:80p");
    assert!(!name.contains('/') && !name.contains(':'), "{}", name);
    assert!(!name.starts_with('.'), "{}", name);
    assert_eq!(Path::new(&name).components().count(), 1);
}

#[test]
fn truncation_never_splits_a_character() {
    assert_eq!(truncate_to_bytes("hello", 10), "hello");
    assert_eq!(truncate_to_bytes("hello", 3), "hel");
    // Each of these characters is three bytes in UTF-8.
    let kanji = "葬送のフリーレン";
    assert_eq!(truncate_to_bytes(kanji, 3), "葬");
    assert_eq!(truncate_to_bytes(kanji, 4), "葬");
    assert_eq!(truncate_to_bytes(kanji, 5), "葬");
    assert_eq!(truncate_to_bytes(kanji, 6), "葬送");
    assert_eq!(truncate_to_bytes(kanji, 0), "");
    assert_eq!(truncate_to_bytes("a😀b", 2), "a");
}

#[test]
fn long_titles_fit_in_a_file_name_with_every_suffix() {
    let titles = [
        "A".repeat(1000),
        "葬送のフリーレン".repeat(60),
        "😀".repeat(300),
        format!("{}é", "x".repeat(155)),
    ];

    for title in titles {
        let base = output_base_name(&title, 1024.5, "sub", "1080p");
        assert!(base.len() <= 180, "base is {} bytes", base.len());
        assert!(base.ends_with(" - EP 1024.5 [SUB 1080p]"), "the episode part must survive: {}", base);
        assert!(std::str::from_utf8(base.as_bytes()).is_ok());

        // The longest names the downloader derives from the base.
        let tag = work_tag("some-download-id");
        let longest = [
            format!("{} (999).mkv", base),
            format!("{}.{}.download{}", base, tag, PART_SUFFIX),
            format!("{}.{}.sub12{}", base, tag, PART_SUFFIX),
            format!("{} (999).{}-12.vtt", base, "l".repeat(24)),
        ];
        for name in longest {
            assert!(name.len() <= MAX_FILE_NAME_BYTES, "{} bytes: {}", name.len(), name);
        }
    }

    // Oversized category and quality values are cut down too.
    let base = output_base_name("Title", 1.0, &"c".repeat(500), &"q".repeat(500));
    assert!(base.len() <= 180);
    assert!(base.starts_with("Title - EP 01 ["));
}

#[test]
fn work_tag_separates_downloads_and_is_stable() {
    assert_eq!(work_tag("dl-1"), work_tag("dl-1"));
    assert_ne!(work_tag("dl-1"), work_tag("dl-2"));
    assert_eq!(work_tag("dl-1").len(), 16);
    assert!(work_tag("../../x").chars().all(|c| c.is_ascii_hexdigit()));
    // Fixed across builds and toolchains: the first 8 bytes of SHA-256("dl-1").
    assert_eq!(work_tag("dl-1"), "f45e1b72e3715dc0");
}

#[test]
fn download_dir_setting_is_honoured_when_usable() {
    let home = Path::new("/home/someone");
    let dir = |settings: serde_json::Value| configured_download_dir(&settings, Some(home));

    assert_eq!(dir(serde_json::json!({ "download_dir": "/mnt/media/Anime" })), Some(PathBuf::from("/mnt/media/Anime")));
    assert_eq!(dir(serde_json::json!({ "download_dir": "  /mnt/media/Anime  " })), Some(PathBuf::from("/mnt/media/Anime")));
    assert_eq!(dir(serde_json::json!({ "download_dir": "~/Videos/Anime" })), Some(PathBuf::from("/home/someone/Videos/Anime")));
    assert_eq!(dir(serde_json::json!({ "download_dir": "~" })), Some(PathBuf::from("/home/someone")));

    // Unset, empty, not a string, or relative: the default folder is used instead.
    assert_eq!(dir(serde_json::json!({})), None);
    assert_eq!(dir(serde_json::json!({ "download_dir": "" })), None);
    assert_eq!(dir(serde_json::json!({ "download_dir": "   " })), None);
    assert_eq!(dir(serde_json::json!({ "download_dir": null })), None);
    assert_eq!(dir(serde_json::json!({ "download_dir": 5 })), None);
    assert_eq!(dir(serde_json::json!({ "download_dir": "Videos/Anime" })), None);
    assert_eq!(configured_download_dir(&serde_json::json!({ "download_dir": "~/x" }), None), None);
}

#[test]
fn stale_part_files_are_removed_and_nothing_else() {
    let dir = std::env::temp_dir().join(format!("anix-part-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(format!("folder{}", PART_SUFFIX))).unwrap();

    let part_a = dir.join(format!("Show - EP 01 [SUB 720p].0123456789abcdef.video{}", PART_SUFFIX));
    let part_b = dir.join(format!("Show - EP 01 [SUB 720p].0123456789abcdef.sub0{}", PART_SUFFIX));
    let finished = dir.join("Show - EP 01 [SUB 720p].mkv");
    let other = dir.join("notes.txt");
    for file in [&part_a, &part_b, &finished, &other] {
        std::fs::write(file, b"x").unwrap();
    }

    assert_eq!(remove_stale_part_files(&dir, &[]), 2);
    assert!(!part_a.exists() && !part_b.exists());
    assert!(finished.exists() && other.exists());
    assert!(dir.join(format!("folder{}", PART_SUFFIX)).is_dir(), "directories are left alone");
    assert_eq!(remove_stale_part_files(&dir, &[]), 0);
    assert_eq!(remove_stale_part_files(&dir.join("missing"), &[]), 0);

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------
// URLs and headers
// ---------------------------------------------------------------------------------------

#[test]
fn only_http_and_https_stream_urls_are_accepted() {
    assert_eq!(
        validate_stream_url(" https://cdn.example/a/index.m3u8?t=1 ").unwrap(),
        "https://cdn.example/a/index.m3u8?t=1"
    );
    assert!(validate_stream_url("http://127.0.0.1:8080/v.mp4").is_ok());

    for bad in [
        "file:///etc/passwd",
        "concat:/etc/passwd|/etc/shadow",
        "-i",
        "-f lavfi",
        "/etc/passwd",
        "ftp://host/file.mp4",
        "data:text/plain,hi",
        "subfile,,start,0,end,10,,:/etc/passwd",
        "https://",
        "",
    ] {
        assert!(validate_stream_url(bad).is_err(), "{:?} must be rejected", bad);
    }
}

fn source(referer: Option<&str>, origin: Option<&str>, headers: &[(&str, &str)]) -> DownloadSource {
    DownloadSource {
        url: "https://cdn.example/index.m3u8".to_string(),
        stream_type: Some("hls".to_string()),
        referer: referer.map(str::to_string),
        origin: origin.map(str::to_string),
        headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>(),
        subtitles: Vec::new(),
        language: None,
    }
}

fn pairs(headers: &RequestHeaders) -> Vec<(&str, &str)> {
    headers.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

#[test]
fn request_headers_include_referer_origin_custom_headers_and_a_browser_user_agent() {
    let headers = request_headers(&source(
        Some("https://player.example:8443/embed/1"),
        None,
        &[("X-Token", "abc"), ("Accept", "*/*")],
    ));
    assert_eq!(headers.user_agent, BROWSER_USER_AGENT);
    assert!(headers.user_agent.starts_with("Mozilla/5.0"));
    assert_eq!(
        pairs(&headers),
        vec![
            ("Referer", "https://player.example:8443/embed/1"),
            ("Origin", "https://player.example:8443"),
            ("Accept", "*/*"),
            ("X-Token", "abc"),
        ]
    );

    // An explicit origin wins over the one derived from the referer.
    let headers = request_headers(&source(Some("https://a.example/x"), Some("https://b.example"), &[]));
    assert_eq!(pairs(&headers), vec![("Referer", "https://a.example/x"), ("Origin", "https://b.example")]);

    // Origin alone, and nothing at all.
    let headers = request_headers(&source(None, Some("https://b.example"), &[]));
    assert_eq!(pairs(&headers), vec![("Origin", "https://b.example")]);
    let headers = request_headers(&source(None, None, &[]));
    assert!(headers.headers.is_empty());
    assert_eq!(headers.user_agent, BROWSER_USER_AGENT);
}

#[test]
fn custom_headers_override_defaults_and_cannot_inject_lines() {
    let headers = request_headers(&source(
        Some("https://a.example/x"),
        None,
        &[
            ("user-agent", "Custom/1.0"),
            ("referer", "https://override.example/"),
            ("X-Evil", "1\r\nHost: attacker.example"),
            ("Bad Name", "x"),
            ("Bad\r\nName", "x"),
            ("X-Empty", "   "),
        ],
    ));

    assert_eq!(headers.user_agent, "Custom/1.0");
    assert_eq!(
        pairs(&headers),
        vec![
            ("Referer", "https://override.example/"),
            ("Origin", "https://a.example"),
            ("X-Evil", "1Host: attacker.example"),
        ]
    );
    for (name, value) in &headers.headers {
        assert!(!name.contains(['\r', '\n', ' ', ':']));
        assert!(!value.contains(['\r', '\n']));
    }
}

// ---------------------------------------------------------------------------------------
// ffmpeg arguments
// ---------------------------------------------------------------------------------------

#[test]
fn ffmpeg_args_for_an_hls_stream_carry_headers_and_keep_the_url_behind_dash_i() {
    let input = FfmpegInput::Remote {
        url: "https://cdn.example/a/index.m3u8?t=1".to_string(),
        headers: RequestHeaders {
            user_agent: BROWSER_USER_AGENT.to_string(),
            headers: vec![
                ("Referer".to_string(), "https://player.example/embed/1".to_string()),
                ("Origin".to_string(), "https://player.example".to_string()),
                ("X-Token".to_string(), "abc".to_string()),
            ],
        },
        hls: Some(HlsDemuxerOptions { allowed_extensions: true, extension_picky: true }),
    };
    let output = Path::new("/downloads/Show - EP 01 [SUB 1080p].tag.video.anix-part");
    let args = strings(&build_ffmpeg_args(&input, None, output));

    assert_eq!(
        args,
        vec![
            "-hide_banner", "-nostdin", "-loglevel", "warning", "-y",
            "-nostats", "-progress", "pipe:1",
            "-user_agent", BROWSER_USER_AGENT,
            "-headers",
            "Referer: https://player.example/embed/1\r\nOrigin: https://player.example\r\nX-Token: abc\r\n",
            "-rw_timeout", "60000000",
            "-allowed_extensions", "ALL",
            "-extension_picky", "0",
            "-i", "https://cdn.example/a/index.m3u8?t=1",
            "-c", "copy",
            "-f", "matroska",
            "/downloads/Show - EP 01 [SUB 1080p].tag.video.anix-part",
        ]
    );

    // The URL is the value of -i and the output is the last operand, an absolute path.
    let i = position(&args, "-i");
    assert_eq!(args[i + 1], "https://cdn.example/a/index.m3u8?t=1");
    assert!(args.last().unwrap().starts_with('/'));
}

#[test]
fn ffmpeg_args_only_use_hls_options_the_installed_ffmpeg_has() {
    let remote = |hls| FfmpegInput::Remote {
        url: "https://cdn.example/v".to_string(),
        headers: RequestHeaders { user_agent: "UA".to_string(), headers: Vec::new() },
        hls,
    };
    let output = Path::new("/d/out");

    let old_ffmpeg = strings(&build_ffmpeg_args(
        &remote(Some(HlsDemuxerOptions { allowed_extensions: true, extension_picky: false })),
        None,
        output,
    ));
    assert!(old_ffmpeg.contains(&"-allowed_extensions".to_string()));
    assert!(!old_ffmpeg.contains(&"-extension_picky".to_string()));

    // Not an HLS input (for example DASH): no HLS demuxer options, and no -headers when
    // there are no headers to send.
    let not_hls = strings(&build_ffmpeg_args(&remote(None), None, output));
    assert!(!not_hls.contains(&"-allowed_extensions".to_string()));
    assert!(!not_hls.contains(&"-extension_picky".to_string()));
    assert!(!not_hls.contains(&"-headers".to_string()));
    assert_eq!(not_hls[position(&not_hls, "-user_agent") + 1], "UA");
}

#[test]
fn ffmpeg_args_with_a_subtitle_add_it_as_a_second_typed_input() {
    let input = FfmpegInput::Local(PathBuf::from("/d/Show.tag.download.anix-part"));
    let output = Path::new("/d/Show.tag.video.anix-part");

    let vtt = SubtitleInput {
        path: PathBuf::from("/d/Show.tag.sub0.anix-part"),
        format: SubtitleFormat::WebVtt,
        language: "English".to_string(),
    };
    let args = strings(&build_ffmpeg_args(&input, Some(&vtt), output));
    assert_eq!(
        args,
        vec![
            "-hide_banner", "-nostdin", "-loglevel", "warning", "-y",
            "-nostats", "-progress", "pipe:1",
            "-i", "/d/Show.tag.download.anix-part",
            "-f", "webvtt", "-i", "/d/Show.tag.sub0.anix-part",
            "-c", "copy", "-c:s", "srt",
            "-metadata:s:s:0", "title=English",
            "-f", "matroska",
            "/d/Show.tag.video.anix-part",
        ]
    );
    // A local input sends no network options.
    assert!(!args.contains(&"-user_agent".to_string()));
    assert!(!args.contains(&"-rw_timeout".to_string()));

    // ASS keeps its styling; a blank language adds no metadata.
    let ass = SubtitleInput {
        path: PathBuf::from("/d/s"),
        format: SubtitleFormat::Ass,
        language: " \n".to_string(),
    };
    let args = strings(&build_ffmpeg_args(&input, Some(&ass), output));
    assert_eq!(args[position(&args, "-f") + 1], "ass");
    assert_eq!(args[position(&args, "-c:s") + 1], "ass");
    assert!(!args.iter().any(|a| a.starts_with("-metadata")));
}

#[test]
fn hls_demuxer_options_are_read_from_ffmpeg_help() {
    let modern = "\
Demuxer hls [Apple HTTP Live Streaming]:
hls demuxer AVOptions:
  -live_start_index  <int>        .D......... segment index to start live streams at
  -allowed_extensions <string>     .D......... List of file extensions that hls is allowed to access
  -allowed_segment_extensions <string>     .D......... List of file extensions
  -extension_picky   <boolean>    .D......... Be picky with all extensions matching (default true)
";
    assert_eq!(
        parse_hls_demuxer_help(modern),
        HlsDemuxerOptions { allowed_extensions: true, extension_picky: true }
    );

    let older = "  -allowed_extensions <string>     .D......... List of file extensions\n";
    assert_eq!(
        parse_hls_demuxer_help(older),
        HlsDemuxerOptions { allowed_extensions: true, extension_picky: false }
    );

    assert_eq!(parse_hls_demuxer_help("Unknown format 'hls'"), HlsDemuxerOptions::default());
    // A mention inside a description is not the option itself.
    assert_eq!(
        parse_hls_demuxer_help("  -other <int> see -extension_picky\n"),
        HlsDemuxerOptions::default()
    );
}

// ---------------------------------------------------------------------------------------
// Subtitles and stream choice
// ---------------------------------------------------------------------------------------

#[test]
fn subtitles_are_recognised_by_content() {
    assert_eq!(
        detect_subtitle_format(b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nHi\n"),
        Some(SubtitleFormat::WebVtt)
    );
    assert_eq!(
        detect_subtitle_format("\u{feff}WEBVTT - Title\n".as_bytes()),
        Some(SubtitleFormat::WebVtt)
    );
    assert_eq!(
        detect_subtitle_format(b"[Script Info]\nTitle: x\n\n[V4+ Styles]\n\n[Events]\n"),
        Some(SubtitleFormat::Ass)
    );
    assert_eq!(
        detect_subtitle_format(b"1\n00:00:01,000 --> 00:00:04,000\nHello\n\n2\n00:00:05,000 --> 00:00:06,000\nBye\n"),
        Some(SubtitleFormat::Srt)
    );

    // What a blocked or broken subtitle URL really returns.
    assert_eq!(detect_subtitle_format(b""), None);
    assert_eq!(detect_subtitle_format(b"<!DOCTYPE html><html><body>403 Forbidden</body></html>"), None);
    assert_eq!(detect_subtitle_format(b"<html>see 00:00:01,000 --> 00:00:04,000</html>"), None);
    assert_eq!(detect_subtitle_format(b"{\"error\":\"not found\"}"), None);
    assert_eq!(detect_subtitle_format(b"Access denied --> contact support"), None);
    assert_eq!(detect_subtitle_format(&[0u8, 1, 2, 3, 0, 0, 7]), None);
    assert_eq!(detect_subtitle_format(&vec![b'a'; 11 * 1024 * 1024]), None);

    assert_eq!(SubtitleFormat::WebVtt.extension(), "vtt");
    assert_eq!(SubtitleFormat::Ass.extension(), "ass");
    assert_eq!(SubtitleFormat::Srt.extension(), "srt");
}

fn stream(url: &str, quality: Option<&str>) -> StreamItem {
    StreamItem::new(
        url.to_string(),
        "hls".to_string(),
        quality.map(str::to_string),
        None,
        None,
        false,
    )
}

#[test]
fn stream_is_chosen_by_wanted_quality() {
    let streams = vec![
        stream("u360", Some("360p")),
        stream("u1080", Some("1080p")),
        stream("u720", Some("720p")),
        stream("uauto", Some("auto")),
    ];
    let pick = |wanted: &str| pick_stream(&streams, wanted).map(|s| s.url.as_str());

    assert_eq!(pick("1080p"), Some("u1080"));
    assert_eq!(pick("720p"), Some("u720"));
    assert_eq!(pick("480p"), Some("u360"), "nearest quality below the wish");
    assert_eq!(pick("240p"), Some("u360"), "nothing below: the smallest one");
    assert_eq!(pick("2160p"), Some("u1080"));
    for best in ["best", "auto", "highest", ""] {
        assert_eq!(pick(best), Some("u1080"), "{:?} means the highest quality", best);
    }

    // No stream states a quality: the provider's first one.
    let unrated = vec![stream("first", None), stream("second", Some("default"))];
    assert_eq!(pick_stream(&unrated, "1080p").map(|s| s.url.as_str()), Some("first"));
    assert_eq!(pick_stream(&unrated, "best").map(|s| s.url.as_str()), Some("first"));

    assert!(pick_stream(&[], "best").is_none());
}

#[test]
fn unknown_provider_names_fall_back_to_auto() {
    let known = ["bonk", "kiwi", "KickAssAnime", "AniBD"];

    assert_eq!(preferred_provider("kiwi", &known), Some("kiwi"));
    assert_eq!(preferred_provider("  kiwi ", &known), Some("kiwi"));
    assert_eq!(preferred_provider("kickassanime", &known), Some("KickAssAnime"), "spelled as the manager does");
    assert_eq!(preferred_provider("auto", &known), None);
    assert_eq!(preferred_provider("AUTO", &known), None);
    assert_eq!(preferred_provider("", &known), None);
    assert_eq!(preferred_provider("Some Old Display Name", &known), None);
    assert_eq!(preferred_provider("kiwi", &[]), None);
}

#[test]
fn hls_media_playlist_length_is_the_sum_of_its_segments() {
    use anix_lib::downloads::{parse_hls_playlist, HlsPlaylist};
    let text = "#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXTINF:10.0,\nseg1.ts\n#EXTINF:9.5,\nseg2.ts\n#EXTINF:4.25,title\nseg3.ts\n#EXT-X-ENDLIST\n";
    assert_eq!(parse_hls_playlist(text, "https://cdn.example/a/index.m3u8"), Some(HlsPlaylist::Media { duration_s: 23.75 }));
}

#[test]
fn hls_master_playlist_points_at_its_first_variant() {
    use anix_lib::downloads::{parse_hls_playlist, HlsPlaylist};
    let text = "\u{feff}#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n\n360/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2800000\n1080/index.m3u8\n";
    assert_eq!(
        parse_hls_playlist(text, "https://cdn.example/a/master.m3u8?t=1"),
        Some(HlsPlaylist::Master { first_variant: "https://cdn.example/a/360/index.m3u8".to_string() })
    );
}

#[test]
fn text_that_is_not_a_playlist_has_no_length() {
    use anix_lib::downloads::parse_hls_playlist;
    assert_eq!(parse_hls_playlist("<html>blocked</html>", "https://x.example/"), None);
    assert_eq!(parse_hls_playlist("#EXTM3U\n#EXT-X-ENDLIST\n", "https://x.example/"), None);
}

#[test]
fn resumable_part_files_of_queued_downloads_survive_the_startup_cleanup() {
    let dir = std::env::temp_dir().join(format!("anix-resume-part-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let queued = work_tag("queued-row");
    let gone = work_tag("deleted-row");
    let name = |tag: &str, role: &str| dir.join(format!("Show - EP 01 [SUB 720p].{}.{}{}", tag, role, PART_SUFFIX));
    for role in ["download", "resume", "video", "sub0"] {
        std::fs::write(name(&queued, role), b"x").unwrap();
        std::fs::write(name(&gone, role), b"x").unwrap();
    }
    std::fs::create_dir_all(name(&queued, "hls")).unwrap();
    std::fs::write(name(&queued, "hls").join("seg0.ts"), b"x").unwrap();
    std::fs::create_dir_all(name(&gone, "hls")).unwrap();

    // Everything of the deleted row goes, and of the queued row only what cannot resume.
    assert_eq!(remove_stale_part_files(&dir, &[queued.clone()]), 7);
    for role in ["download", "resume", "hls"] {
        assert!(name(&queued, role).exists(), "{} is kept", role);
        assert!(!name(&gone, role).exists(), "{} of another row is removed", role);
    }
    assert!(name(&queued, "hls").join("seg0.ts").exists());
    assert!(!name(&queued, "video").exists() && !name(&queued, "sub0").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn partials_in_a_folder_the_user_moved_away_from_are_cleaned_up() {
    let root = std::env::temp_dir().join(format!("anix-old-dirs-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (old, current, list) = (root.join("old"), root.join("current"), root.join("dirs.txt"));
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&current).unwrap();
    let part = |dir: &Path| dir.join(format!("Show - EP 01 [SUB 720p].{}.download{}", work_tag("q"), PART_SUFFIX));
    std::fs::write(part(&old), b"x").unwrap();
    std::fs::write(part(&current), b"x").unwrap();
    std::fs::write(old.join("Show - EP 01 [SUB 720p].mkv"), b"x").unwrap();

    remember_work_dir(&list, &old);
    remember_work_dir(&list, &old);
    remember_work_dir(&list, &root.join("unplugged"));
    remember_work_dir(&list, &current);
    assert_eq!(std::fs::read_to_string(&list).unwrap().lines().count(), 3, "each folder once");

    assert_eq!(remove_part_files_in_old_dirs(&list, &current), 1);
    assert!(!part(&old).exists());
    assert!(old.join("Show - EP 01 [SUB 720p].mkv").exists(), "finished downloads stay");
    assert!(part(&current).exists(), "the current folder is left to the usual cleanup");
    let kept = std::fs::read_to_string(&list).unwrap();
    assert!(!kept.contains("/old"), "{}", kept);
    assert!(kept.contains("unplugged"), "a folder that is not there now is tried again later: {}", kept);
    assert_eq!(remove_part_files_in_old_dirs(&root.join("no-list"), &current), 0);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resume_stamp_tells_variants_and_mirrors_apart_but_ignores_tokens() {
    let playlist = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.key?t=1\"\n#EXTINF:4,\na.ts?t=1\n#EXT-X-ENDLIST\n";
    let plan = |base: &str, text: &str| plan_hls_download(text, base).unwrap();
    let p720 = plan("https://cdn.example/720/i.m3u8", playlist);
    let p1080 = plan("https://cdn.example/1080/i.m3u8", playlist);
    let mirror = plan("https://mirror.example/720/i.m3u8", playlist);
    let new_token = plan("https://cdn.example/720/i.m3u8", &playlist.replace("t=1", "t=2"));
    assert_eq!(p720.playlist, p1080.playlist, "the local playlists alone look the same");
    assert_ne!(hls_source_stamp(&p720), hls_source_stamp(&p1080));
    assert_ne!(hls_source_stamp(&p720), hls_source_stamp(&mirror));
    assert_eq!(hls_source_stamp(&p720), hls_source_stamp(&new_token));
    assert_eq!(hls_source_stamp(&p720), "https://cdn.example/720/k.key\nhttps://cdn.example/720/a.ts");
}

#[test]
fn content_range_is_read_as_start_and_total() {
    assert_eq!(parse_content_range("bytes 1000-4999/5000"), Some((1000, 5000)));
    assert_eq!(parse_content_range(" bytes 0-0/1 "), Some((0, 1)));
    assert_eq!(parse_content_range("bytes 1000-4999/*"), None, "an unknown total cannot be checked");
    assert_eq!(parse_content_range("bytes */5000"), None);
    assert_eq!(parse_content_range("items 1-2/3"), None);
}

#[test]
fn a_folder_that_cannot_be_written_is_refused_with_a_reason() {
    let dir = std::env::temp_dir().join(format!("anix-writable-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(check_writable_dir(&dir.join("new/sub")).is_ok(), "a missing folder is created");
    assert!(dir.join("new/sub").is_dir());
    assert!(std::fs::read_dir(dir.join("new/sub")).unwrap().next().is_none(), "the check leaves nothing behind");

    let message = check_writable_dir(Path::new("relative/folder")).unwrap_err();
    assert!(message.contains("full path"), "{}", message);

    use std::os::unix::fs::PermissionsExt;
    let locked = dir.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    // root writes anywhere, so the refusal can only be seen as a normal user.
    if unsafe { libc::geteuid() } != 0 {
        let message = check_writable_dir(&locked).unwrap_err();
        assert!(message.starts_with("Anix cannot save downloads in"), "{}", message);
        assert!(message.contains("Choose another folder"), "{}", message);
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn download_rows_get_audio_and_subtitle_languages() {
    use anix_lib::models::SubtitleItem;
    let track = |language: &str, is_default: bool| SubtitleItem {
        url: "https://cdn.example/s.vtt".to_string(),
        language: language.to_string(),
        format: "vtt".to_string(),
        is_default: Some(is_default),
    };
    let tracks = [track("Spanish", false), track("English [CC]", true)];
    assert_eq!(download_languages("sub", None, &tracks), (Some("ja".into()), Some("en".into())));
    assert_eq!(download_languages("sub", Some("pt"), &tracks), (Some("ja".into()), Some("pt".into())), "the provider's word wins");
    assert_eq!(download_languages("sub", None, &[]), (Some("ja".into()), None));
    assert_eq!(download_languages("dub", Some("HI"), &tracks), (Some("hi".into()), None));
    assert_eq!(download_languages("dub", None, &tracks), (None, None));
}

#[test]
fn hls_attributes_are_read_with_quotes_and_commas() {
    let attrs = r#"METHOD=AES-128,URI="https://k.example/key?a=1,b=2",IV=0x01"#;
    assert_eq!(hls_attribute(attrs, "METHOD"), Some("AES-128"));
    assert_eq!(hls_attribute(attrs, "URI"), Some("https://k.example/key?a=1,b=2"));
    assert_eq!(hls_attribute(attrs, "IV"), Some("0x01"));
    assert_eq!(hls_attribute(attrs, "KEYFORMAT"), None);
}

#[test]
fn the_best_variant_of_a_master_playlist_is_chosen() {
    let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow/index.m3u8\n\
                  #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nhigh/index.m3u8\n\
                  #EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720\nmid/index.m3u8\n";
    assert_eq!(
        best_hls_variant(master, "https://cdn.example/v/master.m3u8").as_deref(),
        Some("https://cdn.example/v/high/index.m3u8")
    );
    let separate_audio = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"ja\",URI=\"audio.m3u8\"\n\
                          #EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\nvideo.m3u8\n";
    assert_eq!(best_hls_variant(separate_audio, "https://cdn.example/m.m3u8"), None, "ffmpeg combines those itself");
}

#[test]
fn a_vod_playlist_is_planned_file_by_file_with_local_names() {
    let text = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:7\n\
                #EXT-X-KEY:METHOD=AES-128,URI=\"/keys/k1.bin\",IV=0x0A\n\
                #EXTINF:4.0,\nseg-a.jpg\n#EXTINF:3.5,\nhttps://other.example/seg-b.jpg?t=1\n#EXT-X-ENDLIST\n";
    let plan = plan_hls_download(text, "https://cdn.example/hls/index.m3u8").expect("plannable");
    assert_eq!(
        plan.files,
        vec![
            ("https://cdn.example/keys/k1.bin".to_string(), "key0.key".to_string()),
            ("https://cdn.example/hls/seg-a.jpg".to_string(), "seg0.ts".to_string()),
            ("https://other.example/seg-b.jpg?t=1".to_string(), "seg1.ts".to_string()),
        ]
    );
    assert_eq!(plan.duration_s, 7.5);
    assert!(plan.playlist.contains("#EXT-X-KEY:METHOD=AES-128,URI=\"key0.key\",IV=0x0A"), "{}", plan.playlist);
    assert!(plan.playlist.contains("#EXT-X-MEDIA-SEQUENCE:7"), "tags the key needs are kept");
    assert!(!plan.playlist.contains("http"), "ffmpeg only reads local files: {}", plan.playlist);

    let fmp4 = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:2,\na.m4s\n#EXT-X-ENDLIST\n";
    let plan = plan_hls_download(fmp4, "https://cdn.example/x/i.m3u8").unwrap();
    assert_eq!(plan.files[0].1, "init0.mp4");
    assert_eq!(plan.files[1].1, "seg0.m4s");

    // What the plan does not cover is left to ffmpeg.
    let base = "https://cdn.example/i.m3u8";
    assert!(plan_hls_download("#EXTM3U\n#EXTINF:2,\na.ts\n", base).is_none(), "live");
    assert!(plan_hls_download("#EXTM3U\n#EXT-X-BYTERANGE:100@0\n#EXTINF:2,\na.ts\n#EXT-X-ENDLIST\n", base).is_none());
    assert!(plan_hls_download("#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n#EXTINF:2,\na.ts\n#EXT-X-ENDLIST\n", base).is_none());
    assert!(plan_hls_download("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n#EXT-X-ENDLIST\n", base).is_none());
    assert!(plan_hls_download("#EXTM3U\n#EXTINF:2,\nfile:///etc/passwd\n#EXT-X-ENDLIST\n", base).is_none(), "only http(s)");
    assert!(plan_hls_download("#EXTM3U\n#EXT-X-ENDLIST\n", base).is_none(), "no segments");
}

#[test]
fn a_local_hls_playlist_gets_the_relaxed_extension_options() {
    let input = FfmpegInput::LocalHls {
        playlist: PathBuf::from("/d/Show.tag.hls.anix-part/index.m3u8"),
        hls: HlsDemuxerOptions { allowed_extensions: true, extension_picky: true },
    };
    let args = strings(&build_ffmpeg_args(&input, None, Path::new("/d/out.mkv")));
    let i = position(&args, "-i");
    assert_eq!(args[i + 1], "/d/Show.tag.hls.anix-part/index.m3u8");
    assert!(position(&args, "-allowed_extensions") < i && position(&args, "-extension_picky") < i);
    assert!(!args.iter().any(|a| a == "-headers" || a == "-user_agent"), "no network options for local files");
}

#[test]
fn deleting_a_finished_download_removes_its_video_and_side_subtitles_only() {
    let dir = std::env::temp_dir().join(format!("anix-delete-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("Show - EP 01 [SUB 720p].mkv");
    let mine = ["Show - EP 01 [SUB 720p].english.vtt", "Show - EP 01 [SUB 720p].english-2.ass"];
    let kept = [
        "Show - EP 01 [SUB 720p] (2).mkv",
        "Show - EP 01 [SUB 720p] (2).english.vtt",
        "Show - EP 02 [SUB 720p].english.vtt",
        "Show - EP 01 [SUB 720p].notes.txt",
    ];
    std::fs::write(&video, b"x").unwrap();
    for name in mine.iter().chain(kept.iter()) {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    assert_eq!(delete_finished_files(&video), Ok(3));
    assert!(!video.exists());
    for name in mine {
        assert!(!dir.join(name).exists(), "{name} should be gone");
    }
    for name in kept {
        assert!(dir.join(name).exists(), "{name} belongs to something else");
    }
    assert_eq!(delete_finished_files(&video), Ok(0), "an already deleted video is fine");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_refuses_anything_that_is_not_a_downloaded_video_file() {
    let dir = std::env::temp_dir().join(format!("anix-delete-refuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("folder.mkv")).unwrap();
    let notes = dir.join("notes.txt");
    std::fs::write(&notes, b"x").unwrap();
    std::os::unix::fs::symlink(&notes, dir.join("link.mp4")).unwrap();

    assert!(delete_finished_files(&notes).is_err(), "not a video extension");
    assert!(delete_finished_files(Path::new("relative/Show.mkv")).is_err(), "relative path");
    assert!(delete_finished_files(&dir.join("folder.mkv")).is_err(), "a directory");
    assert!(delete_finished_files(&dir.join("link.mp4")).is_err(), "a symbolic link");
    assert!(notes.exists() && dir.join("folder.mkv").is_dir() && dir.join("link.mp4").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
