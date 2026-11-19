//! The preferred streaming resolution: it orders the streams of the chosen source and never
//! overrides the language choice.

use crate::downloads::quality_height;
use crate::models::StreamItem;

const HEIGHTS: [u32; 4] = [1080, 720, 480, 360];

/// A setting ("720", "720p") as a height; "auto", "best" and anything unknown mean no wish.
pub fn wanted_height(setting: &str) -> Option<u32> {
    let height = quality_height(setting.trim())?;
    HEIGHTS.contains(&height).then_some(height)
}

/// Exact height first, then the closest lower ones, then the closest higher ones, then streams
/// without a stated height. The sort is stable, so equal heights keep the provider's order.
pub fn order_streams(streams: &mut [StreamItem], wanted: Option<u32>) {
    let Some(wanted) = wanted else { return };
    streams.sort_by_key(|s| match quality_height(s.quality.as_deref().unwrap_or("")) {
        Some(h) if h <= wanted => (0, wanted - h),
        Some(h) => (1, h - wanted),
        None => (2, 0),
    });
}

static WANTED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The height the current lookup wants, for a source that verifies its own streams and must
/// order them before it does (0 = no wish).
pub fn set_current_wanted(height: Option<u32>) {
    WANTED.store(height.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
}

pub fn current_wanted() -> Option<u32> {
    Some(WANTED.load(std::sync::atomic::Ordering::Relaxed)).filter(|h| *h > 0)
}
