//! Clock maths of the watch party: how far the host's clock is from a guest's, and how old a
//! host report is in the guest's own time.
//!
//! The host's monotonic clock is the reference. A guest pings it; each answer gives one
//! estimate of the offset (NTP style), and the one with the shortest round trip is the most
//! trustworthy, because less of it can be one-way delay. The page then only sees ages
//! ("this position was true 120 ms ago", or "… will be true in 900 ms").

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Instant;

/// Answers kept for the offset.
pub const CLOCK_SAMPLES: usize = 8;

/// Milliseconds on this process's monotonic clock.
pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// One ping answer: round trip and the host clock minus the guest clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSample {
    pub rtt_ms: u64,
    pub offset_ms: i64,
}

impl ClockSample {
    /// From the guest's send time `t1`, the host's answer time `t2` and the guest's receive
    /// time `t3`. The host is assumed to have answered half way through the round trip.
    pub fn new(t1: u64, t2: u64, t3: u64) -> Self {
        let rtt_ms = t3.saturating_sub(t1);
        let midpoint = (t1 as i128 + t3 as i128) / 2;
        Self { rtt_ms, offset_ms: (t2 as i128 - midpoint) as i64 }
    }
}

/// The last few samples and the offset they give.
#[derive(Debug, Default)]
pub struct Clock {
    samples: VecDeque<ClockSample>,
}

impl Clock {
    pub fn add(&mut self, sample: ClockSample) {
        if self.samples.len() == CLOCK_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// The offset of the fastest round trip kept; None before the first answer.
    pub fn offset_ms(&self) -> Option<i64> {
        self.samples.iter().min_by_key(|s| s.rtt_ms).map(|s| s.offset_ms)
    }

    /// The round trip of that sample.
    pub fn rtt_ms(&self) -> Option<u64> {
        self.samples.iter().map(|s| s.rtt_ms).min()
    }
}

/// How long ago, in guest time, host time `at_host_ms` was (negative: still ahead).
pub fn age_ms(at_host_ms: u64, offset_ms: i64, now_guest_ms: u64) -> i64 {
    let at_guest = at_host_ms as i128 - offset_ms as i128;
    (now_guest_ms as i128 - at_guest) as i64
}

/// Where a report puts the playhead `age_ms` after it was true: still while paused, moving
/// at `rate` while playing, and never before the start.
pub fn position_after(position_ms: u64, paused: bool, rate: f64, age_ms: i64) -> u64 {
    if paused {
        return position_ms;
    }
    (position_ms as f64 + age_ms as f64 * rate).max(0.0).round() as u64
}
