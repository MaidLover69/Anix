//! The log file keeps the lines of the session before a crash: a full log moves aside to
//! `anix.log.1` instead of being emptied.

use std::io::Write;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("anix-log-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_small_log_is_appended_to() {
    let dir = scratch("small");
    let path = dir.join("anix.log");
    std::fs::write(&path, "first\n").unwrap();
    writeln!(anix_lib::open_log(&path, 1024).unwrap(), "second").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "first\nsecond\n");
    assert!(!dir.join("anix.log.1").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_full_log_moves_to_dot_one_and_a_new_one_starts() {
    let dir = scratch("full");
    let path = dir.join("anix.log");
    std::fs::write(dir.join("anix.log.1"), "oldest\n").unwrap();
    std::fs::write(&path, "before the crash\n").unwrap();
    writeln!(anix_lib::open_log(&path, 4).unwrap(), "new session").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new session\n");
    assert_eq!(std::fs::read_to_string(dir.join("anix.log.1")).unwrap(), "before the crash\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_log_is_created() {
    let dir = scratch("missing");
    let path = dir.join("anix.log");
    assert!(anix_lib::open_log(&path, 4).is_some());
    assert!(path.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
