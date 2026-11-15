// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Release checks only: no window, no webview.
    if std::env::args().any(|arg| arg == anix_lib::system::BACKGROUND_ARG) {
        std::process::exit(anix_lib::system::background::run());
    }

    #[cfg(target_os = "linux")]
    {
        // Covers load over TLS outside Fedora too (see system/gnutls.rs).
        anix_lib::system::gnutls::apply();

        // After two crashes in a row: the web player, no compositing (see system/safe_mode.rs).
        anix_lib::system::safe_mode::apply(anix_lib::db::app_dir_path().as_deref());

        // WebKit's DMA-BUF renderer blanks the window with some NVIDIA drivers; everywhere else it
        // stays on (off, scrolling drops to about 1 frame a second). Embedded mpv always needs it,
        // and on NVIDIA the user can turn it on for the web player too (Settings › System).
        let wants_mpv = anix_lib::embedded_mpv::wants_embedded_player();
        let fast_drawing = anix_lib::system::fast_drawing::decide_at_start(wants_mpv);
        anix_lib::system::hwaccel::apply_dmabuf_workaround(wants_mpv, fast_drawing);
    }

    anix_lib::run();
}
