//! Release notifications: the planning rules, the notifier driven with a fake schedule and a
//! fake display, its tables, and the AniList request against a loopback server.

use anix_lib::notifier;
use anix_lib::db;
use anix_lib::error::AppError;
use anix_lib::models::NotificationPreference;
use async_trait::async_trait;
use notifier::{
    Airing, AniListSchedule, Bell, NotificationSink, Notifier, Plan, PlanInput, ReleaseNotification,
    ScheduleSource, Snapshot, StepOutcome, CATCH_UP_WINDOW_S, LOOKAHEAD_S, LOOKBACK_S,
    MIN_FETCH_GAP_S, RATE_LIMIT_MAX_S, REFRESH_INTERVAL_S, RETRY_BASE_S, RETRY_MAX_S, WATCH_TICK_S,
};
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Response, Server, StatusCode};

/// An arbitrary "now" in Unix seconds.
const T: i64 = 1_800_000_000;
const HOUR: i64 = 3600;

const SHOW_A: i64 = 101;
const SHOW_B: i64 = 202;

fn bell(media_id: i64, enabled_at: i64) -> Bell {
    Bell {
        media_id,
        media_title: format!("Show {}", media_id),
        cover_image: Some(format!("https://img.example/{}.jpg", media_id)),
        enabled_at,
    }
}

fn airing(media_id: i64, episode: i32, airing_at: i64) -> Airing {
    Airing { media_id, episode, airing_at }
}

fn snapshot(fetched_at: i64, media_ids: &[i64], airings: Vec<Airing>) -> Snapshot {
    Snapshot {
        fetched_at,
        media_ids: media_ids.iter().copied().collect(),
        airings,
    }
}

/// Everything `plan` looks at, with defaults for the parts a test does not care about.
struct Case {
    now: i64,
    enabled: bool,
    silenced_until: i64,
    bells: Vec<Bell>,
    snapshot: Option<Snapshot>,
    notified: HashSet<(i64, i32)>,
    next_fetch_allowed_at: i64,
}

impl Case {
    fn at(now: i64) -> Self {
        Self {
            now,
            enabled: true,
            silenced_until: 0,
            bells: vec![bell(SHOW_A, T - 30 * 24 * HOUR)],
            snapshot: None,
            notified: HashSet::new(),
            next_fetch_allowed_at: 0,
        }
    }

    fn with_snapshot(mut self, fetched_at: i64, airings: Vec<Airing>) -> Self {
        let ids: Vec<i64> = self.bells.iter().map(|b| b.media_id).collect();
        self.snapshot = Some(snapshot(fetched_at, &ids, airings));
        self
    }

    fn plan(&self) -> Plan {
        notifier::plan(&PlanInput {
            now: self.now,
            enabled: self.enabled,
            silenced_until: self.silenced_until,
            bells: &self.bells,
            snapshot: self.snapshot.as_ref(),
            notified: &self.notified,
            next_fetch_allowed_at: self.next_fetch_allowed_at,
        })
    }
}

fn fired(plan: &Plan) -> Vec<(i64, Vec<i32>)> {
    plan.fire
        .iter()
        .map(|n| (n.media_id, n.episodes.iter().map(|a| a.episode).collect()))
        .collect()
}

// ---------------------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------------------

#[test]
fn plan_reads_the_schedule_before_anything_else() {
    let plan = Case::at(T).plan();
    assert!(plan.refresh);
    assert_eq!(plan.wake_at, T);
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());
}

#[test]
fn plan_without_bells_does_nothing() {
    let mut case = Case::at(T).with_snapshot(T, vec![airing(SHOW_A, 12, T - 60)]);
    case.bells.clear();
    let plan = case.plan();
    assert!(!plan.refresh);
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());
    assert_eq!(plan.wake_at, T + REFRESH_INTERVAL_S);
}

#[test]
fn plan_with_the_setting_off_does_nothing() {
    // A confirmed, fresh episode that would otherwise fire.
    let mut case = Case::at(T).with_snapshot(T, vec![airing(SHOW_A, 12, T - 60)]);
    assert_eq!(fired(&case.plan()), vec![(SHOW_A, vec![12])]);

    case.enabled = false;
    let plan = case.plan();
    assert!(!plan.refresh, "nothing is read while notifications are off");
    assert!(plan.fire.is_empty());
    assert!(plan.mark_seen.is_empty());
}

#[test]
fn plan_wakes_at_the_nearest_air_time() {
    let case = Case::at(T + 10).with_snapshot(
        T,
        vec![airing(SHOW_A, 13, T + 20 * 60), airing(SHOW_A, 12, T + 10 * 60)],
    );
    let plan = case.plan();
    assert!(!plan.refresh);
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());
    assert_eq!(plan.wake_at, T + 10 * 60);
}

#[test]
fn plan_wakes_for_the_periodic_read_when_nothing_airs_sooner() {
    let far = Case::at(T + 10).with_snapshot(T, vec![airing(SHOW_A, 12, T + 5 * HOUR)]);
    assert_eq!(far.plan().wake_at, T + REFRESH_INTERVAL_S);

    // A finished show has no entries at all: no noise, just the next periodic read.
    let finished = Case::at(T + 10).with_snapshot(T, vec![]);
    let plan = finished.plan();
    assert!(!plan.refresh && plan.fire.is_empty() && plan.mark_seen.is_empty());
    assert_eq!(plan.wake_at, T + REFRESH_INTERVAL_S);

    let stale = Case::at(T + REFRESH_INTERVAL_S).with_snapshot(T, vec![]);
    let plan = stale.plan();
    assert!(plan.refresh);
    assert_eq!(plan.wake_at, T + REFRESH_INTERVAL_S);
}

#[test]
fn plan_confirms_an_air_time_before_announcing_it() {
    let air = T + 600;

    // The air time has passed, but the schedule was last read before it.
    let unconfirmed = Case::at(air).with_snapshot(T, vec![airing(SHOW_A, 12, air)]);
    let plan = unconfirmed.plan();
    assert!(plan.refresh);
    assert_eq!(plan.wake_at, air);
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());

    // Read again at the air time and still listed: announced.
    let confirmed = Case::at(air).with_snapshot(air, vec![airing(SHOW_A, 12, air)]);
    let plan = confirmed.plan();
    assert!(!plan.refresh);
    assert_eq!(plan.fire.len(), 1);
    assert_eq!(plan.fire[0].title, "Show 101");
    assert_eq!(plan.fire[0].body(), "Episode 12 is out");
    assert_eq!(plan.fire[0].cover_image.as_deref(), Some("https://img.example/101.jpg"));
    assert_eq!(plan.fire[0].episodes, vec![airing(SHOW_A, 12, air)]);
    assert!(plan.mark_seen.is_empty());
    assert_eq!(plan.wake_at, air + REFRESH_INTERVAL_S);
}

#[test]
fn plan_never_repeats_a_recorded_episode() {
    let air = T + 600;
    let mut case = Case::at(air + 30).with_snapshot(air, vec![airing(SHOW_A, 12, air)]);
    case.notified.insert((SHOW_A, 12));
    let plan = case.plan();
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());
    assert!(!plan.refresh);

    // The same episode moved to a later time after it was announced: not announced again,
    // and not something to wake up for.
    let mut shifted = Case::at(air + 60).with_snapshot(air + 40, vec![airing(SHOW_A, 12, air + 300)]);
    shifted.notified.insert((SHOW_A, 12));
    let plan = shifted.plan();
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());
    assert_eq!(plan.wake_at, air + 40 + REFRESH_INTERVAL_S);

    let mut aired_again = Case::at(air + 300).with_snapshot(air + 300, vec![airing(SHOW_A, 12, air + 300)]);
    aired_again.notified.insert((SHOW_A, 12));
    assert!(aired_again.plan().fire.is_empty());
}

#[test]
fn plan_follows_an_episode_that_is_delayed() {
    let air = T + 600;
    let delayed = T + 3 * HOUR;

    // The delay shows up in a periodic read before the old air time.
    let known = Case::at(air).with_snapshot(T + 300, vec![airing(SHOW_A, 12, delayed)]);
    let plan = known.plan();
    assert!(plan.fire.is_empty() && !plan.refresh);
    assert_eq!(plan.wake_at, T + 300 + REFRESH_INTERVAL_S);

    // The delay only shows up in the read made at the old air time.
    let found_late = Case::at(air).with_snapshot(air, vec![airing(SHOW_A, 12, delayed)]);
    let plan = found_late.plan();
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty() && !plan.refresh);

    // It is announced once the new time is confirmed.
    let aired = Case::at(delayed).with_snapshot(delayed, vec![airing(SHOW_A, 12, delayed)]);
    assert_eq!(fired(&aired.plan()), vec![(SHOW_A, vec![12])]);
}

#[test]
fn plan_follows_an_episode_that_is_moved_earlier() {
    // Moved from three hours away to five minutes away.
    let sooner = Case::at(T + 10).with_snapshot(T, vec![airing(SHOW_A, 12, T + 300)]);
    assert_eq!(sooner.plan().wake_at, T + 300);

    // Moved to a time that has already passed: announced by the read that reveals it.
    let already_out = Case::at(T + REFRESH_INTERVAL_S)
        .with_snapshot(T + REFRESH_INTERVAL_S, vec![airing(SHOW_A, 12, T + 900)]);
    assert_eq!(fired(&already_out.plan()), vec![(SHOW_A, vec![12])]);
}

#[test]
fn plan_drops_a_cancelled_episode() {
    let air = T + 600;
    // Planned for `air`, then gone from the read made at that time.
    let cancelled = Case::at(air).with_snapshot(air, vec![]);
    let plan = cancelled.plan();
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty() && !plan.refresh);
    assert_eq!(plan.wake_at, air + REFRESH_INTERVAL_S);
}

#[test]
fn plan_uses_the_later_time_of_an_episode_listed_twice() {
    let case = Case::at(T).with_snapshot(T, vec![airing(SHOW_A, 12, T - 600), airing(SHOW_A, 12, T + 600)]);
    let plan = case.plan();
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());
    assert_eq!(plan.wake_at, T + 600);
}

#[test]
fn plan_catch_up_window() {
    let within = Case::at(T).with_snapshot(T, vec![airing(SHOW_A, 12, T - 2 * HOUR)]);
    let plan = within.plan();
    assert_eq!(fired(&plan), vec![(SHOW_A, vec![12])]);
    assert!(plan.mark_seen.is_empty());

    let at_the_edge = Case::at(T).with_snapshot(T, vec![airing(SHOW_A, 12, T - CATCH_UP_WINDOW_S)]);
    assert_eq!(fired(&at_the_edge.plan()), vec![(SHOW_A, vec![12])]);

    let too_old = Case::at(T).with_snapshot(T, vec![airing(SHOW_A, 12, T - CATCH_UP_WINDOW_S - 1)]);
    let plan = too_old.plan();
    assert!(plan.fire.is_empty());
    assert_eq!(plan.mark_seen, vec![airing(SHOW_A, 12, T - CATCH_UP_WINDOW_S - 1)]);
}

#[test]
fn plan_skips_what_aired_before_the_bell_was_switched_on() {
    let mut case = Case::at(T).with_snapshot(T, vec![airing(SHOW_A, 11, T - 601), airing(SHOW_A, 12, T - 600)]);
    case.bells = vec![bell(SHOW_A, T - 600)];
    let plan = case.plan();
    // Episode 11 aired a second before the bell; episode 12 at the very second it was set.
    assert_eq!(plan.mark_seen, vec![airing(SHOW_A, 11, T - 601)]);
    assert_eq!(fired(&plan), vec![(SHOW_A, vec![12])]);
}

#[test]
fn plan_skips_what_aired_while_notifications_were_off() {
    let mut case = Case::at(T).with_snapshot(T, vec![airing(SHOW_A, 11, T - 600), airing(SHOW_A, 12, T - 599)]);
    case.silenced_until = T - 600;
    let plan = case.plan();
    assert_eq!(plan.mark_seen, vec![airing(SHOW_A, 11, T - 600)]);
    assert_eq!(fired(&plan), vec![(SHOW_A, vec![12])]);
}

#[test]
fn plan_ignores_anime_without_a_bell() {
    let case = Case::at(T).with_snapshot(T, vec![airing(SHOW_B, 3, T - 60), airing(SHOW_B, 4, T + 60)]);
    let plan = case.plan();
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());
    assert_eq!(plan.wake_at, T + REFRESH_INTERVAL_S);
}

#[test]
fn plan_reads_again_for_a_new_bell() {
    let mut case = Case::at(T + 60).with_snapshot(T, vec![]);
    case.bells.push(bell(SHOW_B, T + 60));
    let plan = case.plan();
    assert!(plan.refresh);
    assert_eq!(plan.wake_at, T + 60);

    // Too soon after the previous read: wait for the gap, do not read now.
    case.now = T + 2;
    case.next_fetch_allowed_at = T + MIN_FETCH_GAP_S;
    let plan = case.plan();
    assert!(!plan.refresh);
    assert_eq!(plan.wake_at, T + MIN_FETCH_GAP_S);
}

#[test]
fn plan_after_a_suspend_past_the_air_time() {
    let air = T + HOUR;

    // Woken three hours later with the snapshot from before the suspend.
    let resumed = Case::at(T + 3 * HOUR).with_snapshot(T, vec![airing(SHOW_A, 12, air)]);
    let plan = resumed.plan();
    assert!(plan.refresh);
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty(), "nothing is decided on the old snapshot");

    // The read after resuming confirms it: announced late, once.
    let caught_up = Case::at(T + 3 * HOUR).with_snapshot(T + 3 * HOUR, vec![airing(SHOW_A, 12, air)]);
    assert_eq!(fired(&caught_up.plan()), vec![(SHOW_A, vec![12])]);

    // Suspended for much longer than the catch-up window: recorded, not announced.
    let much_later = T + HOUR + CATCH_UP_WINDOW_S + 60;
    let too_late = Case::at(much_later).with_snapshot(much_later, vec![airing(SHOW_A, 12, air)]);
    let plan = too_late.plan();
    assert!(plan.fire.is_empty());
    assert_eq!(plan.mark_seen, vec![airing(SHOW_A, 12, air)]);
}

#[test]
fn plan_waits_out_a_back_off() {
    let air = T + 600;
    let mut case = Case::at(air).with_snapshot(T, vec![airing(SHOW_A, 12, air)]);
    case.next_fetch_allowed_at = air + 120;
    let plan = case.plan();
    assert!(!plan.refresh);
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty());
    assert_eq!(plan.wake_at, air + 120);

    case.now = air + 120;
    assert!(case.plan().refresh);
}

#[test]
fn plan_after_the_clock_is_set_back() {
    // The snapshot claims to be from ten minutes ahead of now.
    let mut case = Case::at(T).with_snapshot(T + 600, vec![airing(SHOW_A, 12, T - 60)]);
    case.next_fetch_allowed_at = T + 600 + MIN_FETCH_GAP_S;
    let plan = case.plan();
    assert!(plan.fire.is_empty() && plan.mark_seen.is_empty(), "nothing in it counts as confirmed");
    assert!(!plan.refresh);
    assert_eq!(plan.wake_at, T + 600 + MIN_FETCH_GAP_S);

    // A wait no back-off could have asked for is not honoured.
    case.next_fetch_allowed_at = T + RATE_LIMIT_MAX_S + 1;
    let plan = case.plan();
    assert!(plan.refresh);
    assert_eq!(plan.wake_at, T);
}

#[test]
fn plan_groups_episodes_of_one_anime_and_orders_by_air_time() {
    let mut case = Case::at(T);
    case.bells.push(bell(SHOW_B, T - 30 * 24 * HOUR));
    let case = case.with_snapshot(
        T,
        vec![
            airing(SHOW_A, 2, T - 60),
            airing(SHOW_B, 7, T - 120),
            airing(SHOW_A, 1, T - 60),
        ],
    );
    let plan = case.plan();
    assert_eq!(fired(&plan), vec![(SHOW_B, vec![7]), (SHOW_A, vec![1, 2])]);
    assert_eq!(plan.fire[0].body(), "Episode 7 is out");
    assert_eq!(plan.fire[1].body(), "Episodes 1 and 2 are out");
}

#[test]
fn notification_body_wording() {
    let with = |episodes: &[i32]| ReleaseNotification {
        media_id: SHOW_A,
        title: "Show".to_string(),
        cover_image: None,
        episodes: episodes.iter().map(|e| airing(SHOW_A, *e, T)).collect(),
    };
    assert_eq!(with(&[12]).body(), "Episode 12 is out");
    assert_eq!(with(&[2, 1]).body(), "Episodes 1 and 2 are out");
    assert_eq!(with(&[3, 1, 2]).body(), "Episodes 1–3 are out");
}

// ---------------------------------------------------------------------------------------
// Small rules
// ---------------------------------------------------------------------------------------

#[test]
fn retry_delay_backs_off_and_respects_the_server() {
    let offline = AppError::Unreachable("graphql.anilist.co: no route".to_string());
    assert_eq!(notifier::retry_delay_s(1, &offline), RETRY_BASE_S);
    assert_eq!(notifier::retry_delay_s(2, &offline), RETRY_BASE_S * 2);
    assert_eq!(notifier::retry_delay_s(3, &offline), RETRY_BASE_S * 4);
    assert_eq!(notifier::retry_delay_s(20, &offline), RETRY_MAX_S);
    assert_eq!(notifier::retry_delay_s(u32::MAX, &offline), RETRY_MAX_S);
    assert_eq!(notifier::retry_delay_s(0, &offline), RETRY_BASE_S);

    let limited = |retry_after_s| AppError::RateLimited { retry_after_s };
    assert_eq!(notifier::retry_delay_s(1, &limited(120)), 120);
    assert_eq!(notifier::retry_delay_s(1, &limited(5)), RETRY_BASE_S);
    assert_eq!(notifier::retry_delay_s(1, &limited(86_400)), RATE_LIMIT_MAX_S);
    assert_eq!(notifier::retry_delay_s(1, &limited(u64::MAX)), RATE_LIMIT_MAX_S);
}

#[test]
fn sleep_is_cut_into_short_slices_while_a_bell_is_on() {
    let watching = |wake_at| StepOutcome { wake_at, watching: true };
    assert_eq!(notifier::sleep_seconds(watching(T + 10), T), 10);
    assert_eq!(notifier::sleep_seconds(watching(T + HOUR), T), WATCH_TICK_S as u64);
    // Already due, or due in the past after a jump of the clock: one second, never a busy loop.
    assert_eq!(notifier::sleep_seconds(watching(T), T), 1);
    assert_eq!(notifier::sleep_seconds(watching(T - 5 * HOUR), T), 1);

    // Nothing to watch: sleep until a bell is set, checking in at the refresh interval.
    let idle = StepOutcome { wake_at: T + REFRESH_INTERVAL_S, watching: false };
    assert_eq!(notifier::sleep_seconds(idle, T), REFRESH_INTERVAL_S as u64);
}

#[test]
fn only_a_click_opens_the_anime() {
    use notifier::Click;
    use notify_rust::{CloseReason, NotificationResponse};

    assert_eq!(notifier::click_for(SHOW_A, &NotificationResponse::Default), Click::OpenAnime(SHOW_A));
    // Some servers report the default action by its key.
    let by_key = NotificationResponse::Action(notifier::OPEN_ACTION.to_string());
    assert_eq!(notifier::click_for(SHOW_A, &by_key), Click::OpenAnime(SHOW_A));

    for reason in [CloseReason::Expired, CloseReason::Dismissed, CloseReason::CloseAction, CloseReason::Other(9)] {
        assert_eq!(notifier::click_for(SHOW_A, &NotificationResponse::Closed(reason)), Click::Nothing);
    }
    let other = NotificationResponse::Action("snooze".to_string());
    assert_eq!(notifier::click_for(SHOW_A, &other), Click::Nothing);
    assert_eq!(notifier::click_for(SHOW_A, &NotificationResponse::Reply("hi".to_string())), Click::Nothing);

    // The debug sample notification has no anime: a click only brings the window forward.
    assert_eq!(notifier::click_for(0, &NotificationResponse::Default), Click::Focus);
    assert_eq!(notifier::click_for(0, &NotificationResponse::Closed(CloseReason::Dismissed)), Click::Nothing);
}

#[test]
fn schedule_ids_are_valid_distinct_and_sorted() {
    let bells: Vec<Bell> = [9, 0, 7, 2, 9, -1].iter().map(|id| bell(*id, T)).collect();
    assert_eq!(notifier::schedule_media_ids(&bells), vec![2, 7, 9]);
}

#[test]
fn release_notifications_setting_defaults_to_on() {
    assert!(notifier::release_notifications_enabled(&json!({})));
    assert!(notifier::release_notifications_enabled(&json!({ "release_notifications": true })));
    assert!(!notifier::release_notifications_enabled(&json!({ "release_notifications": false })));
    assert!(notifier::release_notifications_enabled(&json!({ "release_notifications": "no" })));
    assert!(notifier::release_notifications_enabled(&json!([])));
}

#[test]
fn airing_page_parsing() {
    let data = json!({
        "Page": {
            "pageInfo": { "hasNextPage": true },
            "airingSchedules": [
                { "mediaId": 101, "episode": 12, "airingAt": 1_800_000_600 },
                { "mediaId": 0, "episode": 1, "airingAt": 1_800_000_600 },
                { "mediaId": 202, "episode": null, "airingAt": 1_800_000_600 },
                { "mediaId": 202, "episode": 3 },
                { "mediaId": 202, "episode": 4, "airingAt": 1_800_000_700 }
            ]
        }
    });
    let (airings, has_next_page) = notifier::parse_airing_page(&data).unwrap();
    assert!(has_next_page);
    assert_eq!(airings, vec![airing(101, 12, 1_800_000_600), airing(202, 4, 1_800_000_700)]);

    let (airings, has_next_page) =
        notifier::parse_airing_page(&json!({ "Page": { "airingSchedules": [] } })).unwrap();
    assert!(airings.is_empty() && !has_next_page);

    assert!(matches!(
        notifier::parse_airing_page(&json!({ "Page": null })),
        Err(AppError::Parse(_))
    ));
}

#[test]
fn cover_icons_are_jpeg_or_png_only() {
    assert_eq!(notifier::cover_extension(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]), Some("jpg"));
    assert_eq!(
        notifier::cover_extension(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00]),
        Some("png")
    );
    assert_eq!(notifier::cover_extension(b"RIFF\x00\x00\x00\x00WEBP"), None);
    assert_eq!(notifier::cover_extension(b"<html>"), None);
    assert_eq!(notifier::cover_extension(&[]), None);
}

// ---------------------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------------------

async fn memory_db() -> SqlitePool {
    db::init_db_in_memory().await.expect("in-memory database")
}

fn pref(media_id: i64, enabled: bool) -> NotificationPreference {
    NotificationPreference {
        media_id,
        enabled,
        media_title: format!("Show {}", media_id),
        cover_image: Some(format!("https://img.example/{}.jpg", media_id)),
    }
}

async fn set_bell(pool: &SqlitePool, media_id: i64, enabled: bool, now: i64) {
    db::save_notification_preference_at(pool, &pref(media_id, enabled), now)
        .await
        .expect("save bell");
}

/// `(shown, airing_at)` of a recorded episode.
async fn recorded(pool: &SqlitePool, media_id: i64, episode: i32) -> Option<(bool, i64)> {
    sqlx::query("SELECT shown, airing_at FROM notified_episodes WHERE media_id = ?1 AND episode = ?2")
        .bind(media_id)
        .bind(episode)
        .fetch_optional(pool)
        .await
        .expect("query notified_episodes")
        .map(|r| (r.get::<i64, _>("shown") != 0, r.get("airing_at")))
}

#[tokio::test]
async fn bell_keeps_the_time_it_was_switched_on() {
    let pool = memory_db().await;

    set_bell(&pool, SHOW_A, true, T).await;
    assert_eq!(db::list_notification_bells(&pool).await.unwrap(), vec![bell(SHOW_A, T)]);

    // Saving a bell that is already on does not move its switch-on time.
    set_bell(&pool, SHOW_A, true, T + 500).await;
    assert_eq!(db::list_notification_bells(&pool).await.unwrap()[0].enabled_at, T);

    set_bell(&pool, SHOW_A, false, T + 600).await;
    assert!(db::list_notification_bells(&pool).await.unwrap().is_empty());
    assert!(db::list_notification_preferences(&pool).await.unwrap().is_empty());

    set_bell(&pool, SHOW_A, true, T + 700).await;
    set_bell(&pool, SHOW_B, true, T + 800).await;
    assert_eq!(
        db::list_notification_bells(&pool).await.unwrap(),
        vec![bell(SHOW_A, T + 700), bell(SHOW_B, T + 800)]
    );
    assert_eq!(db::list_notification_preferences(&pool).await.unwrap().len(), 2);
}

#[tokio::test]
async fn notified_episodes_are_recorded_once() {
    let pool = memory_db().await;

    assert!(db::record_notified_episode(&pool, SHOW_A, 12, T, T + 1, true).await.unwrap());
    // A second record for the same episode changes nothing, whatever it says.
    assert!(!db::record_notified_episode(&pool, SHOW_A, 12, T + 900, T + 901, false).await.unwrap());
    assert!(db::record_notified_episode(&pool, SHOW_A, 13, T, T + 1, false).await.unwrap());

    assert_eq!(recorded(&pool, SHOW_A, 12).await, Some((true, T)));
    assert_eq!(recorded(&pool, SHOW_A, 13).await, Some((false, T)));

    let mut all = db::list_notified_episodes(&pool).await.unwrap();
    all.sort_unstable();
    assert_eq!(all, vec![(SHOW_A, 12), (SHOW_A, 13)]);
}

#[tokio::test]
async fn notifier_state_round_trip() {
    let pool = memory_db().await;
    assert_eq!(db::get_notifier_state(&pool, "silenced_until").await.unwrap(), None);
    db::set_notifier_state(&pool, "silenced_until", T).await.unwrap();
    db::set_notifier_state(&pool, "silenced_until", T + 5).await.unwrap();
    assert_eq!(db::get_notifier_state(&pool, "silenced_until").await.unwrap(), Some(T + 5));
}

#[tokio::test]
async fn migration_gives_older_bells_a_switch_on_time() {
    // The table as shipped before release notifications existed.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(SqliteConnectOptions::new().in_memory(true))
        .await
        .expect("raw pool");
    sqlx::query(
        r#"
        CREATE TABLE notification_preferences (
            media_id      INTEGER PRIMARY KEY,
            enabled       INTEGER NOT NULL DEFAULT 1,
            media_title   TEXT NOT NULL,
            cover_image   TEXT
        );
        INSERT INTO notification_preferences (media_id, enabled, media_title, cover_image)
        VALUES (101, 1, 'Show 101', 'https://img.example/101.jpg'), (202, 0, 'Show 202', NULL);
        "#,
    )
    .execute(&pool)
    .await
    .expect("old table");

    let before = db::current_unix_timestamp();
    db::run_migrations(&pool).await.expect("migrate");
    let after = db::current_unix_timestamp();

    let bells = db::list_notification_bells(&pool).await.unwrap();
    assert_eq!(bells.len(), 1);
    assert_eq!(bells[0].media_id, 101);
    assert_eq!(bells[0].media_title, "Show 101");
    assert_eq!(bells[0].cover_image.as_deref(), Some("https://img.example/101.jpg"));
    assert!((before..=after).contains(&bells[0].enabled_at), "old bells count from the upgrade");

    for table in ["notified_episodes", "notifier_state"] {
        let n: i64 = sqlx::query("SELECT COUNT(*) AS n FROM sqlite_master WHERE type = 'table' AND name = ?1")
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("n");
        assert_eq!(n, 1, "table {} must exist", table);
    }

    // Running the migrations again leaves the switch-on time alone.
    sqlx::query("UPDATE notification_preferences SET enabled_at = 42 WHERE media_id = 101")
        .execute(&pool)
        .await
        .unwrap();
    db::run_migrations(&pool).await.expect("migrate again");
    assert_eq!(db::list_notification_bells(&pool).await.unwrap()[0].enabled_at, 42);
}

// ---------------------------------------------------------------------------------------
// The notifier, with a fake schedule and a fake display
// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct FakeSchedule {
    airings: Mutex<Vec<Airing>>,
    failure: Mutex<Option<AppError>>,
    /// `(media_ids, from_ts, to_ts)` of every read.
    reads: Mutex<Vec<(Vec<i64>, i64, i64)>>,
}

impl FakeSchedule {
    fn set(&self, airings: Vec<Airing>) {
        *self.airings.lock().unwrap() = airings;
    }

    fn fail_with(&self, failure: Option<AppError>) {
        *self.failure.lock().unwrap() = failure;
    }

    fn read_count(&self) -> usize {
        self.reads.lock().unwrap().len()
    }

    fn last_read(&self) -> (Vec<i64>, i64, i64) {
        self.reads.lock().unwrap().last().cloned().expect("at least one read")
    }
}

#[async_trait]
impl ScheduleSource for FakeSchedule {
    async fn fetch(&self, media_ids: &[i64], from_ts: i64, to_ts: i64) -> Result<Vec<Airing>, AppError> {
        self.reads.lock().unwrap().push((media_ids.to_vec(), from_ts, to_ts));
        if let Some(failure) = self.failure.lock().unwrap().clone() {
            return Err(failure);
        }
        // Like the real schedule, only what was asked for.
        Ok(self
            .airings
            .lock()
            .unwrap()
            .iter()
            .filter(|a| media_ids.contains(&a.media_id) && a.airing_at > from_ts && a.airing_at < to_ts)
            .cloned()
            .collect())
    }
}

#[derive(Default)]
struct FakeDisplay {
    shown: Mutex<Vec<ReleaseNotification>>,
}

impl FakeDisplay {
    /// `(title, body)` of everything shown so far.
    fn shown(&self) -> Vec<(String, String)> {
        self.shown.lock().unwrap().iter().map(|n| (n.title.clone(), n.body())).collect()
    }
}

#[async_trait]
impl NotificationSink for FakeDisplay {
    async fn show(&self, notification: &ReleaseNotification) {
        self.shown.lock().unwrap().push(notification.clone());
    }
}

fn out(title: &str, body: &str) -> (String, String) {
    (title.to_string(), body.to_string())
}

#[tokio::test]
async fn notifier_announces_an_episode_exactly_once() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();
    let air = T + 600;

    set_bell(&pool, SHOW_A, true, T - 100).await;
    schedule.set(vec![airing(SHOW_A, 12, air)]);

    let mut notifier = Notifier::load(&pool).await.unwrap();
    let outcome = notifier.step(&pool, &schedule, &display, true, T).await;
    assert!(outcome.watching);
    assert_eq!(outcome.wake_at, air);
    assert_eq!(schedule.last_read(), (vec![SHOW_A], T - LOOKBACK_S, T + LOOKAHEAD_S));
    assert!(display.shown().is_empty());

    // A check in between reads nothing and shows nothing.
    notifier.step(&pool, &schedule, &display, true, T + 300).await;
    assert_eq!(schedule.read_count(), 1);
    assert!(display.shown().is_empty());

    // At the air time the schedule is read once more, then the notification goes out.
    let outcome = notifier.step(&pool, &schedule, &display, true, air).await;
    assert_eq!(schedule.read_count(), 2);
    assert_eq!(display.shown(), vec![out("Show 101", "Episode 12 is out")]);
    assert_eq!(recorded(&pool, SHOW_A, 12).await, Some((true, air)));
    assert_eq!(outcome.wake_at, air + REFRESH_INTERVAL_S);

    // Later checks, including ones that read the schedule again, do not repeat it.
    notifier.step(&pool, &schedule, &display, true, air + 30).await;
    notifier.step(&pool, &schedule, &display, true, air + REFRESH_INTERVAL_S).await;
    assert_eq!(schedule.read_count(), 3);
    assert_eq!(display.shown().len(), 1);

    // Neither does a restart...
    let mut restarted = Notifier::load(&pool).await.unwrap();
    restarted.step(&pool, &schedule, &display, true, air + 2 * HOUR).await;
    assert_eq!(schedule.read_count(), 4);
    assert_eq!(display.shown().len(), 1);

    // ...nor the episode moving to a new time afterwards.
    let moved = air + 3 * HOUR;
    schedule.set(vec![airing(SHOW_A, 12, moved)]);
    let mut restarted = Notifier::load(&pool).await.unwrap();
    restarted.step(&pool, &schedule, &display, true, moved - 60).await;
    restarted.step(&pool, &schedule, &display, true, moved).await;
    restarted.step(&pool, &schedule, &display, true, moved + REFRESH_INTERVAL_S).await;
    assert_eq!(display.shown().len(), 1);
    assert_eq!(recorded(&pool, SHOW_A, 12).await, Some((true, air)));
}

#[tokio::test]
async fn notifier_survives_a_lost_memory_of_what_it_announced() {
    // Even a notifier that starts with no memory at all cannot repeat a recorded episode.
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();

    set_bell(&pool, SHOW_A, true, T - HOUR).await;
    schedule.set(vec![airing(SHOW_A, 12, T - 60)]);
    db::record_notified_episode(&pool, SHOW_A, 12, T - 60, T - 59, true).await.unwrap();

    let mut forgetful = Notifier::default();
    forgetful.step(&pool, &schedule, &display, true, T).await;
    assert!(display.shown().is_empty());
}

#[tokio::test]
async fn notifier_catches_up_on_launch_within_the_window_only() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();

    // Both bells were set long ago; the app was closed while these aired.
    set_bell(&pool, SHOW_A, true, T - 20 * HOUR).await;
    set_bell(&pool, SHOW_B, true, T - 20 * HOUR).await;
    let recent = T - 2 * HOUR;
    let old = T - 8 * HOUR;
    schedule.set(vec![airing(SHOW_A, 5, recent), airing(SHOW_B, 9, old), airing(SHOW_B, 10, T + HOUR)]);

    let mut notifier = Notifier::load(&pool).await.unwrap();
    let outcome = notifier.step(&pool, &schedule, &display, true, T).await;

    assert_eq!(display.shown(), vec![out("Show 101", "Episode 5 is out")]);
    assert_eq!(recorded(&pool, SHOW_A, 5).await, Some((true, recent)));
    assert_eq!(recorded(&pool, SHOW_B, 9).await, Some((false, old)), "recorded without a notification");
    assert_eq!(recorded(&pool, SHOW_B, 10).await, None);
    assert_eq!(outcome.wake_at, T + REFRESH_INTERVAL_S);
}

#[tokio::test]
async fn notifier_does_not_announce_what_aired_before_the_bell() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();

    schedule.set(vec![airing(SHOW_A, 12, T - 600), airing(SHOW_A, 13, T + 600)]);
    set_bell(&pool, SHOW_A, true, T).await;

    let mut notifier = Notifier::load(&pool).await.unwrap();
    let outcome = notifier.step(&pool, &schedule, &display, true, T).await;
    assert!(display.shown().is_empty());
    assert_eq!(recorded(&pool, SHOW_A, 12).await, Some((false, T - 600)));
    assert_eq!(outcome.wake_at, T + 600);

    notifier.step(&pool, &schedule, &display, true, T + 600).await;
    assert_eq!(display.shown(), vec![out("Show 101", "Episode 13 is out")]);
}

#[tokio::test]
async fn notifier_backs_off_while_the_schedule_is_unreachable() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();
    let air = T + 600;

    set_bell(&pool, SHOW_A, true, T - 100).await;
    schedule.set(vec![airing(SHOW_A, 12, air)]);
    schedule.fail_with(Some(AppError::Unreachable("graphql.anilist.co: offline".to_string())));

    let mut notifier = Notifier::load(&pool).await.unwrap();
    let outcome = notifier.step(&pool, &schedule, &display, true, T).await;
    assert_eq!(schedule.read_count(), 1);
    assert_eq!(outcome.wake_at, T + RETRY_BASE_S);

    // Checks before the retry time do not touch the network.
    for now in [T + 1, T + 10, T + RETRY_BASE_S - 1] {
        let outcome = notifier.step(&pool, &schedule, &display, true, now).await;
        assert_eq!(outcome.wake_at, T + RETRY_BASE_S);
    }
    assert_eq!(schedule.read_count(), 1);

    // Each failure doubles the wait.
    let second = T + RETRY_BASE_S;
    let outcome = notifier.step(&pool, &schedule, &display, true, second).await;
    assert_eq!(schedule.read_count(), 2);
    assert_eq!(outcome.wake_at, second + 2 * RETRY_BASE_S);

    // A rate limit is waited out for as long as the server asks.
    schedule.fail_with(Some(AppError::RateLimited { retry_after_s: 600 }));
    let third = second + 2 * RETRY_BASE_S;
    let outcome = notifier.step(&pool, &schedule, &display, true, third).await;
    assert_eq!(schedule.read_count(), 3);
    assert_eq!(outcome.wake_at, third + 600);
    assert!(display.shown().is_empty());

    // The air time passed while offline. Nothing is announced on unconfirmed data...
    let outcome = notifier.step(&pool, &schedule, &display, true, air + 60).await;
    assert_eq!(schedule.read_count(), 3);
    assert!(display.shown().is_empty());
    assert_eq!(outcome.wake_at, third + 600);

    // ...and once the schedule is back, the episode is announced, late but once.
    schedule.fail_with(None);
    notifier.step(&pool, &schedule, &display, true, third + 600).await;
    assert_eq!(schedule.read_count(), 4);
    assert_eq!(display.shown(), vec![out("Show 101", "Episode 12 is out")]);

    // Back to the normal rhythm.
    let outcome = notifier.step(&pool, &schedule, &display, true, third + 601).await;
    assert_eq!(schedule.read_count(), 4);
    assert_eq!(outcome.wake_at, third + 600 + REFRESH_INTERVAL_S);
}

#[tokio::test]
async fn notifier_master_switch_silences_without_removing_bells() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();
    let air = T + 600;

    set_bell(&pool, SHOW_A, true, T - 100).await;
    schedule.set(vec![airing(SHOW_A, 12, air), airing(SHOW_A, 13, air + HOUR)]);

    let mut notifier = Notifier::load(&pool).await.unwrap();
    for now in [T, air, air + 100] {
        let outcome = notifier.step(&pool, &schedule, &display, false, now).await;
        assert!(outcome.watching);
    }
    assert_eq!(schedule.read_count(), 0, "nothing is read while notifications are off");
    assert!(display.shown().is_empty());
    assert_eq!(db::list_notification_bells(&pool).await.unwrap().len(), 1, "the bell stays");

    // Switched back on: what aired meanwhile is recorded without a notification.
    notifier.step(&pool, &schedule, &display, true, air + 130).await;
    assert_eq!(schedule.read_count(), 1);
    assert!(display.shown().is_empty());
    assert_eq!(recorded(&pool, SHOW_A, 12).await, Some((false, air)));

    // The next episode is announced as usual.
    notifier.step(&pool, &schedule, &display, true, air + HOUR).await;
    assert_eq!(display.shown(), vec![out("Show 101", "Episode 13 is out")]);
}

#[tokio::test]
async fn notifier_remembers_the_silence_across_a_restart() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();
    let air = T + 600;

    set_bell(&pool, SHOW_A, true, T - 100).await;
    schedule.set(vec![airing(SHOW_A, 12, air)]);

    // Off across the air time, switched on shortly after, then the app is restarted before
    // the episode could be dealt with.
    let mut notifier = Notifier::load(&pool).await.unwrap();
    notifier.step(&pool, &schedule, &display, false, T + 100).await;
    assert_eq!(db::get_notifier_state(&pool, "silenced_until").await.unwrap(), Some(T + 100));
    notifier.step(&pool, &schedule, &display, false, air + 20).await;
    schedule.fail_with(Some(AppError::Unreachable("offline".to_string())));
    notifier.step(&pool, &schedule, &display, true, air + 50).await;
    assert_eq!(db::get_notifier_state(&pool, "silenced_until").await.unwrap(), Some(air + 20));

    schedule.fail_with(None);
    let mut restarted = Notifier::load(&pool).await.unwrap();
    restarted.step(&pool, &schedule, &display, true, air + 90).await;
    assert!(display.shown().is_empty());
    assert_eq!(recorded(&pool, SHOW_A, 12).await, Some((false, air)));
}

#[tokio::test]
async fn notifier_reads_again_when_a_bell_is_added_and_stops_when_it_is_removed() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();
    schedule.set(vec![airing(SHOW_A, 12, T + 600), airing(SHOW_B, 3, T + 900)]);

    // No bell at all: nothing to read, nothing to watch.
    let mut notifier = Notifier::load(&pool).await.unwrap();
    let outcome = notifier.step(&pool, &schedule, &display, true, T).await;
    assert!(!outcome.watching);
    assert_eq!(schedule.read_count(), 0);

    set_bell(&pool, SHOW_A, true, T + 1).await;
    let outcome = notifier.step(&pool, &schedule, &display, true, T + 1).await;
    assert_eq!(schedule.last_read().0, vec![SHOW_A]);
    assert_eq!(outcome.wake_at, T + 600);

    // A second bell right after: read again, but not before the minimum gap.
    set_bell(&pool, SHOW_B, true, T + 2).await;
    let outcome = notifier.step(&pool, &schedule, &display, true, T + 2).await;
    assert_eq!(schedule.read_count(), 1);
    assert_eq!(outcome.wake_at, T + 1 + MIN_FETCH_GAP_S);
    let outcome = notifier.step(&pool, &schedule, &display, true, T + 1 + MIN_FETCH_GAP_S).await;
    assert_eq!(schedule.read_count(), 2);
    assert_eq!(schedule.last_read().0, vec![SHOW_A, SHOW_B]);
    assert_eq!(outcome.wake_at, T + 600);

    // Bell A is switched off before its episode airs: only B is announced.
    set_bell(&pool, SHOW_A, false, T + 300).await;
    notifier.step(&pool, &schedule, &display, true, T + 600).await;
    notifier.step(&pool, &schedule, &display, true, T + 900).await;
    assert_eq!(display.shown(), vec![out("Show 202", "Episode 3 is out")]);
    assert_eq!(recorded(&pool, SHOW_A, 12).await, None);

    // Switched on again later: the episode that aired in between stays silent.
    set_bell(&pool, SHOW_A, true, T + 1000).await;
    notifier.step(&pool, &schedule, &display, true, T + 1000).await;
    assert_eq!(display.shown().len(), 1);
    assert_eq!(recorded(&pool, SHOW_A, 12).await, Some((false, T + 600)));
}

#[tokio::test]
async fn notifier_handles_a_suspend_past_the_air_time() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();

    set_bell(&pool, SHOW_A, true, T - 100).await;
    set_bell(&pool, SHOW_B, true, T - 100).await;
    schedule.set(vec![airing(SHOW_A, 12, T + HOUR), airing(SHOW_B, 3, T + 2 * HOUR)]);

    let mut notifier = Notifier::load(&pool).await.unwrap();
    notifier.step(&pool, &schedule, &display, true, T).await;

    // The machine sleeps through both air times and wakes three hours later.
    notifier.step(&pool, &schedule, &display, true, T + 3 * HOUR).await;
    assert_eq!(schedule.read_count(), 2);
    assert_eq!(
        display.shown(),
        vec![out("Show 101", "Episode 12 is out"), out("Show 202", "Episode 3 is out")]
    );

    notifier.step(&pool, &schedule, &display, true, T + 3 * HOUR + 30).await;
    assert_eq!(display.shown().len(), 2);

    // A second, much longer sleep: the episode that aired during it is too old to announce.
    schedule.set(vec![airing(SHOW_A, 13, T + 4 * HOUR)]);
    notifier.step(&pool, &schedule, &display, true, T + 3 * HOUR + REFRESH_INTERVAL_S).await;
    notifier.step(&pool, &schedule, &display, true, T + 4 * HOUR + CATCH_UP_WINDOW_S + 60).await;
    assert_eq!(display.shown().len(), 2);
    assert_eq!(recorded(&pool, SHOW_A, 13).await, Some((false, T + 4 * HOUR)));
}

#[tokio::test]
async fn notifier_announces_a_double_episode_as_one_notification() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();

    set_bell(&pool, SHOW_A, true, T - 100).await;
    schedule.set(vec![airing(SHOW_A, 1, T + 600), airing(SHOW_A, 2, T + 600)]);

    let mut notifier = Notifier::load(&pool).await.unwrap();
    notifier.step(&pool, &schedule, &display, true, T).await;
    notifier.step(&pool, &schedule, &display, true, T + 600).await;
    assert_eq!(display.shown(), vec![out("Show 101", "Episodes 1 and 2 are out")]);
    assert_eq!(recorded(&pool, SHOW_A, 1).await, Some((true, T + 600)));
    assert_eq!(recorded(&pool, SHOW_A, 2).await, Some((true, T + 600)));
}

#[tokio::test]
async fn notifier_keeps_quiet_about_a_finished_show() {
    let pool = memory_db().await;
    let schedule = FakeSchedule::default();
    let display = FakeDisplay::default();

    set_bell(&pool, SHOW_A, true, T - 100).await;

    let mut notifier = Notifier::load(&pool).await.unwrap();
    let outcome = notifier.step(&pool, &schedule, &display, true, T).await;
    assert_eq!(outcome.wake_at, T + REFRESH_INTERVAL_S);
    let outcome = notifier.step(&pool, &schedule, &display, true, T + REFRESH_INTERVAL_S).await;
    assert_eq!(outcome.wake_at, T + 2 * REFRESH_INTERVAL_S);
    assert_eq!(schedule.read_count(), 2);
    assert!(display.shown().is_empty());
    assert!(db::list_notified_episodes(&pool).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------------------
// The AniList request
// ---------------------------------------------------------------------------------------

/// Serves `responses` in order to successive requests and returns the URL to post to and the
/// JSON bodies that were received.
fn graphql_server(responses: Vec<(u16, Option<&'static str>, Value)>) -> (String, Arc<Mutex<Vec<Value>>>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::new()));
    let seen = received.clone();

    std::thread::spawn(move || {
        for (status, retry_after, body) in responses {
            let Ok(mut request) = server.recv() else {
                return;
            };
            let mut text = String::new();
            let _ = request.as_reader().read_to_string(&mut text);
            seen.lock().unwrap().push(serde_json::from_str(&text).unwrap_or(Value::Null));

            let mut response = Response::from_string(body.to_string())
                .with_status_code(StatusCode(status))
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            if let Some(seconds) = retry_after {
                response = response.with_header(Header::from_bytes("Retry-After", seconds).unwrap());
            }
            let _ = request.respond(response);
        }
    });

    (format!("http://127.0.0.1:{}/", port), received)
}

fn page(entries: &[(i64, i32, i64)], has_next_page: bool) -> Value {
    let schedules: Vec<Value> = entries
        .iter()
        .map(|(media_id, episode, airing_at)| json!({ "mediaId": media_id, "episode": episode, "airingAt": airing_at }))
        .collect();
    json!({ "data": { "Page": { "pageInfo": { "hasNextPage": has_next_page }, "airingSchedules": schedules } } })
}

#[tokio::test(flavor = "multi_thread")]
async fn anilist_schedule_pages_and_splits_long_id_lists() {
    let (url, received) = graphql_server(vec![
        (200, None, page(&[(1, 12, T + 10)], true)),
        (200, None, page(&[(2, 3, T + 20)], false)),
        (200, None, page(&[(51, 7, T + 30)], false)),
    ]);
    let source = AniListSchedule::with_url(reqwest::Client::new(), url);

    let ids: Vec<i64> = (1..=51).collect();
    let airings = source.fetch(&ids, T - 100, T + 100).await.unwrap();
    assert_eq!(airings, vec![airing(1, 12, T + 10), airing(2, 3, T + 20), airing(51, 7, T + 30)]);

    let received = received.lock().unwrap();
    assert_eq!(received.len(), 3);
    let first_fifty: Vec<i64> = (1..=50).collect();
    assert_eq!(received[0]["variables"], json!({ "ids": first_fifty, "from": T - 100, "to": T + 100, "page": 1 }));
    assert_eq!(received[1]["variables"]["page"], json!(2));
    assert_eq!(received[1]["variables"]["ids"], json!(first_fifty));
    assert_eq!(received[2]["variables"], json!({ "ids": [51], "from": T - 100, "to": T + 100, "page": 1 }));
    let query = received[0]["query"].as_str().unwrap();
    assert!(query.contains("airingSchedules(mediaId_in: $ids, airingAt_greater: $from, airingAt_lesser: $to, sort: TIME)"));
}

#[tokio::test(flavor = "multi_thread")]
async fn anilist_schedule_reports_rate_limits_and_failures() {
    let (url, _) = graphql_server(vec![(429, Some("77"), json!({ "errors": [{ "message": "Too Many Requests.", "status": 429 }] }))]);
    let source = AniListSchedule::with_url(reqwest::Client::new(), url);
    assert!(matches!(
        source.fetch(&[1], T - 100, T + 100).await,
        Err(AppError::RateLimited { retry_after_s: 77 })
    ));

    let (url, _) = graphql_server(vec![(500, None, json!({ "errors": [{ "message": "Internal Server Error" }] }))]);
    let source = AniListSchedule::with_url(reqwest::Client::new(), url);
    assert!(matches!(source.fetch(&[1], T - 100, T + 100).await, Err(AppError::Network(_))));

    // Nothing listens on this port.
    let source = AniListSchedule::with_url(reqwest::Client::new(), "http://127.0.0.1:9/");
    assert!(source.fetch(&[1], T - 100, T + 100).await.is_err());
}
