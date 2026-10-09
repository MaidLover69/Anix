//! The few `settings.json` keys the HTTP layer and the AniList client act on.

use once_cell::sync::Lazy;
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const REFRESH_EVERY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DohProvider {
    Cloudflare,
    Google,
    AdGuard,
}

impl DohProvider {
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "google" => DohProvider::Google,
            "adguard" => DohProvider::AdGuard,
            _ => DohProvider::Cloudflare,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkSettings {
    pub enable_doh: bool,
    pub doh_provider: DohProvider,
    pub hide_adult_content: bool,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            enable_doh: true,
            doh_provider: DohProvider::Cloudflare,
            hide_adult_content: true,
        }
    }
}

impl NetworkSettings {
    /// Missing or malformed keys keep their defaults.
    pub fn from_json(json: &Value) -> Self {
        let defaults = Self::default();
        Self {
            enable_doh: json
                .get("enable_doh")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.enable_doh),
            doh_provider: json
                .get("doh_provider")
                .and_then(Value::as_str)
                .map(DohProvider::from_setting)
                .unwrap_or(defaults.doh_provider),
            hide_adult_content: json
                .get("hide_adult_content")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.hide_adult_content),
        }
    }
}

static CACHED: Lazy<Mutex<Option<(Instant, NetworkSettings)>>> = Lazy::new(|| Mutex::new(None));

fn read_from_disk() -> NetworkSettings {
    let Ok(dir) = crate::db::get_config_dir() else {
        return NetworkSettings::default();
    };
    match std::fs::read_to_string(dir.join("settings.json")) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .map(|json| NetworkSettings::from_json(&json))
            .unwrap_or_default(),
        Err(_) => NetworkSettings::default(),
    }
}

/// Current settings, re-read from disk at most every few seconds so a change made in the
/// Settings screen applies without a restart.
pub fn current() -> NetworkSettings {
    let mut guard = CACHED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((read_at, settings)) = *guard {
        if read_at.elapsed() < REFRESH_EVERY {
            return settings;
        }
    }
    let settings = read_from_disk();
    *guard = Some((Instant::now(), settings));
    settings
}
