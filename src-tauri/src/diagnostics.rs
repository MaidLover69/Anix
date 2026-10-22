//! Settings › System › Save report: a plain-text report to attach to a bug report, with
//! versions, settings and recent failures. Private data is removed before anything is written.

use crate::db;
use crate::error::code_for_message;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use sqlx::SqlitePool;
use std::path::Path;

/// Recent warnings and errors from the log that go into the report.
const LOG_LINES: usize = 80;
const FAILED_DOWNLOADS: usize = 20;

static URL_QUERY: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?i)(\b[a-z][a-z0-9+.-]*://[^\s"'<>?#]+)[?#][^\s"'<>]*"#).unwrap());
static EMAIL: Lazy<Regex> = Lazy::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}").unwrap());
static BEARER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]+").unwrap());
static SECRET_FIELD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)((?:access_|refresh_|id_)?token|secret|password|passphrase|api[_-]?key|authorization|cookie|session)(["']?\s*[:=]\s*["']?)([^\s"',;&}]+)"#).unwrap()
});
static JWT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*").unwrap());
static LONG_TOKEN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[A-Za-z0-9_-]{32,}\b").unwrap());
static USER_DIR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^|[\s"'=(:])/(home|run/media|media|Users)/[^/\s"']+"#).unwrap());

/// Removes private data from `text`: query strings and fragments of links, email addresses,
/// tokens and secrets, and the user's home folder (written as `~`) or any other user folder.
pub fn redact(text: &str, home: Option<&str>) -> String {
    let mut out = text.to_string();
    if let Some(home) = home.map(|h| h.trim_end_matches('/')).filter(|h| h.len() > 1) {
        out = out.replace(home, "~");
    }
    out = USER_DIR.replace_all(&out, "$1/$2/<user>").into_owned();
    out = URL_QUERY.replace_all(&out, "$1?<removed>").into_owned();
    out = EMAIL.replace_all(&out, "<email>").into_owned();
    out = BEARER.replace_all(&out, "$1 <removed>").into_owned();
    out = JWT.replace_all(&out, "<token>").into_owned();
    out = SECRET_FIELD.replace_all(&out, "$1$2<removed>").into_owned();
    LONG_TOKEN.replace_all(&out, "<token>").into_owned()
}

/// The settings without the account keys, which only hold logins.
pub fn report_settings(settings: &Value) -> Value {
    let Some(map) = settings.as_object() else { return Value::Null };
    let private = |key: &str| {
        let key = key.to_ascii_lowercase();
        ["anilist_", "mal_", "comick_"].iter().any(|p| key.starts_with(p))
            || ["token", "secret", "password", "session", "cookie"].iter().any(|w| key.contains(w))
    };
    Value::Object(map.iter().filter(|(k, _)| !private(k)).map(|(k, v)| (k.clone(), v.clone())).collect())
}

fn os_name() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn kernel() -> String {
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut name) } != 0 {
        return "unknown".to_string();
    }
    let field = |raw: &[libc::c_char]| unsafe { std::ffi::CStr::from_ptr(raw.as_ptr()) }.to_string_lossy().into_owned();
    format!("{} {}", field(&name.release), field(&name.machine))
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| "unset".to_string())
}

/// The last warnings and errors in the log file.
fn log_failures(log: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().filter(|l| l.contains(" WARN ") || l.contains(" ERROR ")).collect();
    let skip = lines.len().saturating_sub(LOG_LINES);
    lines.drain(..skip);
    lines.into_iter().map(str::to_string).collect()
}

/// Puts the report together. Everything in it has been through [`redact`].
pub async fn build_report(pool: &SqlitePool) -> String {
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };

    line("Anix diagnostics report".to_string());
    line(format!("Anix {}", env!("CARGO_PKG_VERSION")));
    line(format!("System: {} · kernel {}", os_name(), kernel()));
    line(format!("Desktop: {} · session {}", env("XDG_CURRENT_DESKTOP"), env("XDG_SESSION_TYPE")));
    line(format!(
        "Package: {}",
        if std::env::var_os("APPIMAGE").is_some() { "AppImage" } else { "installed package or development build" }
    ));
    line(format!("{}: {}", crate::system::gnutls::PRIORITY_VAR, env(crate::system::gnutls::PRIORITY_VAR)));
    line(format!("WEBKIT_DISABLE_DMABUF_RENDERER: {}", env("WEBKIT_DISABLE_DMABUF_RENDERER")));

    let hardware = crate::system::hwaccel::report().await;
    line(String::new());
    line("== Video and graphics ==".to_string());
    line(serde_json::to_string_pretty(&hardware).unwrap_or_default());

    line(String::new());
    line("== Settings (logins left out) ==".to_string());
    let settings = db::get_config_dir()
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join("settings.json")).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null);
    line(serde_json::to_string_pretty(&report_settings(&settings)).unwrap_or_default());

    line(String::new());
    line("== Failed downloads ==".to_string());
    let mut failed: Vec<_> = db::get_all_downloads(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.status == db::STATUS_FAILED)
        .collect();
    failed.truncate(FAILED_DOWNLOADS);
    if failed.is_empty() {
        line("None".to_string());
    }
    for record in failed {
        let message = record.error_msg.unwrap_or_default();
        line(format!(
            "[{}] {} episode {} ({} {}, provider {}): {}",
            code_for_message(&message),
            record.series_title,
            record.episode_num,
            record.category,
            record.quality,
            record.provider,
            message
        ));
    }

    line(String::new());
    line("== Recent warnings and errors in the log ==".to_string());
    let log = db::get_app_dir().map(|dir| dir.join(crate::migration::LOG_FILE_NAME));
    let entries = log.as_deref().map(log_failures).unwrap_or_default();
    if entries.is_empty() {
        line("None".to_string());
    }
    for entry in entries {
        line(entry);
    }

    let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned());
    redact(&out, home.as_deref())
}
