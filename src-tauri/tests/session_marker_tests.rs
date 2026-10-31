//! The next start can tell that the last session did not exit cleanly.

use anix_lib::system::session_marker::{begin, end, FILE_NAME};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("anix-marker-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_clean_exit_leaves_nothing_to_report() {
    let dir = scratch("clean");
    assert!(!begin(&dir), "the first start has nothing to report");
    end(&dir);
    assert!(!dir.join(FILE_NAME).exists());
    assert!(!begin(&dir), "a start after a clean exit has nothing to report");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_session_that_never_exited_is_reported_once() {
    let dir = scratch("crash");
    assert!(!begin(&dir));
    // The process died here: no end().
    assert!(begin(&dir), "the next start sees the marker");
    end(&dir);
    assert!(!begin(&dir), "and the start after a clean exit does not");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ending_without_a_marker_is_harmless() {
    let dir = scratch("none");
    end(&dir);
    assert!(!dir.join(FILE_NAME).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
