//! The weekly airing schedule without AniList, which is the only catalogue with per-episode
//! air times. Shikimori knows each show's next episode and its time; MyAnimeList only the
//! weekly broadcast slot (in JST) and the start date. Both are extended week by week across
//! the window, so the times are estimates: a break week or a double episode is not known.

use super::model::{days_from_civil, parse_date, Found};
use crate::models::AiringEntry;
use serde_json::Value;

pub const WEEK_S: i64 = 7 * 86_400;
/// Japan has no daylight saving time.
pub const JST_OFFSET_S: i64 = 9 * 3600;

/// Above this, an episode number counted from the start date of a show without a known
/// episode total is not believed.
const MAX_UNCOUNTED_EPISODE: i32 = 60;

/// The episode number of a slot whose number is not known (a long-running show on MAL):
/// the schedule shows it without a number.
pub const UNKNOWN_EPISODE: i32 = 0;

/// One anime and the (air time, episode) pairs it has in the window.
pub type Slots = (Found, Vec<(i64, i32)>);

/// Episode `episode` airs at `at`, and one more every week; the airings in `[from, to]`.
/// `total` caps the episode number when known; a show that is not weekly TV only gets `at`.
pub fn weekly(at: i64, episode: i32, total: Option<i32>, weekly: bool, from: i64, to: i64) -> Vec<(i64, i32)> {
    if !weekly {
        return if (from..=to).contains(&at) && episode >= 1 { vec![(at, episode)] } else { Vec::new() };
    }
    let first_k = (from - at).div_euclid(WEEK_S) + if (from - at).rem_euclid(WEEK_S) == 0 { 0 } else { 1 };
    let mut out = Vec::new();
    let mut k = first_k;
    loop {
        let time = at + k * WEEK_S;
        if time > to {
            break;
        }
        let ep = episode as i64 + k;
        if ep >= 1 && total.map_or(true, |t| ep <= t as i64) {
            out.push((time, ep as i32));
        }
        k += 1;
    }
    out
}

/// Whether Shikimori's count for a long-running show without an episode total is behind: fewer
/// than two episodes for every three weeks since its start season. Recorded 2026-10-22: Crayon
/// Shin-chan "999 aired" (AniList: next is 1358) and Doraemon (2005) "529" (AniList: 938),
/// while One Piece (1180) and Detective Conan (1217) matched AniList.
pub fn count_is_behind(media: &crate::models::Media, airing_at: i64, episode: i32) -> bool {
    if media.episodes.is_some() || episode <= MAX_UNCOUNTED_EPISODE {
        return false;
    }
    let Some(year) = media.season_year else { return false };
    let month = match media.season.as_deref() {
        Some("SPRING") => 3,
        Some("SUMMER") => 6,
        Some("FALL") => 9,
        _ => 1,
    };
    let weeks = (airing_at - days_from_civil(year as i64, month, 1) * 86_400) / WEEK_S;
    (episode as i64) * 3 < weeks * 2
}

/// Shikimori's `animes` with `nextEpisodeAt` (already parsed into `next_airing_episode`). A
/// long-runner whose count is behind ([`count_is_behind`]) is listed without a number.
pub fn from_shikimori(found: Vec<Found>, from: i64, to: i64) -> Vec<Slots> {
    found
        .into_iter()
        .filter_map(|f| {
            let next = f.media.next_airing_episode.clone()?;
            let is_weekly = matches!(f.media.format.as_deref(), Some("TV") | Some("ONA"));
            let mut slots = weekly(next.airing_at, next.episode, f.media.episodes, is_weekly, from, to);
            if count_is_behind(&f.media, next.airing_at, next.episode) {
                for slot in &mut slots {
                    slot.1 = UNKNOWN_EPISODE;
                }
            }
            (!slots.is_empty()).then_some((f, slots))
        })
        .collect()
}

fn weekday(day: &str) -> Option<i64> {
    // Days since 1970-01-01 (a Thursday) modulo 7: Thursday = 0.
    Some(match day {
        "thursday" => 0,
        "friday" => 1,
        "saturday" => 2,
        "sunday" => 3,
        "monday" => 4,
        "tuesday" => 5,
        "wednesday" => 6,
        _ => return None,
    })
}

/// The first broadcast of a MAL entry as Unix seconds: its start date at its weekly slot, moved
/// to the slot's weekday when the date is another day. A date one day after the slot's weekday
/// is a late-night slot (Thursday 23:56 starting "Friday") and moves back a day.
pub fn mal_first_broadcast(node: &Value) -> Option<i64> {
    let (year, month, day) = node["start_date"].as_str().and_then(parse_date)?;
    let wanted = weekday(node["broadcast"]["day_of_the_week"].as_str()?)?;
    let time = node["broadcast"]["start_time"].as_str()?;
    let hour: i64 = time.get(0..2)?.parse().ok()?;
    let minute: i64 = time.get(3..5)?.parse().ok()?;
    let mut days = days_from_civil(year, month?, day?);
    days += match (wanted - days.rem_euclid(7)).rem_euclid(7) {
        6 => -1,
        ahead => ahead,
    };
    Some(days * 86_400 + hour * 3600 + minute * 60 - JST_OFFSET_S)
}

/// MAL's seasonal list: every airing show with a broadcast slot, numbered from its start date.
pub fn from_mal(json: &Value, from: i64, to: i64) -> Vec<Slots> {
    json["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let node = &item["node"];
            if node["status"].as_str() != Some("currently_airing") {
                return None;
            }
            let first = mal_first_broadcast(node)?;
            let found = super::mal::parse_node(node)?;
            let mut slots = weekly(first, 1, found.media.episodes, true, from, to);
            // A long-running show's count since its start ignores years of breaks: its slots
            // are kept without a number.
            if found.media.episodes.is_none() && slots.iter().any(|(_, ep)| *ep > MAX_UNCOUNTED_EPISODE) {
                for slot in &mut slots {
                    slot.1 = UNKNOWN_EPISODE;
                }
            }
            (!slots.is_empty()).then_some((found, slots))
        })
        .collect()
}

/// `primary` plus the anime of `extra` it does not have (the same MAL id is the same anime).
/// Each fallback lists only part of the week, so the schedule is built from both.
pub fn merge(mut primary: Vec<Slots>, extra: Vec<Slots>) -> Vec<Slots> {
    let known: std::collections::HashSet<i64> = primary.iter().filter_map(|(f, _)| f.ids.mal).collect();
    primary.extend(extra.into_iter().filter(|(f, _)| f.ids.mal.map_or(true, |mal| !known.contains(&mal))));
    primary
}

/// A fallback airing's id: negative, so it never equals one of AniList's, and stable for the
/// same anime and episode.
pub fn entry_id(media_id: i64, episode: i32) -> i64 {
    -(media_id.abs() * 10_000 + episode as i64)
}

/// The id of an airing without an episode number: one per anime and week, kept above 5 000 so it
/// stays clear of the numbered ids of an anime (which never has both kinds in one schedule).
fn unnumbered_entry_id(media_id: i64, airing_at: i64) -> i64 {
    -(media_id.abs() * 10_000 + 5_000 + airing_at.div_euclid(WEEK_S).rem_euclid(5_000))
}

/// The entries of anime whose id is known, sorted by air time.
pub fn entries(slots: Vec<Slots>, hide_adult: bool) -> Vec<AiringEntry> {
    let mut out: Vec<AiringEntry> = slots
        .into_iter()
        .filter(|(f, _)| f.media.id != 0 && !(hide_adult && f.media.is_adult))
        .flat_map(|(f, slots)| {
            let m = f.media;
            slots.into_iter().map(move |(at, episode)| AiringEntry {
                id: if episode == UNKNOWN_EPISODE {
                    unnumbered_entry_id(m.id as i64, at)
                } else {
                    entry_id(m.id as i64, episode)
                },
                airing_at: at,
                episode,
                media_id: m.id as i64,
                media_title: m.title.preferred(),
                cover_image: m.cover_image.extra_large.clone().or_else(|| m.cover_image.large.clone()),
                banner_image: m.banner_image.clone(),
                format: m.format.clone(),
                duration: m.duration,
                average_score: m.average_score,
            })
        })
        .collect();
    out.sort_by_key(|e| e.airing_at);
    out
}
