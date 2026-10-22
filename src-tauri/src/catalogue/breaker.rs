//! Keeping away from catalogues that are down or that asked us to slow down.

use crate::error::AppError;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a failed catalogue is skipped.
pub const OPEN_FOR: Duration = Duration::from_secs(3 * 60);
/// Upper bound for a server-requested wait.
const MAX_OPEN_FOR: Duration = Duration::from_secs(15 * 60);

/// Failures that say the catalogue is out of reach or overloaded, not that the request was bad:
/// connection failures, timeouts, HTTP errors (AniList's 403 "API disabled", 5xx) and 429.
pub fn trips(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Unreachable(_) | AppError::Network(_) | AppError::RateLimited { .. }
    )
}

/// Remembers, per catalogue, until when it is not asked.
#[derive(Default)]
pub struct Breaker {
    open_until: Mutex<HashMap<&'static str, Instant>>,
}

impl Breaker {
    pub fn is_open(&self, name: &'static str) -> bool {
        self.is_open_at(name, Instant::now())
    }

    pub fn is_open_at(&self, name: &'static str, now: Instant) -> bool {
        let guard = self.open_until.lock().unwrap_or_else(|p| p.into_inner());
        guard.get(name).is_some_and(|until| *until > now)
    }

    pub fn record<T>(&self, name: &'static str, result: &Result<T, AppError>) {
        self.record_at(name, result, Instant::now());
    }

    pub fn record_at<T>(&self, name: &'static str, result: &Result<T, AppError>, now: Instant) {
        let mut guard = self.open_until.lock().unwrap_or_else(|p| p.into_inner());
        match result {
            Ok(_) => {
                guard.remove(name);
            }
            Err(e) if trips(e) => {
                let wait = match e {
                    AppError::RateLimited { retry_after_s } => {
                        Duration::from_secs(*retry_after_s).clamp(OPEN_FOR, MAX_OPEN_FOR)
                    }
                    _ => OPEN_FOR,
                };
                guard.insert(name, now + wait);
            }
            Err(_) => {}
        }
    }
}

/// Spaces the requests to each catalogue at least its minimum gap apart. A caller reserves the
/// next free slot and waits for it without holding the lock.
#[derive(Default)]
pub struct Pacer {
    next_free: Mutex<HashMap<&'static str, Instant>>,
}

impl Pacer {
    pub async fn wait(&self, name: &'static str, gap: Duration) {
        if gap.is_zero() {
            return;
        }
        let slot = {
            let mut guard = self.next_free.lock().unwrap_or_else(|p| p.into_inner());
            let now = Instant::now();
            let slot = guard.get(name).copied().filter(|t| *t > now).unwrap_or(now);
            guard.insert(name, slot + gap);
            slot
        };
        tokio::time::sleep_until(slot.into()).await;
    }
}
