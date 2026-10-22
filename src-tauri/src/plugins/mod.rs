//! Streaming plugins: JavaScript providers in Seanime's `onlinestream-provider` format, run in
//! a sandboxed engine and installed from repositories the user adds.

pub mod dom;
pub mod engine;
pub mod health;
pub mod net;
pub mod transpile;
pub mod provider;
pub mod registry;
pub mod userconfig;
pub mod scanner_utils;

use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::sync::Mutex;

static INTERNED: Lazy<Mutex<HashSet<&'static str>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// A `'static` copy of a plugin id, which the provider interfaces name providers by. Each
/// distinct id is kept once for the life of the process.
pub fn intern(text: &str) -> &'static str {
    let mut known = INTERNED.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(found) = known.get(text) {
        return found;
    }
    let leaked: &'static str = Box::leak(text.to_string().into_boxed_str());
    known.insert(leaked);
    leaked
}

/// Announces plugin changes to the frontend and, in the background, loads the installed
/// plugins and looks for new versions. Nothing here delays startup.
pub fn start(app: tauri::AppHandle, registry: registry::PluginRegistry) {
    use tauri::Emitter;
    let announcer = app.clone();
    registry.set_notifier(std::sync::Arc::new(move || {
        if let Err(e) = announcer.emit(registry::EVENT_PLUGINS_CHANGED, ()) {
            tracing::warn!("could not announce a plugin change: {}", e);
        }
    }));
    let health_announcer = app.clone();
    health::start_background(
        registry.clone(),
        std::sync::Arc::new(move || {
            let _ = health_announcer.emit(health::EVENT_HEALTH_CHANGED, ());
        }),
    );
    tauri::async_runtime::spawn(async move {
        registry.load_all().await;
        registry.refresh_repos().await;
    });
}
