//! Issue #9, part 3: when the embedded player may load a libmpv built on a GPL FFmpeg.
//! The AppImage bundles its own LGPL libmpv and keeps refusing a GPL one. System packages
//! (RPM/DEB/AUR) bundle none, so the user's own libmpv is allowed whatever its licence.
//!
//! Seams (in `anix_lib::embedded_mpv`):
//! - `license_refusal(license: &str, bundled: bool) -> Option<String>`: `Some(message)` when
//!   the library must not be used. `bundled` is true when the build ships its own libmpv
//!   (the AppImage layout), false for a system library. `load_lib` calls it in release builds.
//! - `no_libmpv_message() -> String`: shown (as the web-player reason) when no libmpv loads.

use anix_lib::embedded_mpv::{default_engine, quiet_no_libmpv, libmpv_is_bundled, license_refusal, no_libmpv_message, no_libmpv_message_for};

const GPL: &str = "GPL version 3 or later";
const LGPL: &str = "LGPL version 2.1 or later";

#[test]
fn a_bundled_libmpv_must_be_lgpl() {
    assert!(license_refusal(GPL, true).is_some());
    assert!(license_refusal("nonfree and unredistributable", true).is_some());
    assert!(license_refusal(LGPL, true).is_none());
}

#[test]
fn the_refusal_names_the_licence_in_plain_words() {
    let message = license_refusal(GPL, true).unwrap();
    assert!(message.contains(GPL), "{}", message);
    assert!(message.contains("web player"), "{}", message);
}

#[test]
fn a_system_libmpv_is_allowed_even_when_gpl() {
    assert!(license_refusal(GPL, false).is_none());
    assert!(license_refusal("nonfree and unredistributable", false).is_none());
    assert!(license_refusal(LGPL, false).is_none());
    assert!(license_refusal("", false).is_none());
}

#[test]
fn without_any_libmpv_the_message_explains_the_way_forward() {
    let message = no_libmpv_message();
    assert!(message.contains("AppImage"), "{}", message);
    assert!(message.contains("mpv"), "{}", message);
    assert!(message.contains("web player"), "{}", message);
    assert!(message.contains("Open in mpv"), "points to the external player: {}", message);
    assert!(!message.contains("not an LGPL"), "{}", message);
    assert!(!message.contains("could not be loaded"), "no raw dlopen error: {}", message);
}

#[test]
fn the_flatpak_message_suggests_neither_a_system_libmpv_nor_open_in_mpv() {
    let message = no_libmpv_message_for(true);
    assert!(message.contains("web player"), "{}", message);
    assert!(!message.contains("Open in mpv"), "{}", message);
    assert!(!message.contains("installed"), "{}", message);
    assert!(no_libmpv_message_for(false).contains("Open in mpv"));
}

#[test]
fn an_install_that_never_chose_uses_mpv_only_where_it_can() {
    // debug builds and the AppImage: mpv without asking whether a libmpv loads
    assert_eq!(default_engine(false, true, false, false, || false), "mpv");
    assert_eq!(default_engine(false, false, true, false, || false), "mpv");
    // RPM/DEB/AUR: mpv when the system's libmpv loads, else quietly the web player
    assert_eq!(default_engine(false, false, false, false, || true), "mpv");
    assert_eq!(default_engine(false, false, false, false, || false), "web");
    // the Flatpak never loads a system libmpv
    assert_eq!(default_engine(false, false, false, true, || true), "web");
}

#[test]
fn a_forced_web_run_never_probes_libmpv() {
    for (debug, appimage) in [(false, false), (true, false), (false, true)] {
        let engine = default_engine(true, debug, appimage, false, || panic!("libmpv was probed"));
        assert_eq!(engine, "web");
    }
}

#[test]
fn a_missing_system_libmpv_is_quiet_but_a_failing_bundled_one_is_not() {
    assert!(quiet_no_libmpv(false, false));
    assert!(!quiet_no_libmpv(true, false));
    assert!(!quiet_no_libmpv(false, true));
}

#[test]
fn an_appimage_without_its_own_libmpv_is_still_bundled() {
    assert!(libmpv_is_bundled(true, false));
    assert!(libmpv_is_bundled(true, true));
    assert!(libmpv_is_bundled(false, true));
    assert!(!libmpv_is_bundled(false, false));
}
