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

        // Fix white/blank screen on modern Linux distros (Fedora/Ubuntu/Arch) with WebKitGTK 2.42+,
        // except for the embedded mpv player, which needs WebKit's DMA-BUF renderer to draw the
        // page over the video.
        anix_lib::system::hwaccel::apply_dmabuf_workaround(anix_lib::embedded_mpv::wants_embedded_player());
    }

    anix_lib::run();
}
