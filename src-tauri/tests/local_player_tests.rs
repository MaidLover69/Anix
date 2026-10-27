use anix_lib::commands::{
    build_external_playlist, common_referer, launch_stream_player,
    launch_playlist_external_player, local_file_player_args, open_file_in_player,
    read_settings_file, remove_stale_playlist_files, resolve_local_media_file,
    resolve_player_bin, single_line, stream_player_args, strip_appdir_entries,
    validate_media_url, write_settings_file,
};
use anix_lib::models::ExternalPlaylistItem;
use std::os::unix::fs::PermissionsExt;

fn temp_path(tag: &str, ext: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "anix_local_player_{}_{}.{}",
        tag,
        std::process::id(),
        ext
    ))
}

fn temp_video_file(tag: &str) -> std::path::PathBuf {
    let path = temp_path(tag, "mkv");
    std::fs::write(&path, b"").expect("Failed to create temp video file");
    path
}

#[tokio::test]
async fn test_open_file_in_player_rejects_arbitrary_program() {
    let file = temp_video_file("reject");
    let file_path = file.to_string_lossy().to_string();

    for program in [
        "true",
        "/usr/bin/true",
        "sh",
        "../../usr/bin/true",
        "mpv --script=/tmp/x.lua",
        "/usr/bin/mpv",
        " mpv",
        "mpv\0",
    ] {
        let res = open_file_in_player(file_path.clone(), Some(program.to_string())).await;
        assert!(res.is_err(), "player {:?} must not be executed", program);
    }

    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn test_stream_commands_reject_unknown_player_and_unsafe_urls() {
    let url = "https://cdn.example.com/master.m3u8".to_string();
    assert!(launch_stream_player("sh".to_string(), url.clone(), None, None, None, None, None, None).await.is_err());
    assert!(launch_stream_player("mpv".to_string(), "--script=/tmp/x.lua".to_string(), None, None, None, None, None, None)
        .await
        .is_err());
    assert!(launch_stream_player("vlc".to_string(), "file:///etc/passwd".to_string(), None, None, None, None, None, None)
        .await
        .is_err());

    let item = |url: &str| ExternalPlaylistItem {
        url: url.to_string(),
        title: "x".to_string(),
        referer: None,
    };
    assert!(launch_playlist_external_player("sh".to_string(), "p".to_string(), vec![item(&url)])
        .await
        .is_err());
    assert!(
        launch_playlist_external_player("mpv".to_string(), "p".to_string(), vec![item("file:///etc/passwd")])
            .await
            .is_err()
    );
    assert!(launch_playlist_external_player("mpv".to_string(), "p".to_string(), vec![]).await.is_err());
}

#[test]
fn test_resolve_player_bin_allow_list() {
    assert_eq!(resolve_player_bin(None), Ok("mpv"));
    assert_eq!(resolve_player_bin(Some("mpv")), Ok("mpv"));
    assert_eq!(resolve_player_bin(Some("MPV")), Ok("mpv"));
    // The default setting value: there is no built-in path for external playback.
    assert_eq!(resolve_player_bin(Some("builtin")), Ok("mpv"));
    assert_eq!(resolve_player_bin(Some("vlc")), Ok("vlc"));
    assert_eq!(resolve_player_bin(Some("Vlc")), Ok("vlc"));

    for rejected in ["sh", "true", "/usr/bin/vlc", "vlc ", "mpv --x", "celluloid"] {
        assert!(resolve_player_bin(Some(rejected)).is_err(), "{:?}", rejected);
    }
}

#[test]
fn test_resolve_local_media_file_is_absolute_regular_file() {
    let dir = temp_path("resolve_dir", "d");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // A file whose name looks like an option, reached through a relative path.
    let hostile = dir.join("--script=x.mkv");
    std::fs::write(&hostile, b"").unwrap();
    let cwd = std::env::current_dir().unwrap();
    let relative = pathdiff(&hostile, &cwd);
    assert!(!std::path::Path::new(&relative).is_absolute(), "{}", relative);
    let resolved = resolve_local_media_file(&relative).expect("existing file");
    assert!(resolved.is_absolute());
    assert!(resolved.to_string_lossy().starts_with('/'));
    assert!(resolved.is_file());

    // A symlink keeps its own name (players look for subtitles next to it) but must point
    // at a regular file.
    let link = dir.join("Show - 01.mkv");
    std::os::unix::fs::symlink(&hostile, &link).unwrap();
    assert_eq!(resolve_local_media_file(&link.to_string_lossy()).unwrap(), link);
    let dir_link = dir.join("folder.mkv");
    std::os::unix::fs::symlink(&dir, &dir_link).unwrap();
    assert!(resolve_local_media_file(&dir_link.to_string_lossy()).is_err());

    // Directories and missing paths are not playable files.
    assert!(resolve_local_media_file(&dir.to_string_lossy()).is_err());
    assert!(resolve_local_media_file("--script=/tmp/does-not-exist.lua").is_err());
    assert!(resolve_local_media_file("").is_err());

    // The operand can never be read as an option: mpv gets it after `--`.
    let mpv_args = local_file_player_args("mpv", &resolved);
    assert_eq!(mpv_args[0], "--");
    assert_eq!(mpv_args[1], resolved.as_os_str());
    assert_eq!(local_file_player_args("vlc", &resolved), vec![resolved.clone().into_os_string()]);

    let _ = std::fs::remove_dir_all(&dir);
}

/// `target` expressed relative to `base` (both absolute), using `..` segments.
fn pathdiff(target: &std::path::Path, base: &std::path::Path) -> String {
    let ups = base.components().count() - 1;
    let mut relative = std::path::PathBuf::new();
    for _ in 0..ups {
        relative.push("..");
    }
    relative.push(target.strip_prefix("/").unwrap());
    relative.to_string_lossy().to_string()
}

#[test]
fn test_validate_media_url_only_http() {
    assert!(validate_media_url("https://cdn.example.com/a/master.m3u8?t=1").is_ok());
    assert!(validate_media_url("http://cdn.example.com/v.mp4").is_ok());

    for rejected in [
        "--script=/tmp/x.lua",
        "-v",
        "file:///etc/passwd",
        "/etc/passwd",
        "concat:a|b",
        "javascript:alert(1)",
        "smb://host/share/x.mkv",
        "",
    ] {
        assert!(validate_media_url(rejected).is_err(), "{:?}", rejected);
    }

    // What comes back is the normalised URL, never the raw input: whitespace and control
    // characters cannot survive into an argument or a playlist line.
    assert_eq!(validate_media_url(" \thttps://a.example/b c\n").unwrap(), "https://a.example/b%20c");
    let smuggled = validate_media_url("https://a.example/1.m3u8\n#EXTVLCOPT:sout=x\nfile:///etc/passwd").unwrap();
    assert!(!smuggled.contains('\n') && !smuggled.contains(' '), "{}", smuggled);
    assert!(smuggled.starts_with("https://a.example/"));
}

#[test]
fn test_stream_player_args_cannot_inject_options() {
    let args = stream_player_args(
        "mpv",
        "https://cdn.example.com/master.m3u8",
        Some("Title\n--script=/tmp/x.lua"),
        Some("https://site.example/, X-Evil: 1"),
    )
    .expect("valid stream");
    // The URL is the last argument, after the end-of-options marker.
    assert_eq!(args[args.len() - 2], "--");
    assert_eq!(args[args.len() - 1], "https://cdn.example.com/master.m3u8");
    // The title stays one argument; the referer is one normalised URL, never a header list.
    assert!(args.iter().all(|a| !a.contains('\n')));
    assert_eq!(args.iter().filter(|a| a.starts_with("--script")).count(), 0);
    assert_eq!(args.iter().filter(|a| a.starts_with("--http-header-fields")).count(), 0);
    let referrer = args.iter().find(|a| a.starts_with("--referrer=")).expect("referrer");
    assert!(!referrer.contains(' '), "{}", referrer);

    let vlc = stream_player_args("vlc", "https://a.example/v.mp4", None, Some("https://a.example/"))
        .expect("valid stream");
    assert_eq!(vlc, vec!["--http-referrer=https://a.example/", "https://a.example/v.mp4"]);

    assert!(stream_player_args("mpv", "--input-commands=run sh", None, None).is_err());
    assert!(stream_player_args("vlc", "file:///etc/passwd", None, None).is_err());
}

#[test]
fn test_single_line_strips_control_characters() {
    assert_eq!(single_line("  One\r\nTwo\tThree\u{0} "), "OneTwoThree");
}

#[test]
fn test_build_external_playlist_rejects_injection() {
    let ok = build_external_playlist(&[ExternalPlaylistItem {
        url: "https://cdn.example.com/1.m3u8".to_string(),
        title: "Show - EP 1\n#EXTVLCOPT:sout=evil\nfile:///etc/passwd".to_string(),
        referer: Some("https://site.example/".to_string()),
    }])
    .expect("valid playlist");
    assert_eq!(
        ok,
        "#EXTM3U\n#EXTINF:-1,Show - EP 1#EXTVLCOPT:sout=evilfile:///etc/passwd\n\
         #EXTVLCOPT:http-referrer=https://site.example/\nhttps://cdn.example.com/1.m3u8\n"
    );
    assert_eq!(ok.lines().count(), 4);
    assert!(!ok.contains("EXT-X-PLAYLIST-TYPE"));

    let bad = build_external_playlist(&[ExternalPlaylistItem {
        url: "file:///etc/passwd".to_string(),
        title: "x".to_string(),
        referer: None,
    }]);
    assert!(bad.is_err());

    // Line breaks in the URL or referer cannot add playlist lines or player options.
    let smuggled = build_external_playlist(&[ExternalPlaylistItem {
        url: "https://a.example/1.m3u8\n#EXTVLCOPT:sout=x\nfile:///etc/passwd".to_string(),
        title: "x".to_string(),
        referer: Some("https://site.example/\n#EXTVLCOPT:sout=y".to_string()),
    }])
    .expect("normalised playlist");
    assert_eq!(smuggled.lines().count(), 4, "{}", smuggled);
    // The injected text survives only inside a URL fragment, never as a line of its own.
    assert_eq!(smuggled.lines().filter(|l| l.starts_with("#EXTVLCOPT:")).count(), 1);
    assert!(smuggled.lines().all(|l| !l.starts_with("file:")));
}

#[test]
fn test_common_referer_only_when_every_item_agrees() {
    let item = |referer: Option<&str>| ExternalPlaylistItem {
        url: "https://cdn.example.com/1.m3u8".to_string(),
        title: "x".to_string(),
        referer: referer.map(String::from),
    };
    let site = Some("https://site.example/");
    assert_eq!(common_referer(&[item(site), item(site)]).as_deref(), site);
    assert_eq!(common_referer(&[item(site), item(Some("https://other.example/"))]), None);
    assert_eq!(common_referer(&[item(site), item(None)]), None);
    assert_eq!(common_referer(&[item(Some("file:///etc/passwd"))]), None);
    assert_eq!(common_referer(&[]), None);
}

#[test]
fn test_stale_playlist_files_are_removed() {
    let dir = temp_path("playlist_dir", "d");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ours = dir.join("anix_playlist_demo_1_2.m3u");
    let other = dir.join("someone_elses.m3u");
    std::fs::write(&ours, b"#EXTM3U\n").unwrap();
    std::fs::write(&other, b"#EXTM3U\n").unwrap();

    remove_stale_playlist_files(&dir, std::time::Duration::from_secs(3600));
    assert!(ours.exists(), "a fresh playlist may still be in use");

    remove_stale_playlist_files(&dir, std::time::Duration::ZERO);
    assert!(!ours.exists());
    assert!(other.exists(), "only this app's playlist files are touched");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_strip_appdir_entries() {
    let appdir = "/tmp/.mount_Anix";
    assert_eq!(
        strip_appdir_entries("/tmp/.mount_Anix/usr/bin/:/usr/local/bin:/usr/bin", appdir),
        "/usr/local/bin:/usr/bin"
    );
    assert_eq!(strip_appdir_entries("/tmp/.mount_Anix/usr/lib/:", appdir), "");
    // A sibling directory that only shares the prefix is kept.
    assert_eq!(
        strip_appdir_entries("/tmp/.mount_Anix2/bin:/tmp/.mount_Anix", appdir),
        "/tmp/.mount_Anix2/bin"
    );
}

#[test]
fn test_settings_file_is_private_and_survives_corruption() {
    let path = temp_path("settings", "json");
    let _ = std::fs::remove_file(&path);

    assert_eq!(read_settings_file(&path).unwrap(), serde_json::json!({}));

    let settings = serde_json::json!({ "anilist_token": "secret", "autoplay": true });
    write_settings_file(&path, &settings).expect("write settings");
    assert_eq!(read_settings_file(&path).unwrap(), settings);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // Overwriting a world-readable file tightens it too.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    write_settings_file(&path, &settings).expect("rewrite settings");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    assert!(write_settings_file(&path, &serde_json::json!([1, 2])).is_err());

    // A file left world-readable by an older version is tightened as soon as it is read.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(read_settings_file(&path).unwrap(), settings);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // A settings file symlinked elsewhere (dotfiles) is written through the link.
    let link = temp_path("settings_link", "json");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let changed = serde_json::json!({ "autoplay": false });
    write_settings_file(&link, &changed).expect("write through symlink");
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(read_settings_file(&path).unwrap(), changed);
    let _ = std::fs::remove_file(&link);

    // A corrupt file is kept aside rather than lost to the next save.
    std::fs::write(&path, b"{ not json").unwrap();
    assert_eq!(read_settings_file(&path).unwrap(), serde_json::json!({}));
    let backup = path.with_extension("json.corrupt");
    assert_eq!(std::fs::read(&backup).unwrap(), b"{ not json");

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&backup);
}

#[test]
fn external_player_gets_the_subtitle_files() {
    use anix_lib::commands::player::{subtitle_file_name, subtitle_player_args};
    use std::path::PathBuf;
    let files = vec![PathBuf::from("/run/user/1000/anix_sub_1_1_English.vtt"), PathBuf::from("/run/user/1000/anix_sub_1_2_Español.ass")];
    let mpv: Vec<String> = subtitle_player_args("mpv", &files, true).iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(mpv, ["--sub-file=/run/user/1000/anix_sub_1_1_English.vtt", "--sub-file=/run/user/1000/anix_sub_1_2_Español.ass", "--sid=1"]);
    let off: Vec<String> = subtitle_player_args("mpv", &files, false).iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(off.last().unwrap(), "--sid=no");
    let vlc: Vec<String> = subtitle_player_args("vlc", &files, true).iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(vlc, ["--sub-file=/run/user/1000/anix_sub_1_1_English.vtt"]);
    assert!(subtitle_player_args("vlc", &files, false).is_empty());
    assert!(subtitle_player_args("mpv", &[], true).is_empty());
    assert_eq!(subtitle_file_name(7, 0, Some("pt-BR / Brasil"), "srt"), "anix_sub_7_1_pt_BR___Brasil.srt");
    assert_eq!(subtitle_file_name(7, 2, None, "vtt"), "anix_sub_7_3_subtitle.vtt");
}

#[test]
fn only_web_addresses_are_handed_to_the_browser() {
    use anix_lib::commands::player::external_url;
    assert_eq!(
        external_url(" https://comick.dev/oauth/authorize?a=1&b=two ").as_deref(),
        Ok("https://comick.dev/oauth/authorize?a=1&b=two")
    );
    assert!(external_url("http://127.0.0.1:47823/x").is_ok());
    for bad in ["file:///etc/passwd", "javascript:alert(1)", "--help", "/usr/bin/true", "mailto:a@b.c", "https://", ""] {
        assert!(external_url(bad).is_err(), "{bad} must be refused");
    }
}
