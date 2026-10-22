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

        // Fix white/blank screen on modern Linux distros (Fedora/Ubuntu/Arch) with WebKitGTK 2.42+
        let set_by_app = std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err();
        // The embedded mpv player needs the DMA-BUF renderer: without it WebKit does not draw
        // the page over the video.
        let keep_dmabuf = set_by_app && anix_lib::embedded_mpv::wants_embedded_player();
        if set_by_app && !keep_dmabuf {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        if !keep_dmabuf {
            anix_lib::system::hwaccel::note_dmabuf_workaround(set_by_app);
        }
    }

    anix_lib::run();
}
