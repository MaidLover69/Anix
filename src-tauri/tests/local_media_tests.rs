//! Tests of "Your files": the link tables, episode mapping, sidecar subtitles, the proxy's
//! allow-list and its local-file routes.
//!
//! Every test uses an in-memory database and files in a fresh temp directory.

use anix_lib::commands::local_episode_source;
use anix_lib::commands::local_scanner::{parse_episode_and_title, LocalAnimeFile};
use anix_lib::db;
use anix_lib::local_media::{
    self, episode_for_file_name, find_sidecars, is_sidecar_of, parse_byte_range, playable_path,
    register_playable, sidecar_label, video_content_type, PlayableKind,
};
use anix_lib::models::DownloadRecord;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-local-media-{}-{}-{}",
            name,
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn file(&self, name: &str, body: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, body).expect("write file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn memory_db() -> SqlitePool {
    db::init_db_in_memory().await.expect("in-memory database")
}

fn scanned(path: &Path) -> LocalAnimeFile {
    let file_name = path.file_name().unwrap().to_string_lossy().to_string();
    let (parsed_title, parsed_episode) = parse_episode_and_title(&file_name);
    LocalAnimeFile {
        file_path: path.to_string_lossy().to_string(),
        file_size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        file_name,
        parsed_title,
        parsed_episode,
    }
}

#[test]
fn test_local_media_episode_numbers_from_file_names() {
    let cases: [(&str, Option<f64>); 9] = [
        ("[SubsPlease] Sousou no Frieren - 05 (1080p) [ABCD1234].mkv", Some(5.0)),
        ("[Erai-raws] Show - 12 [1080p][Multiple Subtitle].mkv", Some(12.0)),
        // A digit inside a release tag is not the episode.
        ("[Group-1] Show - 07 [720p].mp4", Some(7.0)),
        ("Show.S01E03.1080p.WEB.mkv", Some(3.0)),
        ("Show Episode 10.mp4", Some(10.0)),
        ("Show EP 2.webm", Some(2.0)),
        ("Show 08.mkv", Some(8.0)),
        ("Show - 06v2.mkv", Some(6.0)),
        ("Show - 13.5.mkv", Some(13.5)),
    ];
    for (name, want) in cases {
        assert_eq!(episode_for_file_name(name), want, "{}", name);
    }
    assert_eq!(episode_for_file_name("Opening.mkv"), None);
    let (title, _) = parse_episode_and_title("[SubsPlease] Sousou no Frieren - 05 (1080p).mkv");
    assert_eq!(title, "Sousou no Frieren");
}

#[tokio::test]
async fn test_local_media_link_scan_and_rescan() {
    let pool = memory_db().await;
    let dir = TempDir::new("scan");
    let ep1 = dir.file("Show - 01.mkv", b"one");
    let ep2 = dir.file("Show - 02.mp4", b"two");
    let extra = dir.file("NCOP.mkv", b"op");
    let folder = dir.0.to_string_lossy().to_string();

    let empty = local_media::get_link(&pool, 7).await.unwrap();
    assert!(!empty.linked);
    assert!(empty.files.is_empty() && empty.episodes.is_empty());

    let files = [scanned(&ep1), scanned(&ep2), scanned(&extra)];
    local_media::save_scan(&pool, 7, Some(&folder), &files).await.unwrap();
    let link = local_media::get_link(&pool, 7).await.unwrap();
    assert!(link.linked);
    assert_eq!(link.folder_path.as_deref(), Some(folder.as_str()));
    assert_eq!(link.files.len(), 3);
    assert_eq!(link.episodes, vec![1.0, 2.0]);
    assert!(link.files.iter().all(|f| f.exists && f.from_folder && !f.manual));
    // Other titles are not affected.
    assert!(!local_media::get_link(&pool, 8).await.unwrap().linked);

    // The user maps the unnumbered file and corrects episode 2 to 3.
    let op = link.files.iter().find(|f| f.file_name == "NCOP.mkv").unwrap().id;
    let two = link.files.iter().find(|f| f.file_name == "Show - 02.mp4").unwrap().id;
    assert!(local_media::set_file_episode(&pool, 7, two, Some(3.0)).await.unwrap());
    assert!(local_media::set_file_episode(&pool, 7, op, None).await.unwrap());
    // A file id of another title is refused.
    assert!(!local_media::set_file_episode(&pool, 8, two, Some(1.0)).await.unwrap());
    assert!(local_media::set_file_episode(&pool, 7, two, Some(f64::NAN)).await.is_err());

    // Re-scan: episode 1 is gone from disk, episode 4 is new, the correction stays.
    std::fs::remove_file(&ep1).unwrap();
    let ep4 = dir.file("Show - 04.mkv", b"four");
    let files = [scanned(&ep2), scanned(&extra), scanned(&ep4)];
    local_media::save_scan(&pool, 7, Some(&folder), &files).await.unwrap();
    let link = local_media::get_link(&pool, 7).await.unwrap();
    assert_eq!(link.files.len(), 3);
    assert_eq!(link.episodes, vec![3.0, 4.0]);
    let two_row = link.files.iter().find(|f| f.id == two).unwrap();
    assert_eq!(two_row.episode_number, Some(3.0));
    assert!(two_row.manual);

    assert_eq!(
        local_media::episode_file(&pool, 7, 3.0).await.unwrap(),
        Some(ep2.clone())
    );
    assert_eq!(local_media::episode_file(&pool, 7, 2.0).await.unwrap(), None);
    assert_eq!(local_media::episode_file(&pool, 7, 1.0).await.unwrap(), None);

    // Remove one file, then unlink everything.
    assert!(local_media::remove_file(&pool, 7, two).await.unwrap());
    assert_eq!(local_media::get_link(&pool, 7).await.unwrap().episodes, vec![4.0]);
    local_media::unlink(&pool, 7).await.unwrap();
    let link = local_media::get_link(&pool, 7).await.unwrap();
    assert!(!link.linked && link.files.is_empty());
    assert!(ep4.exists(), "unlinking never deletes files");
}

#[tokio::test]
async fn test_local_media_change_folder_and_single_files() {
    let pool = memory_db().await;
    let old_dir = TempDir::new("old");
    let new_dir = TempDir::new("new");
    let single_dir = TempDir::new("single");
    let old_ep = old_dir.file("Show - 01.mkv", b"old");
    let new_ep = new_dir.file("Show - 02.mkv", b"new");
    let single = single_dir.file("Special - 05.mp4", b"single");

    local_media::save_scan(&pool, 1, Some(&old_dir.0.to_string_lossy()), &[scanned(&old_ep)])
        .await
        .unwrap();
    local_media::save_scan(&pool, 1, None, &[scanned(&single)]).await.unwrap();
    assert_eq!(local_media::get_link(&pool, 1).await.unwrap().episodes, vec![1.0, 5.0]);

    // A new folder replaces the old folder's files; single files stay.
    local_media::save_scan(&pool, 1, Some(&new_dir.0.to_string_lossy()), &[scanned(&new_ep)])
        .await
        .unwrap();
    let link = local_media::get_link(&pool, 1).await.unwrap();
    assert_eq!(link.episodes, vec![2.0, 5.0]);
    assert_eq!(link.folder_path.as_deref(), Some(&*new_dir.0.to_string_lossy()));
    assert!(link.files.iter().any(|f| f.file_path == single.to_string_lossy() && !f.from_folder));
}

#[tokio::test]
async fn test_local_media_counts_finished_downloads() {
    let pool = memory_db().await;
    let dir = TempDir::new("downloads");
    let done = dir.file("Show - Episode 3.mkv", b"dl");

    for (id, episode, status, path) in [
        ("a", 3.0, "COMPLETED", Some(done.to_string_lossy().to_string())),
        ("b", 4.0, "DOWNLOADING", Some(dir.0.join("partial.mkv").to_string_lossy().to_string())),
        ("c", 5.0, "COMPLETED", Some(dir.0.join("deleted.mkv").to_string_lossy().to_string())),
    ] {
        let record = DownloadRecord {
            id: id.to_string(),
            anilist_id: 42,
            episode_num: episode,
            series_title: "Show".to_string(),
            provider: "p".to_string(),
            category: "sub".to_string(),
            quality: "auto".to_string(),
            status: "QUEUED".to_string(),
            ..Default::default()
        };
        db::create_download(&pool, &record).await.unwrap();
        db::update_download_status(&pool, id, status, None, path.as_deref(), None, None, None)
            .await
            .unwrap();
    }

    let link = local_media::get_link(&pool, 42).await.unwrap();
    assert!(!link.linked, "downloads alone do not create a link");
    assert_eq!(link.downloads.len(), 2);
    assert_eq!(link.episodes, vec![3.0], "only finished downloads that exist count");
    assert_eq!(local_media::episode_file(&pool, 42, 3.0).await.unwrap(), Some(done));
    assert_eq!(local_media::episode_file(&pool, 42, 5.0).await.unwrap(), None);

    // A linked file is preferred over a download of the same episode.
    let linked = dir.file("mine/Show - 03.mp4", b"mine");
    local_media::save_scan(&pool, 42, None, &[scanned(&linked)]).await.unwrap();
    assert_eq!(local_media::episode_file(&pool, 42, 3.0).await.unwrap(), Some(linked));
}

#[test]
fn test_local_media_sidecar_subtitles() {
    let dir = TempDir::new("sidecar");
    let video = dir.file("Show - 01.mkv", b"v");
    dir.file("Show - 01.srt", b"1\n00:00:01,000 --> 00:00:02,000\nHi\n");
    dir.file("Show - 01.en.ass", b"[Script Info]");
    dir.file("Show - 012.srt", b"other episode");
    dir.file("Show - 02.srt", b"other episode");
    dir.file("Show - 01.txt", b"not a subtitle");
    dir.file("sub/Show - 01.srt", b"other folder");

    let found: Vec<String> = find_sidecars(&video)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(found, vec!["Show - 01.en.ass", "Show - 01.srt"]);
    assert!(!is_sidecar_of(&video, &dir.0.join("sub/Show - 01.srt")));
    assert_eq!(sidecar_label(&video, &dir.0.join("Show - 01.en.ass")), "en");
    assert_eq!(sidecar_label(&video, &dir.0.join("Show - 01.srt")), "Subtitles (SRT)");
}

#[test]
fn test_local_media_allow_list_only_serves_registered_files() {
    let dir = TempDir::new("allow");
    let video = dir.file("Show - 01.mp4", b"video");
    let subtitle = dir.file("Show - 01.srt", b"sub");
    let text = dir.file("notes.txt", b"secret");

    let key = register_playable(PlayableKind::Video, &video);
    assert_eq!(key.len(), 32);
    assert_eq!(register_playable(PlayableKind::Video, &video), key, "a file keeps its key");
    assert_eq!(playable_path(PlayableKind::Video, &key), Some(video.clone()));
    // A video key is not a subtitle key.
    assert_eq!(playable_path(PlayableKind::Subtitle, &key), None);
    let sub_key = register_playable(PlayableKind::Subtitle, &subtitle);
    assert_eq!(playable_path(PlayableKind::Subtitle, &sub_key), Some(subtitle));

    // Paths, traversal and unknown keys are refused.
    for bad in [
        "",
        "../../etc/passwd",
        "/etc/passwd",
        "%2e%2e%2fetc%2fpasswd",
        &video.to_string_lossy(),
        "0123456789abcdef0123456789abcdef",
        &format!("{}/..", key),
    ] {
        assert_eq!(playable_path(PlayableKind::Video, bad), None, "{:?}", bad);
    }

    // Only video files are served as video, even when registered.
    let text_key = register_playable(PlayableKind::Video, &text);
    assert_eq!(playable_path(PlayableKind::Video, &text_key), None);
    // A registered file that is gone is not served.
    std::fs::remove_file(&video).unwrap();
    assert_eq!(playable_path(PlayableKind::Video, &key), None);
}

#[test]
fn test_local_media_byte_ranges_and_types() {
    assert_eq!(parse_byte_range(None, 100), Ok(None));
    assert_eq!(parse_byte_range(Some("bytes=0-"), 100), Ok(Some((0, 99))));
    assert_eq!(parse_byte_range(Some("bytes=10-19"), 100), Ok(Some((10, 19))));
    assert_eq!(parse_byte_range(Some("bytes=90-500"), 100), Ok(Some((90, 99))));
    assert_eq!(parse_byte_range(Some("bytes=-10"), 100), Ok(Some((90, 99))));
    assert_eq!(parse_byte_range(Some("bytes=-500"), 100), Ok(Some((0, 99))));
    assert_eq!(parse_byte_range(Some("bytes=0-1,5-6"), 100), Ok(None));
    assert_eq!(parse_byte_range(Some("items=0-1"), 100), Ok(None));
    assert_eq!(parse_byte_range(Some("bytes=100-"), 100), Err(()));
    assert_eq!(parse_byte_range(Some("bytes=20-10"), 100), Err(()));
    assert_eq!(parse_byte_range(Some("bytes=x-"), 100), Err(()));
    assert_eq!(parse_byte_range(Some("bytes=0-"), 0), Err(()));

    assert_eq!(video_content_type(Path::new("/a/b.MP4")), "video/mp4");
    assert_eq!(video_content_type(Path::new("/a/b.mkv")), "video/x-matroska");
    assert_eq!(video_content_type(Path::new("/a/b.webm")), "video/webm");
}

#[tokio::test]
async fn test_local_media_proxy_routes() {
    let base = anix_lib::stream_proxy::start().expect("proxy");
    let dir = TempDir::new("proxy");
    let body: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    let video = dir.file("Show - 01.mp4", &body);
    dir.file("Show - 01.en.srt", b"1\n00:00:01,000 --> 00:00:02,500\nHello\n");
    let secret = dir.file("secret.mp4", b"not linked");

    let source = local_episode_source(&base, &video);
    assert_eq!(source.sources.provider.as_deref(), Some("local"));
    assert_eq!(source.sources.streams.len(), 1);
    assert_eq!(source.sources.subtitles.len(), 1);
    assert_eq!(source.sources.subtitles[0].language, "en");
    let stream_url = source.sources.streams[0].url.clone();
    let subtitle_url = source.sources.subtitles[0].url.clone();
    assert!(!stream_url.contains("/hls/") && !stream_url.contains(".m3u8"));

    let client = reqwest::Client::new();

    let full = client.get(&stream_url).send().await.unwrap();
    assert_eq!(full.status().as_u16(), 200);
    assert_eq!(full.headers()["content-type"], "video/mp4");
    assert_eq!(full.headers()["accept-ranges"], "bytes");
    assert_eq!(full.bytes().await.unwrap().to_vec(), body);

    let part = client
        .get(&stream_url)
        .header("Range", "bytes=100-199")
        .send()
        .await
        .unwrap();
    assert_eq!(part.status().as_u16(), 206);
    assert_eq!(part.headers()["content-range"], "bytes 100-199/4096");
    assert_eq!(part.bytes().await.unwrap().to_vec(), body[100..200].to_vec());

    let unsatisfiable = client
        .get(&stream_url)
        .header("Range", "bytes=5000-")
        .send()
        .await
        .unwrap();
    assert_eq!(unsatisfiable.status().as_u16(), 416);

    let head = client.head(&stream_url).send().await.unwrap();
    assert_eq!(head.status().as_u16(), 200);
    assert_eq!(head.headers()["content-length"], "4096");

    // The player wraps every URL in /proxy and /subtitle; the proxy answers its own routes.
    let wrapped = format!(
        "{}/proxy?url={}",
        base,
        url::form_urlencoded::byte_serialize(stream_url.as_bytes()).collect::<String>()
    );
    let wrapped = client
        .get(&wrapped)
        .header("Range", "bytes=0-9")
        .send()
        .await
        .unwrap();
    assert_eq!(wrapped.status().as_u16(), 206);
    assert_eq!(wrapped.bytes().await.unwrap().to_vec(), body[..10].to_vec());

    let vtt_url = format!(
        "{}/subtitle?url={}",
        base,
        url::form_urlencoded::byte_serialize(subtitle_url.as_bytes()).collect::<String>()
    );
    let vtt = client.get(&vtt_url).send().await.unwrap();
    assert_eq!(vtt.status().as_u16(), 200);
    let vtt = vtt.text().await.unwrap();
    assert!(vtt.starts_with("WEBVTT"), "{}", vtt);
    assert!(vtt.contains("00:00:01.000 --> 00:00:02.500"), "{}", vtt);

    // Refused: unknown keys, paths, a subtitle key on the video route, a wrong token, and a
    // local route under the wrong wrapper.
    let token_less = base.rsplit_once('/').unwrap().0.to_string();
    let subtitle_key = subtitle_url.rsplit_once('/').unwrap().1.to_string();
    for (url, status) in [
        (format!("{}/local/0123456789abcdef0123456789abcdef", base), 404),
        (format!("{}/local/..%2F..%2Fetc%2Fpasswd", base), 404),
        (format!("{}/local/{}", base, secret.to_string_lossy()), 404),
        (format!("{}/local/{}", base, subtitle_key), 404),
        (format!("{}/wrongtoken/local/{}", token_less, stream_url.rsplit_once('/').unwrap().1), 403),
        (
            format!(
                "{}/subtitle?url={}",
                base,
                url::form_urlencoded::byte_serialize(stream_url.as_bytes()).collect::<String>()
            ),
            404,
        ),
    ] {
        let response = client.get(&url).send().await.unwrap();
        assert_eq!(response.status().as_u16(), status, "{}", url);
    }
}
