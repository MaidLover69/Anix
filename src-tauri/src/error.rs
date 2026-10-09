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
