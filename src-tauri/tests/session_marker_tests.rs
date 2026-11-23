//! The next start can tell that the last session did not exit cleanly, how many did in a
//! row, and when that calls for safe mode.

use anix_lib::system::safe_mode;
use anix_lib::system::session_marker::{abandon, begin, end, peek, FILE_NAME};

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

#[test]
fn compositing_counts_as_off_for_any_value_of_the_variable_but_zero() {
    use std::ffi::OsStr;
    // WebKitGTK's own rule; the page drops view transitions then, which abort the page without it.
    assert!(!safe_mode::compositing_off_value(None));
    assert!(!safe_mode::compositing_off_value(Some(OsStr::new("0"))));
    assert!(safe_mode::compositing_off_value(Some(OsStr::new("1"))));
    assert!(safe_mode::compositing_off_value(Some(OsStr::new("true"))));
    assert!(safe_mode::compositing_off_value(Some(OsStr::new(""))));
}

#[test]
fn a_marker_from_another_boot_or_login_session_is_no_crash() {
    let dir = scratch("logout");
    // Left by a session that ended with a logout or shutdown, then counted on from there.
    std::fs::write(dir.join(FILE_NAME), "4242 3 another-boot 7").unwrap();
    assert_eq!(peek(&dir), 0);
    assert_eq!(begin(&dir), 0);
    assert_eq!(begin(&dir), 1, "a crash in this session still counts");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_second_launch_keeps_the_running_anix_marker() {
    let dir = scratch("second");
    let fake = dir.join("anix");
    std::fs::copy("/usr/bin/sleep", &fake).unwrap();
    let mut running = std::process::Command::new(&fake).arg("30").spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let marker = format!("{} 0", running.id());
    std::fs::write(dir.join(FILE_NAME), &marker).unwrap();
    begin(&dir);
    let kept = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
    running.kill().unwrap();
    running.wait().unwrap();
    assert_eq!(kept, marker);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_start_that_gives_up_keeps_the_earlier_crash() {
    let dir = scratch("dbfail");
    assert_eq!(begin(&dir), 0);
    // That session crashed; the next start cannot open the database and quits.
    let unclean = begin(&dir);
    assert_eq!(unclean, 1);
    abandon(&dir, unclean);
    end(&dir);
    assert_eq!(peek(&dir), 1, "the crash before it is still reported");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn only_the_session_that_wrote_the_marker_removes_it() {
    let dir = scratch("owner");
    std::fs::write(dir.join(FILE_NAME), "4242 0").unwrap();
    end(&dir);
    assert_eq!(peek(&dir), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_ignored_hangup_is_seen_as_ignored() {
    // Nothing else in this test binary touches SIGHUP.
    let before = unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
    assert!(anix_lib::system::sighup_ignored(), "as under nohup");
    unsafe { libc::signal(libc::SIGHUP, libc::SIG_DFL) };
    assert!(!anix_lib::system::sighup_ignored());
    unsafe { libc::signal(libc::SIGHUP, before) };
}

#[test]
fn compositing_goes_off_only_after_three_unclean_exits_in_a_row() {
    // Arrange / Act / Assert
    for n in [0, 1, 2] {
        assert!(!safe_mode::compositing_needed(n), "{n} crashes keep compositing on");
    }
    for n in [3, 4, 9] {
        assert!(safe_mode::compositing_needed(n), "{n} crashes turn compositing off");
    }
    assert_eq!(safe_mode::COMPOSITING_STARTS, 3);
    assert!(safe_mode::COMPOSITING_STARTS > safe_mode::UNCLEAN_STARTS);
}

#[test]
fn the_compositing_switch_only_touches_what_safe_mode_owns() {
    use safe_mode::{switch_env, Switch};
    let cases = [
        (None, None, true, Switch::Set),
        (None, None, false, Switch::Untouched),
        (Some("1"), Some("1"), false, Switch::Clear),
        (Some("1"), Some("1"), true, Switch::Untouched),
        (Some("1"), None, false, Switch::User),
        (Some("1"), None, true, Switch::User),
        (Some("0"), Some("1"), true, Switch::User),
        (Some(""), None, true, Switch::User),
        (None, Some("1"), false, Switch::Untouched),
    ];
    for (value, mark, wanted, expected) in cases {
        assert_eq!(switch_env(value, mark, wanted), expected, "{value:?} {mark:?} wanted={wanted}");
    }
}

#[test]
fn a_dmabuf_value_set_by_safe_mode_is_removed_by_a_normal_start_and_is_not_the_users() {
    use anix_lib::system::hwaccel::{self, DmabufEnv};
    // Arrange: safe mode reuses hwaccel's own mark for the DMA-BUF switch.
    let (value, mark) = (Some("1"), Some("1"));

    // Act / Assert
    assert_eq!(hwaccel::dmabuf_env(value, mark, true), DmabufEnv::Clear);
    assert!(!hwaccel::dmabuf_off_by_user(value, mark));
}

#[test]
fn compositing_switch_follows_the_crash_count_and_ownership() {
    use safe_mode::{compositing_switch as sw, Switch};
    let cases = [
        (2, None, None, Switch::Untouched), // step 1: compositing stays on
        (3, None, None, Switch::Set),
        (9, None, None, Switch::Set),
        (2, Some("1"), Some("1"), Switch::Clear), // inherited from a step-2 run
        (0, Some("1"), Some("1"), Switch::Clear),
        (3, Some("1"), Some("1"), Switch::Untouched),
        (0, Some("1"), None, Switch::User),
        (2, Some("1"), None, Switch::User),
        (3, Some("0"), None, Switch::User),
    ];
    for (n, value, mark, expected) in cases {
        assert_eq!(sw(n, value, mark), expected, "{n} {value:?} {mark:?}");
    }
}

#[test]
fn a_non_utf8_user_value_is_read_as_the_users_and_left_alone() {
    use anix_lib::system::hwaccel::{dmabuf_env, env_text, DmabufEnv};
    use safe_mode::{switch_env, Switch};
    use std::os::unix::ffi::OsStrExt;
    // Arrange
    let raw = std::ffi::OsStr::from_bytes(b"\xff");

    // Act
    let text = env_text(Some(raw));

    // Assert
    assert!(text.as_deref().is_some_and(|t| t != "1"));
    assert_eq!(switch_env(text.as_deref(), None, true), Switch::User);
    assert_eq!(dmabuf_env(text.as_deref(), None, false), DmabufEnv::User);
    assert_eq!(env_text(None), None);
}
