//! The watch party's message set and its strict decoder.
//!
//! Every message is one JSON object with a `kind`, sent inside one encrypted frame. The set is
//! closed: an unknown kind, an extra field, a missing field, a number out of range or a frame
//! over [`MAX_MESSAGE_BYTES`] is refused, and the connection that sent it is closed. A peer
//! can only say *what* plays (anime id, episode, sub/dub, language, the host's extension id
//! and server name, the episode length) and *where* (position, paused, speed); never a URL,
//! a path or anything that runs.

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Version of the protocol; a `Hello` with another one is refused.
pub const PROTOCOL_VERSION: u8 = 1;
/// Largest plaintext message, in bytes.
pub const MAX_MESSAGE_BYTES: usize = 4096;
/// Largest episode number accepted (some lists number on past 1000).
pub const MAX_EPISODE: f64 = 100_000.0;
/// Positions and lengths above a day are not an episode.
pub const MAX_POSITION_MS: u64 = 86_400_000;
/// Speeds the player offers.
pub const MIN_RATE: f64 = 0.25;
pub const MAX_RATE: f64 = 4.0;

/// Sub or dub, as in the player's route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Sub,
    Dub,
}

/// What the host is playing. Each guest finds this episode through their own extensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Load {
    /// AniList id, or a negative stand-in id (never 0).
    pub anime_id: i64,
    pub episode: f64,
    pub category: Category,
    /// ISO 639 code of the subtitle (sub) or audio (dub) language, when known.
    pub language: Option<String>,
    /// The host's extension id, so a guest can pick the same one.
    pub extension: Option<String>,
    /// The host's server name, shown to guests.
    pub server: Option<String>,
    /// The host's episode length, so a guest sees when their version differs.
    pub duration_ms: Option<u64>,
}

/// One message of the party. `Hello`, `Ping` and `Ready` go from guest to host; `Welcome`,
/// `Pong`, `Load`, `Prepare` and `State` from host to guest; `Bye` both ways.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    /// The guest's first message; `t1` is its clock (ms) when sent.
    Hello { proto: u8, t1: u64 },
    /// Answers `Hello`: the guest's `t1` and the host's clock when it answered.
    Welcome { t1: u64, t2: u64 },
    Ping { t1: u64 },
    Pong { t1: u64, t2: u64 },
    Load(Load),
    /// Get ready to play from `position_ms`: seek there paused, then answer `Ready`.
    Prepare { seq: u64, position_ms: u64 },
    /// The guest is at the position of `Prepare` `seq` and has enough buffered.
    Ready { seq: u64 },
    /// At host time `at_ms` (which may lie ahead), the host is at `position_ms`.
    State { seq: u64, paused: bool, position_ms: u64, rate: f64, at_ms: u64 },
    Bye,
}

impl Message {
    /// True for the kinds only the host sends.
    pub fn is_from_host(&self) -> bool {
        matches!(
            self,
            Message::Welcome { .. } | Message::Pong { .. } | Message::Load(_) | Message::Prepare { .. } | Message::State { .. } | Message::Bye
        )
    }

    /// True for the kinds only a guest sends.
    pub fn is_from_guest(&self) -> bool {
        matches!(self, Message::Hello { .. } | Message::Ping { .. } | Message::Ready { .. } | Message::Bye)
    }
}

/// Why a message was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("message too large")]
    TooLarge,
    #[error("malformed message: {0}")]
    Malformed(String),
    #[error("value out of range: {0}")]
    OutOfRange(&'static str),
}

/// Serialises one message.
pub fn encode(message: &Message) -> Vec<u8> {
    serde_json::to_vec(message).expect("party messages always serialise")
}

/// Reads one message and checks every value; anything unexpected is an error.
pub fn decode(bytes: &[u8]) -> Result<Message, ProtocolError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let message: Message = serde_json::from_slice(bytes).map_err(|e| ProtocolError::Malformed(e.to_string()))?;
    validate(&message)?;
    Ok(message)
}

/// Checks the values of a message (also used for what the page hands the host).
pub fn validate(message: &Message) -> Result<(), ProtocolError> {
    match message {
        Message::Hello { proto, .. } if *proto != PROTOCOL_VERSION => Err(ProtocolError::OutOfRange("proto")),
        Message::Load(load) => validate_load(load),
        Message::Prepare { position_ms, .. } if *position_ms > MAX_POSITION_MS => Err(ProtocolError::OutOfRange("position_ms")),
        Message::State { position_ms, rate, .. } => {
            if *position_ms > MAX_POSITION_MS {
                Err(ProtocolError::OutOfRange("position_ms"))
            } else if !valid_rate(*rate) {
                Err(ProtocolError::OutOfRange("rate"))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// A speed the player can play at.
pub fn valid_rate(rate: f64) -> bool {
    rate.is_finite() && (MIN_RATE..=MAX_RATE).contains(&rate)
}

/// Checks a [`Load`].
pub fn validate_load(load: &Load) -> Result<(), ProtocolError> {
    if load.anime_id == 0 {
        return Err(ProtocolError::OutOfRange("anime_id"));
    }
    if !load.episode.is_finite() || !(0.0..=MAX_EPISODE).contains(&load.episode) {
        return Err(ProtocolError::OutOfRange("episode"));
    }
    if let Some(language) = &load.language {
        if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err(ProtocolError::OutOfRange("language"));
        }
    }
    if let Some(extension) = &load.extension {
        if !is_extension_id(extension) {
            return Err(ProtocolError::OutOfRange("extension"));
        }
    }
    if let Some(server) = &load.server {
        let chars = server.chars().count();
        if chars == 0 || chars > 64 || server.chars().any(char::is_control) {
            return Err(ProtocolError::OutOfRange("server"));
        }
    }
    if load.duration_ms.is_some_and(|ms| ms > MAX_POSITION_MS) {
        return Err(ProtocolError::OutOfRange("duration_ms"));
    }
    Ok(())
}

/// An extension id as manifests allow it: `[A-Za-z0-9_-]`, at most 64.
pub fn is_extension_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Token bucket for the messages one peer may send: `RATE_PER_S` a second with bursts of
/// `BURST`. A peer that runs it dry is disconnected.
pub struct RateLimit {
    tokens: f64,
    last: Instant,
}

impl RateLimit {
    pub const RATE_PER_S: f64 = 10.0;
    pub const BURST: f64 = 20.0;

    pub fn new(now: Instant) -> Self {
        Self { tokens: Self::BURST, last: now }
    }

    /// Takes one token; false when there is none.
    pub fn allow(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * Self::RATE_PER_S).min(Self::BURST);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}
