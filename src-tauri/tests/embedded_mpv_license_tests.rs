//! Issue #9, part 3: when the embedded player may load a libmpv built on a GPL FFmpeg.
//! The AppImage bundles its own LGPL libmpv and keeps refusing a GPL one. System packages
//! (RPM/DEB/AUR) bundle none, so the user's own libmpv is allowed whatever its licence.
//!
//! Seams (in `anix_lib::embedded_mpv`):
//! - `license_refusal(license: &str, bundled: bool) -> Option<String>`: `Some(message)` when
//!   the library must not be used. `bundled` is true when the build ships its own libmpv
//!   (the AppImage layout), false for a system library. `load_lib` calls it in release builds.
//! - `no_libmpv_message() -> String`: shown (as the web-player reason) when no libmpv loads.

use anix_lib::embedded_mpv::{license_refusal, no_libmpv_message};

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
