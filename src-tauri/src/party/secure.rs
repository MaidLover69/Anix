//! Joining with the host's short code, and the encryption of every message after that.
//!
//! Guest and host run SPAKE2 with the code as the password. Someone who records or fakes the
//! exchange learns nothing they can test offline: each attempt checks one guess, and the host
//! locks the party after a few wrong ones. The shared key then seals every message with
//! ChaCha20-Poly1305. The nonce is a counter per direction that is never sent, so a replayed,
//! dropped, reordered or altered frame fails to open and the connection is closed.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand::Rng;
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// Digits in a join code.
pub const CODE_DIGITS: usize = 6;
/// Bytes of one SPAKE2 message (side byte + curve point).
pub const PAKE_MESSAGE_BYTES: usize = 33;
/// Bytes the AEAD tag adds to every frame.
pub const TAG_BYTES: usize = 16;

const GUEST_ID: &[u8] = b"anix-party-guest";
const HOST_ID: &[u8] = b"anix-party-host";

/// Which end of the connection this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Guest,
    Host,
}

impl Side {
    fn direction(self) -> u8 {
        match self {
            Side::Guest => 1,
            Side::Host => 2,
        }
    }
    fn other(self) -> Side {
        match self {
            Side::Guest => Side::Host,
            Side::Host => Side::Guest,
        }
    }
}

/// Why the secure channel could not be set up or a frame could not be opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecureError {
    #[error("bad key exchange message")]
    BadExchange,
    #[error("frame did not open")]
    BadFrame,
}

/// A new random code, `CODE_DIGITS` digits.
pub fn new_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000u32))
}

/// The code as typed ("123 456", "123-456") reduced to its digits; None unless it has exactly
/// `CODE_DIGITS` of them and nothing else.
pub fn normalize_code(input: &str) -> Option<String> {
    let digits: String = input.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    (digits.len() == CODE_DIGITS && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

/// One side of the key exchange, between sending its message and reading the other's.
pub struct Pake {
    side: Side,
    state: Spake2<Ed25519Group>,
}

impl Pake {
    /// Starts the exchange; the returned bytes go to the other side.
    pub fn start(side: Side, code: &str) -> (Pake, Vec<u8>) {
        let password = Password::new(code.as_bytes());
        let (guest, host) = (Identity::new(GUEST_ID), Identity::new(HOST_ID));
        let (state, outbound) = match side {
            Side::Guest => Spake2::<Ed25519Group>::start_a(&password, &guest, &host),
            Side::Host => Spake2::<Ed25519Group>::start_b(&password, &guest, &host),
        };
        (Pake { side, state }, outbound)
    }

    /// Reads the other side's message. With different codes this still succeeds, but the two
    /// keys differ, so the first sealed frame fails to open.
    pub fn finish(self, inbound: &[u8]) -> Result<Channel, SecureError> {
        if inbound.len() != PAKE_MESSAGE_BYTES {
            return Err(SecureError::BadExchange);
        }
        let key = self.state.finish(inbound).map_err(|_| SecureError::BadExchange)?;
        Ok(Channel::new(self.side, &key))
    }
}

/// Seals frames to the other side and opens theirs, in order.
pub struct Channel {
    cipher: ChaCha20Poly1305,
    side: Side,
    sent: u64,
    received: u64,
}

impl Channel {
    fn new(side: Side, key: &[u8]) -> Self {
        Self { cipher: ChaCha20Poly1305::new(Key::from_slice(&key[..32])), side, sent: 0, received: 0 }
    }

    fn nonce(direction: u8, counter: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[0] = direction;
        nonce[4..].copy_from_slice(&counter.to_be_bytes());
        nonce
    }

    /// Encrypts the next frame.
    pub fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = Self::nonce(self.side.direction(), self.sent);
        self.sent += 1;
        self.cipher.encrypt(Nonce::from_slice(&nonce), plaintext).expect("encryption of a small frame cannot fail")
    }

    /// Decrypts the next frame from the other side. Only the frame right after the last one
    /// opened succeeds.
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, SecureError> {
        let nonce = Self::nonce(self.side.other().direction(), self.received);
        let plaintext = self.cipher.decrypt(Nonce::from_slice(&nonce), frame).map_err(|_| SecureError::BadFrame)?;
        self.received += 1;
        Ok(plaintext)
    }
}
