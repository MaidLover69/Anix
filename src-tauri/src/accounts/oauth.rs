//! Authorize URLs, PKCE values and redirect parsing for the registered OAuth clients.
//!
//! AniList uses the implicit grant and ends at `http://localhost/#access_token=…`;
//! MyAnimeList uses the authorization-code grant with PKCE (`plain`, the only method it
//! supports) and ends at `https://localhost/?code=…`. Neither redirect can be changed.
//!
//! Comick uses the authorization-code grant with PKCE `S256` for a public client. Its login
//! page must open in the system browser, so its redirect is a loopback address the app listens
//! on: [`COMICK_REDIRECT_URI`]. The client is registered at comick.dev/developers/apps (public
//! native client, library write enabled, exactly that callback URL); its id is read at build
//! time from `ANIX_COMICK_CLIENT_ID`, and a build without it offers no Comick sign-in.

use crate::commands::auth::ANILIST_CLIENT_ID;
use crate::commands::mal::MAL_CLIENT_ID;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use url::Url;

const ANILIST_AUTHORIZE_URL: &str = "https://anilist.co/api/v2/oauth/authorize";
const MAL_AUTHORIZE_URL: &str = "https://myanimelist.net/v1/oauth2/authorize";

pub const COMICK_AUTHORIZE_URL: &str = "https://comick.dev/api/auth/oauth2/authorize";
pub const COMICK_TOKEN_URL: &str = "https://comick.dev/api/auth/oauth2/token";
pub const COMICK_REVOKE_URL: &str = "https://comick.dev/api/auth/oauth2/revoke";
pub const COMICK_API_URL: &str = "https://api.comick.dev/integrations/v1";
/// Sent as `resource` with every authorize and token request.
pub const COMICK_RESOURCE: &str = "https://api.comick.dev/integrations/v1";
pub const COMICK_SCOPES: &str = "library:read library:write offline_access";

pub const COMICK_CALLBACK_HOST: &str = "127.0.0.1";
pub const COMICK_CALLBACK_PORT: u16 = 47823;
pub const COMICK_CALLBACK_PATH: &str = "/oauth/comick/callback";
/// Must be registered for the Comick client exactly as written: Comick compares the whole URL.
pub const COMICK_REDIRECT_URI: &str = "http://127.0.0.1:47823/oauth/comick/callback";

/// The Comick client id this build was made with, from `ANIX_COMICK_CLIENT_ID`.
pub fn comick_client_id() -> Option<&'static str> {
    option_env!("ANIX_COMICK_CLIENT_ID")
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Service {
    #[serde(rename = "anilist")]
    AniList,
    #[serde(rename = "mal")]
    Mal,
    #[serde(rename = "comick")]
    Comick,
}

impl Service {
    pub const ALL: [Service; 3] = [Service::AniList, Service::Mal, Service::Comick];

    /// The name used in events, the sync queue and command arguments.
    pub fn key(self) -> &'static str {
        match self {
            Service::AniList => "anilist",
            Service::Mal => "mal",
            Service::Comick => "comick",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Service::ALL.into_iter().find(|service| service.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Service::AniList => "AniList",
            Service::Mal => "MyAnimeList",
            Service::Comick => "Comick",
        }
    }

    /// Whether this build can sign in to the service at all.
    pub fn available(self) -> bool {
        match self {
            Service::AniList | Service::Mal => true,
            Service::Comick => comick_client_id().is_some(),
        }
    }
}

/// `bytes` random bytes from the kernel, as unpadded base64url text.
fn random_url_safe(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut buffer))
        .map_err(|e| format!("Could not read random bytes: {}", e))?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

/// The value a login attempt sends as `state` and expects back unchanged.
pub fn new_state() -> Result<String, String> {
    random_url_safe(24)
}

/// A PKCE code verifier: 43 characters of `[A-Za-z0-9-_]` (RFC 7636 allows 43 to 128).
pub fn new_pkce_verifier() -> Result<String, String> {
    random_url_safe(32)
}

/// AniList's implicit grant uses the redirect registered for the client; passing the same
/// redirect explicitly makes its grant step fail with `unsupported_grant_type`.
pub fn anilist_authorize_url(state: &str) -> String {
    Url::parse_with_params(
        ANILIST_AUTHORIZE_URL,
        &[
            ("client_id", ANILIST_CLIENT_ID),
            ("response_type", "token"),
            ("state", state),
        ],
    )
    .map(String::from)
    .unwrap_or_default()
}

/// With PKCE `plain` the challenge is the verifier itself.
pub fn mal_authorize_url(state: &str, code_verifier: &str) -> String {
    Url::parse_with_params(
        MAL_AUTHORIZE_URL,
        &[
            ("response_type", "code"),
            ("client_id", MAL_CLIENT_ID),
            ("state", state),
            ("code_challenge", code_verifier),
            ("code_challenge_method", "plain"),
        ],
    )
    .map(String::from)
    .unwrap_or_default()
}

/// The S256 PKCE challenge of `verifier`: unpadded base64url of its SHA-256.
pub fn pkce_s256_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The Comick authorize address for one attempt, opened in the system browser.
pub fn comick_authorize_url(client_id: &str, state: &str, code_verifier: &str) -> String {
    let challenge = pkce_s256_challenge(code_verifier);
    Url::parse_with_params(
        COMICK_AUTHORIZE_URL,
        &[
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", COMICK_REDIRECT_URI),
            ("scope", COMICK_SCOPES),
            ("resource", COMICK_RESOURCE),
            ("state", state),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ],
    )
    .map(String::from)
    .unwrap_or_default()
}

/// What a URL reached during a login means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Redirect {
    /// Not the registered redirect: an ordinary page of the login flow.
    Elsewhere,
    /// AniList's redirect, with the token from the URL fragment.
    Token {
        access_token: String,
        expires_in: Option<i64>,
        state: Option<String>,
    },
    /// MyAnimeList's redirect, with the authorization code from the query.
    Code { code: String, state: Option<String> },
    /// The registered redirect reporting a refusal or failure.
    Denied {
        error: String,
        description: Option<String>,
    },
    /// The registered redirect without a token, code or error.
    Empty,
}

/// True for `http(s)://localhost/…` on the default port, the address both clients are
/// registered with. AniList is registered with `http`, MyAnimeList with `https`; either scheme
/// is accepted for both because the service decides which one it sends.
pub fn is_registered_redirect(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str() == Some("localhost")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

fn value_of(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(name, value)| name == key && !value.trim().is_empty())
        .map(|(_, value)| value.clone())
}

/// True for exactly [`COMICK_REDIRECT_URI`], with any query.
pub fn is_comick_callback(url: &Url) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some(COMICK_CALLBACK_HOST)
        && url.port() == Some(COMICK_CALLBACK_PORT)
        && url.path() == COMICK_CALLBACK_PATH
        && url.username().is_empty()
        && url.password().is_none()
}

/// The request target a loopback listener received (`/path?query`), as a full URL.
pub fn comick_callback_url(request_target: &str) -> Option<Url> {
    if !request_target.starts_with('/') {
        return None;
    }
    Url::parse(&format!(
        "http://{}:{}{}",
        COMICK_CALLBACK_HOST, COMICK_CALLBACK_PORT, request_target
    ))
    .ok()
}

pub fn parse_redirect(service: Service, url: &Url) -> Redirect {
    let registered = match service {
        Service::Comick => is_comick_callback(url),
        Service::AniList | Service::Mal => is_registered_redirect(url),
    };
    if !registered {
        return Redirect::Elsewhere;
    }

    let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let fragment: Vec<(String, String)> = url
        .fragment()
        .map(|fragment| url::form_urlencoded::parse(fragment.as_bytes()).into_owned().collect())
        .unwrap_or_default();

    match service {
        Service::AniList => {
            if let Some(access_token) = value_of(&fragment, "access_token") {
                return Redirect::Token {
                    access_token,
                    expires_in: value_of(&fragment, "expires_in").and_then(|v| v.parse().ok()),
                    state: value_of(&fragment, "state"),
                };
            }
        }
        Service::Mal | Service::Comick => {
            if let Some(code) = value_of(&query, "code") {
                return Redirect::Code {
                    code,
                    state: value_of(&query, "state"),
                };
            }
        }
    }

    let either = |key: &str| value_of(&fragment, key).or_else(|| value_of(&query, key));
    match either("error") {
        Some(error) => Redirect::Denied {
            error,
            description: either("error_description")
                .or_else(|| either("message"))
                .or_else(|| either("hint")),
        },
        None => Redirect::Empty,
    }
}

/// The sentence shown when the service sends the user back with `error=…`.
pub fn denial_message(service: Service, error: &str, description: Option<&str>) -> String {
    if error == "access_denied" {
        return format!("{} sign-in was not approved.", service.label());
    }
    match description {
        Some(description) => format!("{} sign-in failed: {} ({})", service.label(), description, error),
        None => format!("{} sign-in failed: {}", service.label(), error),
    }
}

/// What one request to the Comick loopback listener is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComickCallback {
    /// Another path (a favicon request, a stray visit): answered with 404, the wait goes on.
    NotCallback,
    /// The callback path without this attempt's `state`: refused, the wait goes on.
    WrongState,
    /// The callback of this attempt: a code, a refusal, or nothing usable.
    Answer(Redirect),
}

/// Classifies the request target (`/path?query`) the loopback listener received. `state` is
/// checked first, for refusals too.
pub fn classify_comick_callback(request_target: &str, expected_state: &str) -> ComickCallback {
    let Some(url) = comick_callback_url(request_target).filter(is_comick_callback) else {
        return ComickCallback::NotCallback;
    };
    let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    if value_of(&query, "state").as_deref() != Some(expected_state) {
        return ComickCallback::WrongState;
    }
    ComickCallback::Answer(parse_redirect(Service::Comick, &url))
}
