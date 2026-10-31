//! The next start can tell that the last session did not exit cleanly, how many did in a
//! row, and when that calls for safe mode.

use anix_lib::system::safe_mode;
use anix_lib::system::session_marker::{begin, end, peek, FILE_NAME};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("anix-marker-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_clean_exit_leaves_nothing_to_report() {
    let dir = scratch("clean");
    assert_eq!(begin(&dir), 0, "the first start has nothing to report");
    end(&dir);
    assert!(!dir.join(FILE_NAME).exists());
    assert_eq!(begin(&dir), 0, "a start after a clean exit has nothing to report");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_session_that_never_exited_is_reported_once() {
    let dir = scratch("crash");
    assert_eq!(begin(&dir), 0);
    // The process died here: no end().
    assert_eq!(begin(&dir), 1, "the next start sees the marker");
    end(&dir);
    assert_eq!(begin(&dir), 0, "and the start after a clean exit does not");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ending_without_a_marker_is_harmless() {
    let dir = scratch("none");
    end(&dir);
    assert!(!dir.join(FILE_NAME).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn crashes_in_a_row_are_counted_until_a_clean_exit() {
    let dir = scratch("loop");
    assert_eq!(begin(&dir), 0);
    assert_eq!(begin(&dir), 1);
    assert_eq!(peek(&dir), 2, "peek reports what the next start would see");
    assert_eq!(peek(&dir), 2, "and writes nothing");
    assert_eq!(begin(&dir), 2);
    assert_eq!(begin(&dir), 3);
    end(&dir);
    assert_eq!(peek(&dir), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_marker_from_before_the_count_counts_as_one_crash() {
    let dir = scratch("old");
    // 1.4 builds before the count wrote only the pid.
    std::fs::write(dir.join(FILE_NAME), "4242").unwrap();
    assert_eq!(peek(&dir), 1);
    assert_eq!(begin(&dir), 1);
    assert_eq!(begin(&dir), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn safe_mode_starts_after_two_unclean_exits_in_a_row() {
    assert!(!safe_mode::needed(0));
    assert!(!safe_mode::needed(1), "one crash only shows a notice");
    assert!(safe_mode::needed(2));
    assert!(safe_mode::needed(5));
    assert!(!safe_mode::active(), "nothing turns it on in tests");
}
