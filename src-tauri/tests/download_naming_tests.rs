//! Pure-function tests for the download module: file names, request headers, the ffmpeg
//! argument list, stream and subtitle recognition. Nothing here runs ffmpeg or uses the
//! network.

use anix_lib::downloads::{
    build_ffmpeg_args, configured_download_dir, detect_subtitle_format, output_base_name,
    parse_hls_demuxer_help, pick_stream, preferred_provider, remove_stale_part_files,
    request_headers, sanitize_filename, truncate_to_bytes, validate_stream_url, work_tag,
    DownloadSource, FfmpegInput, HlsDemuxerOptions, RequestHeaders, SubtitleFormat, SubtitleInput,
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

    assert_eq!(remove_stale_part_files(&dir), 2);
    assert!(!part_a.exists() && !part_b.exists());
    assert!(finished.exists() && other.exists());
    assert!(dir.join(format!("folder{}", PART_SUFFIX)).is_dir(), "directories are left alone");
    assert_eq!(remove_stale_part_files(&dir), 0);
    assert_eq!(remove_stale_part_files(&dir.join("missing")), 0);

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
