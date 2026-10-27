//! Programs Anix starts on the host (the browser for sign-in and links, mpv, VLC, ffmpeg) must
//! not inherit the paths the AppImage launcher points at its bundle. Kept in its own file:
//! it changes this test process's environment.

use anix_lib::commands::sanitized_command;

#[test]
fn host_programs_start_without_the_bundle_environment() {
    let appdir = "/tmp/anix-fake-appdir";
    std::env::set_var("APPDIR", appdir);
    std::env::set_var("PATH", format!("{appdir}/usr/bin:/usr/bin:/bin"));
    std::env::set_var("XDG_DATA_DIRS", format!("{appdir}/usr/share:/usr/share"));
    for name in ["GTK_PATH", "GIO_EXTRA_MODULES", "GDK_BACKEND", "GTK_THEME", "GI_TYPELIB_PATH", "GST_PTP_HELPER_1_0", "GTK_DATA_PREFIX"] {
        std::env::set_var(name, format!("{appdir}/x"));
    }

    let out = sanitized_command("env").output().expect("env runs");
    let env = String::from_utf8_lossy(&out.stdout);
    let leaked: Vec<&str> = env
        .lines()
        .filter(|l| l.contains(appdir) && !l.starts_with("APPDIR="))
        .collect();
    assert!(leaked.is_empty(), "bundle paths reached the host program: {leaked:?}");
    assert!(env.lines().any(|l| l == "PATH=/usr/bin:/bin"));
    assert!(env.lines().any(|l| l == "XDG_DATA_DIRS=/usr/share"));
}
