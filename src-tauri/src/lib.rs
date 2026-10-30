pub mod accounts;
pub mod backup;
pub mod cache;
pub mod catalogue;
pub mod clients;
pub mod commands;
pub mod db;
pub mod diagnostics;
pub mod downloads;
pub mod embedded_mpv;
pub mod error;
pub mod i18n;
pub mod local_library;
pub mod local_media;
pub mod migration;
pub mod models;
pub mod mpris;
pub mod mpv_ipc;
pub mod notices;
pub mod notifier;
pub mod offline;
pub mod plugins;
pub mod providers;
pub mod release_check;
pub mod release_parser;
pub mod reminders;
pub mod state;
pub mod stream_proxy;
pub mod system;
pub mod web_session;

use state::AppState;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// An environment variable under its current name, or under the name it had before the app
/// was renamed (`ANILILI_…`), which keeps working.
pub fn env_var_os(name: &str, legacy_name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).or_else(|| std::env::var_os(legacy_name))
}

/// Sends backend logs to stderr in development and to a log file in packaged builds,
/// where nothing is attached to stderr.
pub(crate) fn init_logging() {
    let builder = tracing_subscriber::fmt().with_max_level(tracing::Level::INFO);

    if cfg!(debug_assertions) {
        let _ = builder.with_writer(std::io::stderr).try_init();
        return;
    }

    let log_file = db::get_app_dir().ok().and_then(|dir| {
        let path = dir.join(migration::LOG_FILE_NAME);
        // Start over once the file grows past 2 MB.
        let too_big = std::fs::metadata(&path).map(|m| m.len() > 2 * 1024 * 1024).unwrap_or(false);
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(!too_big)
            .truncate(too_big)
            .open(path)
            .ok()
    });

    let _ = match log_file {
        Some(file) => builder
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .try_init(),
        None => builder.with_writer(std::io::stderr).try_init(),
    };
}

pub fn run() {
    // Before anything else: the log file, the database and the webview's storage are all in
    // folders this renames when they still carry the app's earlier name. Tauri creates the
    // webview's folder while building the main window, which is before `setup` runs.
    let bases = migration::BaseDirs::system();
    let migrated = migration::migrate(&bases);
    init_logging();
    migrated.log();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch (for example from an anix:// link) brings the running window
            // forward, also when it is hidden in the tray.
            system::tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(move |app| {
            web_session::init(app.handle().clone());
            match tauri::async_runtime::block_on(db::init_db()) {
                Ok(pool) => {
                    match tauri::async_runtime::block_on(migration::repoint_download_paths(&pool, &bases)) {
                        Ok(0) => {}
                        Ok(count) => tracing::info!("{} downloads now point into the renamed data folder", count),
                        Err(e) => tracing::warn!("Download paths could not be checked after the rename: {}", e),
                    }
                    let state = AppState::new(pool);
                    plugins::start(app.handle().clone(), state.plugins.clone());
                    let registry = state.plugins.clone();
                    web_session::set_labeller(std::sync::Arc::new(move |site| {
                        Some(registry.site_label(site.key, site.label))
                    }));
                    app.manage(state);
                    accounts::spawn_sync(app.handle().clone());
                    downloads::spawn_queue_worker(app.handle().clone());
                    notices::init(app.handle().clone());
                    notifier::start(app.handle().clone());
                    system::apply(app.handle(), system::SystemSettings::read());
                    system::url_handler::register_for_appimage();
                    let args: Vec<String> = std::env::args().collect();
                    system::tray::hide_if_started_minimised(app.handle(), &args);
                    let pool = app.state::<AppState>().pool.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = offline::prune(&pool, db::current_unix_timestamp()).await {
                            tracing::warn!("Saved AniList data could not be pruned: {}", e);
                        }
                    });
                    if let Err(e) = stream_proxy::start() {
                        tracing::error!("{}", e);
                    }
                    embedded_mpv::install(app.handle());
                    #[cfg(target_os = "linux")]
                    system::page_crash::watch(app.handle());
                }
                Err(e) => {
                    tracing::error!("Failed to initialize database: {}", e);
                    let handle = app.handle().clone();
                    app.dialog()
                        .message(format!(
                            "Anix could not open its database and has to close.\n\n{}",
                            e
                        ))
                        .title("Anix")
                        .kind(MessageDialogKind::Error)
                        .show(move |_| handle.exit(1));
                }
            }
            Ok(())
        })
        .on_window_event(system::tray::on_window_event)
        // Only commands the frontend calls are exposed over IPC.
        .invoke_handler(tauri::generate_handler![
            // AniList
            commands::fetch_home_data,
            commands::search_anime,
            commands::fetch_anime_details,
            commands::get_anilist_viewer,
            commands::fetch_anilist_library,
            commands::save_anilist_entry,
            commands::get_offline_status,
            // Providers & Streams
            commands::get_all_episodes,
            commands::get_anime_catalog_episodes,
            commands::get_episode_sources,
            commands::get_site_verification_status,
            commands::open_site_verification,
            commands::get_skip_times,
            commands::get_filler_episodes,
            commands::fetch_mal_anime_synopsis,
            // Plugins
            commands::list_plugin_repos,
            commands::add_plugin_repo,
            commands::remove_plugin_repo,
            commands::refresh_plugin_repos,
            commands::list_plugins,
            commands::set_plugin_enabled,
            commands::get_plugin_config,
            commands::set_plugin_config,
            commands::test_plugin,
            commands::test_all_plugins,
            commands::get_plugin_health,
            commands::check_plugin_updates,
            commands::get_plugin_directory,
            // Library & History
            commands::get_watched_episodes,
            commands::get_episode_progress,
            commands::get_continue_watching,
            commands::get_watch_history,
            commands::update_watch_progress,
            commands::add_to_watchlist,
            commands::remove_from_watchlist,
            commands::is_in_watchlist,
            commands::get_watchlist,
            commands::set_episodes_watched,
            commands::get_watched_history,
            commands::get_watch_times,
            commands::set_list_status,
            commands::get_list_status,
            commands::sync_list_statuses,
            // Notification centre
            commands::list_notices,
            commands::get_unread_notice_count,
            commands::mark_notices_read,
            commands::clear_notices,
            commands::push_test_notice,
            // Backup & restore
            commands::export_backup,
            commands::import_backup,
            commands::fetch_anilist_stats,
            // Downloads
            commands::create_download,
            commands::start_episode_download,
            commands::retry_download,
            commands::get_all_downloads,
            commands::delete_download,
            commands::check_storage_for_download,
            commands::get_download_directory,
            commands::pick_download_directory,
            // Schedule & Notifications
            commands::fetch_airing_schedule,
            commands::save_notification_preference,
            commands::list_notification_preferences,
            commands::set_bell_lead_time,
            commands::list_bell_lead_times,
            // Settings
            commands::get_settings,
            commands::save_settings,
            // System
            commands::get_desktop_status,
            commands::get_hardware_report,
            commands::save_diagnostics_report,
            commands::open_log_folder,
            commands::get_desktop_appearance,
            // Player
            commands::launch_external_player,
            commands::read_subtitle_file,
            mpris::mpris_update,
            mpris::mpris_clear,
            // AnimeThemes & Local Media
            commands::fetch_anime_themes,
            commands::scan_local_anime_directory,
            commands::open_file_in_player,
            commands::player::open_external,
            // Your files (local episodes linked to a title)
            commands::get_local_media,
            commands::pick_local_media,
            commands::link_local_folder,
            commands::rescan_local_media,
            commands::add_local_files,
            commands::set_local_file_episode,
            commands::remove_local_file,
            commands::unlink_local_media,
            commands::get_local_episode_source,
            // Library › Local files (series matched to AniList)
            commands::scan_local_library,
            commands::match_local_series,
            commands::search_local_match,
            commands::set_local_series_match,
            commands::clear_local_series_match,
            // Updates
            commands::check_for_update,
            commands::get_site_url,
            commands::get_update_install_support,
            commands::install_update,
            commands::restart_after_update,
            // Stream proxy
            stream_proxy::get_stream_proxy_base,
            // Embedded mpv player
            embedded_mpv::embedded_mpv,
            embedded_mpv::embedded_mpv_status,
            embedded_mpv::embedded_mpv_answer,
            embedded_mpv::embedded_mpv_engine_changed,
            embedded_mpv::embedded_mpv_restart,
            embedded_mpv::embedded_mpv_take_route,
            embedded_mpv::web_player_active,
            embedded_mpv::embedded_mpv_clear_cache,
            // Custom Playlists & Queue
            commands::create_playlist,
            commands::list_playlists,
            commands::get_playlist_items,
            commands::add_multiple_to_playlist,
            commands::remove_from_playlist,
            commands::delete_playlist,
            commands::launch_playlist_external_player,
            // Accounts
            commands::start_account_login,
            commands::get_account_status,
            commands::disconnect_account,
            commands::cancel_account_login,
            commands::get_comick_library,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                embedded_mpv::on_exit();
                system::restart_if_asked(app);
            }
        });
}
