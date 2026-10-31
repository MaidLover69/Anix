//! LAN watch party: one host controls playback, up to eight guests on the same network follow.
//!
//! Each viewer plays the episode through their own extensions; the party only carries *what*
//! plays (anime id, episode, sub/dub, language, the host's extension and server, the length)
//! and *where* (position, paused, speed). Joining needs the host's 6-digit code, checked with
//! a password-authenticated key exchange, and every message after that is encrypted. There is
//! no server, no account and nothing is stored.
//!
//! - `protocol`: the closed message set and its strict decoder
//! - `secure`: the code exchange (SPAKE2) and the sealed frames (ChaCha20-Poly1305)
//! - `sync`: clock offset and report ages
//! - `lan`: which interfaces and addresses are allowed, mDNS discovery
//! - `session`: the host and the guest over TCP

pub mod lan;
pub mod protocol;
pub mod secure;
pub mod session;
pub mod sync;

pub use session::{EndReason, Guest, Host, PartyError, PartyEvent, PartyStatus, PlayState, Role};
