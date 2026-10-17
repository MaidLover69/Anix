use anix_lib::commands::{read_subtitle_file, start_arg};

fn temp_file(name: &str, body: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("anix_drop_{}_{}", std::process::id(), name));
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn mpv_starts_only_at_a_real_resume_position() {
    assert_eq!(start_arg(Some(754.25)).as_deref(), Some("--start=754.250"));
    assert_eq!(start_arg(Some(0.4)), None);
    assert_eq!(start_arg(Some(f64::NAN)), None);
    assert_eq!(start_arg(Some(-30.0)), None);
    assert_eq!(start_arg(None), None);
}

#[tokio::test]
async fn a_dropped_srt_file_becomes_webvtt() {
    let path = temp_file("a.srt", b"1\r\n00:00:01,000 --> 00:00:02,500\r\nHello\r\n");
    let vtt = read_subtitle_file(path.display().to_string()).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(vtt.starts_with("WEBVTT"));
    assert!(vtt.contains("00:00:01.000 --> 00:00:02.500"));
}

#[tokio::test]
async fn a_dropped_ass_file_in_a_legacy_encoding_is_read() {
    let ass = b"[Script Info]\nTitle: x\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,Caf\xe9\n";
    let path = temp_file("b.ASS", ass);
    let vtt = read_subtitle_file(path.display().to_string()).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(vtt.contains("00:00:03.000 --> 00:00:04.000\nCafé"));
}

#[tokio::test]
async fn other_files_are_refused() {
    let text = temp_file("c.txt", b"00:00:01,000 --> 00:00:02,000\nx\n");
    assert!(read_subtitle_file(text.display().to_string()).await.is_err());
    std::fs::remove_file(&text).unwrap();

    let empty = temp_file("d.srt", b"not a subtitle");
    assert!(read_subtitle_file(empty.display().to_string()).await.is_err());
    std::fs::remove_file(&empty).unwrap();

    assert!(read_subtitle_file("relative.srt".to_string()).await.is_err());
}
