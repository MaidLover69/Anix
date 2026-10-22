use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug, Clone, Serialize)]
#[serde(tag = "type", content = "message")]
pub enum AppError {
    #[error("Network error: {0}")]
    Network(String),

    /// The host could not be reached at all (DNS failure, refused or timed-out connection, TLS).
    #[error("Connection failed: {0}")]
    Unreachable(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Provider error: {provider} — {message}")]
    Provider {
        provider: &'static str,
        message: String,
    },

    #[error("Blocked by the site's anti-bot check")]
    CaptchaRequired,

    /// The site refused the in-app browser as well, with a check or block page the user can
    /// look at. `site` is the site key and `origin` the mirror that answered.
    #[error("{site} needs verification")]
    VerificationRequired { site: String, origin: String },

    #[error("No sources found")]
    NoSourcesFound,

    /// No provider produced a playable stream; the message lists the reason per provider.
    #[error("{0}")]
    SourcesUnavailable(String),

    #[error("Rate limited — retry after {retry_after_s}s")]
    RateLimited { retry_after_s: u64 },

    #[error("Auth required")]
    AuthRequired,

    #[error("Other error: {0}")]
    Other(String),
}

impl AppError {
    pub fn provider(provider: &'static str, message: impl Into<String>) -> Self {
        AppError::Provider {
            provider,
            message: message.into(),
        }
    }

    /// Short form used when listing why each provider failed.
    pub fn reason(&self) -> String {
        match self {
            AppError::Network(m) | AppError::Other(m) | AppError::SourcesUnavailable(m) => m.clone(),
            AppError::Unreachable(m) => format!("connection failed ({})", m),
            AppError::Database(m) => format!("database error ({})", m),
            AppError::Parse(m) => format!("unexpected response ({})", m),
            AppError::Provider { message, .. } => message.clone(),
            AppError::CaptchaRequired => "blocked by anti-bot check".to_string(),
            AppError::VerificationRequired { .. } => "needs verification".to_string(),
            AppError::NoSourcesFound => "no source for this episode".to_string(),
            AppError::RateLimited { .. } => "rate limited".to_string(),
            AppError::AuthRequired => "login required".to_string(),
        }
    }

    pub fn is_connection_failure(&self) -> bool {
        matches!(self, AppError::Unreachable(_))
    }

    /// A short stable code for bug reports, such as `NET-403` or `CAT-TIMEOUT`. The frontend
    /// derives the same code from the message (`errorCode` in `ErrorState.tsx`), because
    /// commands hand errors over as text.
    pub fn code(&self) -> String {
        match self {
            AppError::Network(m) => format!("{}-{}", domain(m), network_detail(m).as_deref().unwrap_or("ERR")),
            AppError::Unreachable(m) => format!("{}-{}", domain(m), connection_detail(m)),
            AppError::Database(_) => "DB-ERR".to_string(),
            AppError::Parse(m) => format!("{}-PARSE", domain(m)),
            AppError::Provider { message, .. } => format!("SRC-{}", network_detail(message).as_deref().unwrap_or("ERR")),
            AppError::CaptchaRequired => "SRC-CAPTCHA".to_string(),
            AppError::VerificationRequired { .. } => "SRC-VERIFY".to_string(),
            AppError::NoSourcesFound => "SRC-NONE".to_string(),
            AppError::SourcesUnavailable(_) => "SRC-UNAVAILABLE".to_string(),
            AppError::RateLimited { .. } => "NET-429".to_string(),
            AppError::AuthRequired => "AUTH-LOGIN".to_string(),
            AppError::Other(m) => match network_detail(m) {
                Some(detail) => format!("{}-{}", domain(m), detail),
                None => "APP-ERR".to_string(),
            },
        }
    }
}

/// Names of the catalogues; an error that mentions one is a catalogue error.
const CATALOGUE_NAMES: &[&str] = &["anilist", "myanimelist", "jikan", "kitsu", "shikimori", "comick", "imdb"];

fn domain(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if CATALOGUE_NAMES.iter().any(|name| lower.contains(name)) {
        "CAT"
    } else {
        "NET"
    }
}

static HTTP_STATUS: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r"(?i)\b(?:http|status)(?:\s+status)?(?:\s+code)?\s*:?\s*([1-5]\d\d)\b").unwrap()
});

/// `TIMEOUT` or the HTTP status a message names; `None` when it names neither.
fn network_detail(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        return Some("TIMEOUT".to_string());
    }
    Some(HTTP_STATUS.captures(message)?.get(1)?.as_str().to_string())
}

fn connection_detail(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("dns") || lower.contains("lookup") || lower.contains("resolve") {
        "DNS"
    } else if lower.contains("tls") || lower.contains("certificate") || lower.contains("handshake") || lower.contains("ssl") {
        "TLS"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "TIMEOUT"
    } else {
        "CONNECT"
    }
}

/// The code of an error that crossed into text: the message of an [`AppError`] (its Display)
/// or any other failure message. Mirrors [`AppError::code`].
pub fn code_for_message(message: &str) -> String {
    let m = message.trim();
    let rest = |prefix: &str| m.strip_prefix(prefix).map(str::to_string);
    if let Some(inner) = rest("Network error: ") {
        AppError::Network(inner).code()
    } else if let Some(inner) = rest("Connection failed: ") {
        AppError::Unreachable(inner).code()
    } else if m.starts_with("Database error: ") {
        "DB-ERR".to_string()
    } else if let Some(inner) = rest("Parse error: ") {
        AppError::Parse(inner).code()
    } else if let Some(inner) = rest("Provider error: ") {
        AppError::provider("", inner).code()
    } else if m == "Blocked by the site's anti-bot check" {
        "SRC-CAPTCHA".to_string()
    } else if m.ends_with(" needs verification") {
        "SRC-VERIFY".to_string()
    } else if m == "No sources found" {
        "SRC-NONE".to_string()
    } else if m.starts_with("Rate limited") {
        "NET-429".to_string()
    } else if m == "Auth required" {
        "AUTH-LOGIN".to_string()
    } else {
        AppError::Other(m.strip_prefix("Other error: ").unwrap_or(m).to_string()).code()
    }
}

fn root_cause(err: &(dyn std::error::Error + 'static)) -> String {
    let mut current = err;
    while let Some(next) = current.source() {
        current = next;
    }
    current.to_string()
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        let host = err
            .url()
            .and_then(|u| u.host_str())
            .unwrap_or("unknown host")
            .to_string();
        if err.is_connect() {
            AppError::Unreachable(format!("{}: {}", host, root_cause(&err)))
        } else if err.is_timeout() {
            AppError::Network(format!("{} timed out", host))
        } else if err.is_decode() {
            AppError::Parse(format!("{}: {}", host, root_cause(&err)))
        } else {
            AppError::Network(format!("{}: {}", host, root_cause(&err)))
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Parse(err.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Other(err.to_string())
    }
}

impl From<AppError> for String {
    fn from(err: AppError) -> Self {
        err.to_string()
    }
}
