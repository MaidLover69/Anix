// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Fix white/blank screen on modern Linux distros (Fedora/Ubuntu/Arch) with WebKitGTK 2.42+
        let set_by_app = std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err();
        if set_by_app {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        anix_lib::system::hwaccel::note_dmabuf_workaround(set_by_app);
    }

    anix_lib::run();
}
