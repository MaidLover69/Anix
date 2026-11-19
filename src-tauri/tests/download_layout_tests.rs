//! Issue #9: which subtitle tracks a download fetches, and the `<title>/Episode NN/` folder
//! layout for new downloads (naming, clean-up, resume, delete). Written before the code.
//!
//! Seams the code must provide (all in `anix_lib::downloads`):
//! - `select_subtitles(tracks, wanted: Option<&[String]>) -> Vec<SubtitleItem>`
//! - `subtitle_languages_for_download(settings: &Value) -> Option<Vec<String>>`
//! - `DownloadSource.subtitle_languages: Option<Vec<String>>` (`None` = every track)
//! - `episode_dir_name(episode_num) -> String`, `output_dir(root, series_title, episode_num) -> PathBuf`
//! - `download_under(pool, record, source, root) -> DownloadOutcome` (the file lands in `output_dir`)
//! - `delete_download_files(video, root) -> Result<usize, String>`
//! - `remove_stale_part_files(root, keep)` and `remove_resume_files(root, record)` reach one and
//!   two levels down.

mod english_ui;

use anix_lib::db;
use anix_lib::downloads::{
    self, delete_download_files, episode_dir_name, output_base_name, output_dir, remove_resume_files,
    remove_stale_part_files, select_subtitles, subtitle_languages_for_download, work_file_path, work_tag,
    DownloadOutcome, DownloadSource, MAX_FILE_NAME_BYTES, PART_SUFFIX,
};
use anix_lib::models::{DownloadRecord, SubtitleItem};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "anix-layout-{}-{}-{}",
            name,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

fn track(language: &str, is_default: bool) -> SubtitleItem {
    SubtitleItem {
        url: format!("http://127.0.0.1:1/{}", language),
        language: language.to_string(),
        format: "vtt".to_string(),
        is_default: Some(is_default),
    }
}

fn langs(tracks: &[SubtitleItem]) -> Vec<&str> {
    tracks.iter().map(|t| t.language.as_str()).collect()
}

fn wanted(codes: &[&str]) -> Option<Vec<String>> {
    Some(codes.iter().map(|c| c.to_string()).collect())
}

// ---------------------------------------------------------------------------------------
// Subtitle selection
// ---------------------------------------------------------------------------------------

#[test]
fn only_preferred_languages_are_chosen_in_the_users_order() {
    let tracks = vec![
        track("German", false),
        track("Spanish", false),
        track("English", true),
        track("French", false),
    ];
    let w = wanted(&["es", "en"]);
    let chosen = select_subtitles(&tracks, w.as_deref());
    assert_eq!(langs(&chosen), ["Spanish", "English"]);
}

#[test]
fn track_labels_are_matched_by_language_not_spelling() {
    let tracks = vec![track("Português (Brasil)", false), track("eng", false), track("English [CC]", false)];
    let w = wanted(&["pt", "en"]);
    let chosen = select_subtitles(&tracks, w.as_deref());
    assert_eq!(chosen.len(), 2, "one track per preferred language: {:?}", langs(&chosen));
    assert_eq!(chosen[0].language, "Português (Brasil)");
    assert_eq!(chosen[1].language, "eng");
}

#[test]
fn a_language_offered_several_times_gives_one_track_the_default_one_first() {
    let tracks = vec![track("English SDH", false), track("English", true), track("English (forced)", false)];
    let w = wanted(&["en"]);
    let chosen = select_subtitles(&tracks, w.as_deref());
    assert_eq!(langs(&chosen), ["English"]);

    let tracks = vec![track("Spanish", false), track("Spanish (Latin America)", false)];
    let w = wanted(&["es"]);
    assert_eq!(langs(&select_subtitles(&tracks, w.as_deref())), ["Spanish"]);
}

#[test]
fn without_a_match_the_sources_default_track_is_used() {
    let tracks = vec![track("German", false), track("French", true), track("Thai", false)];
    let w = wanted(&["en"]);
    assert_eq!(langs(&select_subtitles(&tracks, w.as_deref())), ["French"]);
    // No preference at all behaves the same, and no default flag means the first track.
    let w = wanted(&[]);
    assert_eq!(langs(&select_subtitles(&tracks, w.as_deref())), ["French"]);
    let tracks = vec![track("German", false), track("Thai", false)];
    let w = wanted(&["en"]);
    assert_eq!(langs(&select_subtitles(&tracks, w.as_deref())), ["German"]);
    assert!(select_subtitles(&[], w.as_deref()).is_empty());
}

#[test]
fn none_means_every_track_in_the_sources_order() {
    let tracks = vec![track("German", false), track("English", true), track("Thai", false)];
    assert_eq!(langs(&select_subtitles(&tracks, None)), ["English", "German", "Thai"]);
}

#[test]
fn the_setting_chooses_between_preferred_and_all_languages() {
    // Default: the user's subtitle languages, in order.
    let settings = json!({ "preferred_sub_languages": ["Spanish", "en"] });
    assert_eq!(subtitle_languages_for_download(&settings), wanted(&["es", "en"]));
    // Nothing set: the same default the player uses (English).
    assert_eq!(subtitle_languages_for_download(&json!({})), wanted(&["en"]));
    // "Download all subtitle languages" restores fetching every track.
    let all = json!({ "preferred_sub_languages": ["en"], "download_all_subtitles": true });
    assert_eq!(subtitle_languages_for_download(&all), None);
    let off = json!({ "preferred_sub_languages": ["en"], "download_all_subtitles": false });
    assert_eq!(subtitle_languages_for_download(&off), wanted(&["en"]));
}

// ---------------------------------------------------------------------------------------
// Folder names
// ---------------------------------------------------------------------------------------

#[test]
fn episode_folder_names() {
    assert_eq!(episode_dir_name(1.0), "Episode 01");
    assert_eq!(episode_dir_name(3.0), "Episode 03");
    assert_eq!(episode_dir_name(12.0), "Episode 12");
    assert_eq!(episode_dir_name(105.0), "Episode 105");
    assert_eq!(episode_dir_name(7.5), "Episode 7.5");
}

#[test]
fn a_new_download_goes_to_title_then_episode_under_the_download_folder() {
    let root = Path::new("/downloads");
    assert_eq!(output_dir(root, "Frieren", 3.0), root.join("Frieren").join("Episode 03"));
    assert_eq!(output_dir(root, "Re:Zero / Part 2?", 105.0), root.join("Re_Zero _ Part 2_").join("Episode 105"));
}

#[test]
fn hostile_titles_stay_one_folder_inside_the_download_folder() {
    let root = Path::new("/downloads");
    let long = "葬".repeat(500);
    let titles = ["..", ".", "...", "/", "../../etc", "a/../../b", "", "   ", "CON", "NUL.txt", "\0evil", &long];
    for title in titles {
        for ep in [1.0, 12.0, 105.0, 7.5] {
            let dir = output_dir(root, title, ep);
            let rel = dir.strip_prefix(root).unwrap_or_else(|_| panic!("{:?} escaped: {:?}", title, dir));
            let parts: Vec<_> = rel.components().collect();
            assert_eq!(parts.len(), 2, "{:?} -> {:?}", title, dir);
            for part in parts {
                let std::path::Component::Normal(name) = part else { panic!("{:?} -> {:?}", title, dir) };
                let name = name.to_string_lossy();
                assert!(!name.is_empty() && !name.starts_with('.'), "{:?} -> {:?}", title, dir);
                assert!(name.len() <= MAX_FILE_NAME_BYTES, "{} bytes for {:?}", name.len(), title);
                assert!(!name.contains(['/', '\\', ':', '\0']), "{:?}", name);
            }
        }
    }
    // An empty title gets the same stand-in as a file name does.
    assert_eq!(output_dir(root, "", 1.0), root.join("Untitled").join("Episode 01"));
}

// ---------------------------------------------------------------------------------------
// Clean-up and resume look one and two levels down
// ---------------------------------------------------------------------------------------

fn record(id: &str, title: &str, episode: f64) -> DownloadRecord {
    DownloadRecord {
        id: id.to_string(),
        anilist_id: 1,
        episode_num: episode,
        episode_title: None,
        series_title: title.to_string(),
        series_cover: None,
        provider: "auto".to_string(),
        category: "sub".to_string(),
        quality: "720p".to_string(),
        status: "DOWNLOADING".to_string(),
        progress: 0.0,
        file_path: None,
        file_size: None,
        duration_s: Some(4.0),
        error_msg: None,
        created_at: 0,
        updated_at: 0,
        audio_lang: None,
        sub_lang: None,
    }
}

#[test]
fn stale_working_files_are_found_one_and_two_levels_down() {
    let root = TempDir::new("stale");
    let ep = root.path().join("Show").join("Episode 01");
    let stale = [
        root.path().join("top.download.anix-part"),
        root.path().join("Show").join("one.download.anix-part"),
        ep.join("two.sub0.anix-part"),
        ep.join("old.anilili-part"),
    ];
    for f in &stale {
        touch(f);
    }
    let hls = ep.join("x.0011.hls.anix-part");
    touch(&hls.join("seg0.ts"));
    let finished = [ep.join("Show - EP 01.mkv"), ep.join("Show - EP 01.english.ass")];
    for f in &finished {
        touch(f);
    }
    let too_deep = ep.join("deeper").join("three.download.anix-part");
    touch(&too_deep);

    let removed = remove_stale_part_files(root.path(), &[]);

    assert_eq!(removed, 5);
    for f in &stale {
        assert!(!f.exists(), "{} should be gone", f.display());
    }
    assert!(!hls.exists());
    for f in &finished {
        assert!(f.exists(), "{} must stay", f.display());
    }
    assert!(too_deep.exists(), "three levels down is not searched");
}

#[test]
fn resumable_files_of_queued_downloads_are_kept_in_the_episode_folder() {
    let root = TempDir::new("keep");
    let rec = record("queued", "Show", 2.0);
    let ep = output_dir(root.path(), &rec.series_title, rec.episode_num);
    let kept = work_file_path(&ep, &rec, "download");
    let other = record("gone", "Show", 2.0);
    let dropped = work_file_path(&ep, &other, "download");
    touch(&kept);
    touch(&dropped);

    remove_stale_part_files(root.path(), &[work_tag(&rec.id)]);

    assert!(kept.exists(), "a queued download resumes from its file");
    assert!(!dropped.exists());
}

#[test]
fn the_part_files_of_a_queued_big_download_survive_the_startup_clean_up() {
    let root = TempDir::new("keep-parts");
    let rec = record("queued-big", "Show", 3.0);
    let other = record("gone-big", "Show", 3.0);
    let ep = output_dir(root.path(), &rec.series_title, rec.episode_num);
    let part = |r: &DownloadRecord, i| downloads::engine::part_path(&work_file_path(&ep, r, "download"), i);
    let (kept, dropped) = ([part(&rec, 0), part(&rec, 3)], part(&other, 0));
    for f in kept.iter().chain([&dropped]) {
        touch(f);
    }

    remove_stale_part_files(root.path(), &[work_tag(&rec.id)]);

    assert!(kept.iter().all(|f| f.exists()), "a big download killed mid-way resumes its parts");
    assert!(!dropped.exists());
}

#[test]
fn try_again_clears_the_resume_files_of_the_episode_folder() {
    let root = TempDir::new("retry");
    let rec = record("retry", "Show", 12.0);
    let ep = output_dir(root.path(), &rec.series_title, rec.episode_num);
    let files: Vec<PathBuf> = ["download", "resume"].iter().map(|r| work_file_path(&ep, &rec, r)).collect();
    for f in &files {
        touch(f);
    }
    let video = ep.join(format!("{}.mkv", output_base_name("Show", 12.0, "sub", "720p")));
    touch(&video);
    // Files already made in the flat layout are still cleared.
    let flat = work_file_path(root.path(), &rec, "resume");
    touch(&flat);

    let removed = remove_resume_files(root.path(), &rec);

    assert_eq!(removed, 3);
    assert!(files.iter().all(|f| !f.exists()) && !flat.exists());
    assert!(video.exists());
}

// ---------------------------------------------------------------------------------------
// Deleting
// ---------------------------------------------------------------------------------------

#[test]
fn deleting_removes_the_empty_episode_folder_then_the_empty_title_folder() {
    let root = TempDir::new("delete");
    let ep = root.path().join("Show").join("Episode 03");
    let video = ep.join("Show - EP 03 [SUB 720p].mkv");
    touch(&video);
    touch(&ep.join("Show - EP 03 [SUB 720p].english.ass"));
    touch(&ep.join("Show - EP 03 [SUB 720p].spanish.srt"));

    let removed = delete_download_files(&video, root.path()).unwrap();

    assert_eq!(removed, 3);
    assert!(!root.path().join("Show").exists(), "both folders were empty");
    assert!(root.path().exists(), "the download folder itself is never removed");
}

#[test]
fn deleting_keeps_folders_that_still_hold_something() {
    let root = TempDir::new("delete-keep");
    let show = root.path().join("Show");
    let video3 = show.join("Episode 03").join("a.mkv");
    let video4 = show.join("Episode 04").join("b.mkv");
    touch(&video3);
    touch(&video4);
    delete_download_files(&video3, root.path()).unwrap();
    assert!(!show.join("Episode 03").exists());
    assert!(video4.exists(), "the other episode stays, and so does the title folder");

    // A foreign file in the episode folder keeps it, and the title folder.
    let video5 = show.join("Episode 05").join("c.mkv");
    let note = show.join("Episode 05").join("notes.txt");
    touch(&video5);
    touch(&note);
    delete_download_files(&video5, root.path()).unwrap();
    assert!(note.exists() && show.join("Episode 05").is_dir());
}

#[test]
fn deleting_never_removes_folders_outside_the_download_folder_or_the_folder_itself() {
    let root = TempDir::new("delete-root");
    let elsewhere = TempDir::new("delete-else");
    // A download made before the folder was changed: its path is outside `root`.
    let old = elsewhere.path().join("Show").join("Episode 01").join("a.mkv");
    touch(&old);
    delete_download_files(&old, root.path()).unwrap();
    assert!(!old.exists());
    assert!(elsewhere.path().join("Show").join("Episode 01").is_dir(), "outside the download folder: left alone");

    // A flat download (old layout) directly in the download folder: the folder stays.
    let flat = root.path().join("Old - EP 01 [SUB 720p].mkv");
    touch(&flat);
    delete_download_files(&flat, root.path()).unwrap();
    assert!(root.path().is_dir());
}

#[test]
fn a_video_that_is_already_gone_still_tidies_its_empty_folders() {
    let root = TempDir::new("delete-gone");
    let video = root.path().join("Show").join("Episode 01").join("a.mkv");
    std::fs::create_dir_all(video.parent().unwrap()).unwrap();
    assert_eq!(delete_download_files(&video, root.path()).unwrap(), 0);
    assert!(!root.path().join("Show").exists());
}

#[test]
fn deleting_still_refuses_what_is_not_a_download() {
    let root = TempDir::new("delete-refuse");
    let doc = root.path().join("Show").join("Episode 01").join("notes.txt");
    touch(&doc);
    assert!(delete_download_files(&doc, root.path()).is_err());
    assert!(delete_download_files(Path::new("relative/a.mkv"), root.path()).is_err());
    assert!(doc.exists());
}

// ---------------------------------------------------------------------------------------
// End to end: a download into the new layout with several subtitle tracks on offer
// ---------------------------------------------------------------------------------------

struct Served {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

async fn serve(files: HashMap<String, Vec<u8>>, content: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    let files = Arc::new(files);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let (files, log) = (files.clone(), log.clone());
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 2048];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&head).to_string();
                let path = text.split(' ').nth(1).unwrap_or("/").to_string();
                log.lock().unwrap().push(path.clone());
                let r = match files.get(path.trim_start_matches('/')) {
                    Some(body) => Served { status: 200, content_type: content, body: body.clone() },
                    None => Served { status: 404, content_type: "text/plain", body: b"no".to_vec() },
                };
                let header = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    r.status, r.content_type, r.body.len()
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&r.body).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    downloads::allow_private_origin_for_tests(&base);
    (base, seen)
}

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn make_hls(dir: &Path) -> Option<HashMap<String, Vec<u8>>> {
    let status = std::process::Command::new("ffmpeg")
        .current_dir(dir)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=duration=4:size=160x120:rate=10",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
            "-c:v", "mpeg2video", "-c:a", "mp2",
            "-f", "hls", "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", "seg%d.jpg", "index.m3u8",
        ])
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let mut files = HashMap::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        files.insert(entry.file_name().to_string_lossy().to_string(), std::fs::read(entry.path()).ok()?);
    }
    files.contains_key("index.m3u8").then_some(files)
}

fn ffmpeg_info(file: &Path) -> String {
    let out = std::process::Command::new("ffmpeg").args(["-hide_banner", "-i"]).arg(file).output().unwrap();
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    v.sort();
    v
}

/// Runs one HLS download with four subtitle tracks on offer and returns what was saved.
async fn run(wanted_languages: Option<Vec<String>>) -> Option<(TempDir, DownloadOutcome, Vec<String>, sqlx::SqlitePool)> {
    english_ui::english_ui();
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return None;
    }
    let media = TempDir::new("media");
    let Some(mut files) = make_hls(media.path()) else {
        eprintln!("skipped: this ffmpeg cannot generate the test stream");
        return None;
    };
    let vtt = |text: &str| format!("WEBVTT\n\n00:00:00.500 --> 00:00:02.000\n{}\n", text).into_bytes();
    for (name, text) in [("en", "Hello"), ("es", "Hola"), ("fr", "Salut"), ("de", "Hallo")] {
        files.insert(format!("subs/{}.vtt", name), vtt(text));
    }
    let files: HashMap<String, Vec<u8>> =
        files.into_iter().map(|(k, v)| (if k.contains('/') { k } else { format!("hls/{}", k) }, v)).collect();
    let (base, seen) = serve(files, "application/octet-stream").await;

    let item = |name: &str, language: &str, is_default: bool| SubtitleItem {
        url: format!("{}/subs/{}.vtt", base, name),
        language: language.to_string(),
        format: "vtt".to_string(),
        is_default: Some(is_default),
    };
    let source = DownloadSource {
        url: format!("{}/hls/index.m3u8", base),
        stream_type: Some("hls".to_string()),
        subtitles: vec![item("fr", "French", true), item("de", "German", false), item("en", "English", false), item("es", "Spanish", false)],
        subtitle_languages: wanted_languages,
        ..DownloadSource::default()
    };
    let rec = record("layout-e2e", "Test Show", 3.0);
    let pool = db::init_db_in_memory().await.unwrap();
    db::create_download(&pool, &rec).await.unwrap();

    let root = TempDir::new("root");
    let outcome = downloads::download_under(&pool, &rec, source, root.path()).await;
    let requested = seen.lock().unwrap().iter().filter(|p| p.starts_with("/subs/")).cloned().collect();
    Some((root, outcome, requested, pool))
}

#[tokio::test]
async fn only_the_preferred_languages_are_fetched_and_the_first_is_embedded() {
    let Some((root, outcome, requested, _pool)) = run(Some(vec!["en".into(), "es".into()])).await else { return };

    let DownloadOutcome::Completed { path, .. } = outcome else { panic!("got {:?}", outcome) };
    let folder = root.path().join("Test Show").join("Episode 03");
    assert_eq!(path, folder.join("Test Show - EP 03 [SUB 720p].mkv"));
    assert_eq!(
        names_in(&folder),
        vec![
            "Test Show - EP 03 [SUB 720p].english.vtt",
            "Test Show - EP 03 [SUB 720p].mkv",
            "Test Show - EP 03 [SUB 720p].spanish.vtt"
        ],
        // The embedded first language is also a side file: the web player reads only those.
        "no working files, no other languages"
    );
    assert_eq!(names_in(root.path()), vec!["Test Show"], "nothing flat in the download folder");
    assert_eq!(names_in(&root.path().join("Test Show")), vec!["Episode 03"]);

    let info = ffmpeg_info(&path);
    assert_eq!(info.matches("Subtitle:").count(), 1, "{}", info);
    assert!(info.contains("English") && !info.contains("French"), "English is the embedded track:\n{}", info);

    let mut requested = requested;
    requested.sort();
    assert_eq!(requested, ["/subs/en.vtt", "/subs/es.vtt"], "the other tracks were never requested");
}

#[tokio::test]
async fn without_a_preferred_match_only_the_default_track_is_used() {
    let Some((root, outcome, requested, _pool)) = run(Some(vec!["ja".into()])).await else { return };
    let DownloadOutcome::Completed { path, .. } = outcome else { panic!("got {:?}", outcome) };
    assert_eq!(
        names_in(path.parent().unwrap()),
        vec!["Test Show - EP 03 [SUB 720p].french.vtt", "Test Show - EP 03 [SUB 720p].mkv"]
    );
    assert!(ffmpeg_info(&path).contains("French"));
    assert_eq!(requested, ["/subs/fr.vtt"]);
    drop(root);
}

#[tokio::test]
async fn the_all_languages_switch_keeps_todays_behaviour() {
    let Some((root, outcome, requested, _pool)) = run(None).await else { return };
    let DownloadOutcome::Completed { path, .. } = outcome else { panic!("got {:?}", outcome) };
    // The source's default (French) is embedded and a side file too; so are the other three.
    assert_eq!(names_in(path.parent().unwrap()).len(), 5);
    assert_eq!(requested.len(), 4);
    drop(root);
}

#[tokio::test]
async fn the_first_preferred_language_is_a_side_file_and_stored_as_the_sub_language() {
    let Some((root, outcome, _, pool)) = run(Some(vec!["es".into(), "fr".into()])).await else { return };
    let DownloadOutcome::Completed { path, .. } = outcome else { panic!("got {:?}", outcome) };
    let spanish = path.with_file_name("Test Show - EP 03 [SUB 720p].spanish.vtt");
    assert!(spanish.exists(), "the embedded first language also exists as a side file");
    assert!(ffmpeg_info(&path).contains("Spanish"));
    // French is the source's default flag but second in the user's order: the first is embedded.
    let stored = db::get_download_by_id(&pool, "layout-e2e").await.unwrap().unwrap();
    assert_eq!(stored.sub_lang.as_deref(), Some("es"));
    drop(root);
}

#[test]
fn cleaning_up_a_cancelled_download_removes_its_empty_folders_and_only_those() {
    let root = TempDir::new("prune");
    let ep = root.path().join("Show").join("Episode 01");
    std::fs::create_dir_all(&ep).unwrap();
    let keep = root.path().join("Other").join("Episode 01");
    std::fs::create_dir_all(&keep).unwrap();
    touch(&keep.join("v.mkv"));
    downloads::prune_empty_folders(&ep, root.path());
    assert!(!root.path().join("Show").exists());
    downloads::prune_empty_folders(&keep, root.path());
    assert!(keep.join("v.mkv").exists());
    // never the download folder itself, nor anything through `..`
    downloads::prune_empty_folders(root.path(), root.path());
    downloads::prune_empty_folders(&root.path().join("Other").join("..").join(".."), root.path());
    assert!(root.path().exists());
}

#[test]
fn startup_clean_up_removes_the_folders_it_emptied() {
    let root = TempDir::new("stale-prune");
    let rec = record("dead", "Show", 5.0);
    let ep = output_dir(root.path(), &rec.series_title, rec.episode_num);
    touch(&work_file_path(&ep, &rec, "download"));
    std::fs::create_dir_all(root.path().join("Untouched")).unwrap();

    remove_stale_part_files(root.path(), &[]);

    assert!(!root.path().join("Show").exists());
    assert!(root.path().join("Untouched").exists(), "folders it did not empty stay");
}

#[test]
fn removing_the_kept_files_of_a_failed_try_removes_the_empty_folders() {
    let root = TempDir::new("retry-prune");
    let rec = record("failed", "Show", 6.0);
    let ep = output_dir(root.path(), &rec.series_title, rec.episode_num);
    touch(&work_file_path(&ep, &rec, "resume"));
    remove_resume_files(root.path(), &rec);
    assert!(!root.path().join("Show").exists());
}

#[test]
fn a_download_queued_under_1_4_1_resumes_in_its_flat_folder() {
    let root = TempDir::new("flat-resume");
    let rec = record("old", "Show", 7.0);
    assert_eq!(downloads::download_folder(root.path(), &rec), output_dir(root.path(), "Show", 7.0));
    touch(&work_file_path(root.path(), &rec, "resume"));
    assert_eq!(downloads::download_folder(root.path(), &rec), root.path());
    // another record's flat partial does not pull this one flat
    assert_eq!(
        downloads::download_folder(root.path(), &record("other", "Show", 8.0)),
        output_dir(root.path(), "Show", 8.0)
    );
}

#[test]
fn part_suffix_constant_is_unchanged() {
    assert_eq!(PART_SUFFIX, ".anix-part");
}

#[test]
fn an_episode_folder_a_second_job_still_uses_is_not_pruned() {
    let root = TempDir::new("busy");
    let ep = root.path().join("Show").join("Episode 01");
    std::fs::create_dir_all(&ep).unwrap();
    // a sub and a dub of one episode run at once
    downloads::claim_episode_folder(&ep);
    downloads::claim_episode_folder(&ep);
    downloads::release_episode_folder(&ep, root.path());
    assert!(ep.exists(), "the other job still uses the folder");
    downloads::release_episode_folder(&ep, root.path());
    assert!(!root.path().join("Show").exists(), "the last one out removes the empty folders");
}

#[test]
fn the_old_folder_pass_leaves_a_current_folder_inside_an_old_one_alone() {
    let root = TempDir::new("old-around-current");
    let current = root.path().join("Anix");
    let ep = current.join("Show").join("Episode 01");
    std::fs::create_dir_all(&ep).unwrap();
    let part = |dir: &Path, name: &str| dir.join(format!("{}.{}.download{}", name, work_tag("q"), PART_SUFFIX));
    touch(&part(&ep, "queued"));
    touch(&part(root.path(), "old"));
    let list = root.path().join("dirs.txt");
    downloads::remember_work_dir(&list, root.path());
    assert_eq!(downloads::remove_part_files_in_old_dirs(&list, &current), 1);
    assert!(!part(root.path(), "old").exists());
    assert!(part(&ep, "queued").exists(), "the current folder keeps its partials");
}

#[test]
fn pruning_skips_a_folder_a_running_download_holds() {
    let root = TempDir::new("busy-prune");
    let ep = root.path().join("Held").join("Episode 01");
    std::fs::create_dir_all(&ep).unwrap();
    downloads::claim_episode_folder(&ep);
    // what delete and Try again clean-up call
    downloads::prune_empty_folders(&ep, root.path());
    assert!(ep.exists());
    downloads::release_episode_folder(&ep, root.path());
    assert!(!root.path().join("Held").exists());
}
