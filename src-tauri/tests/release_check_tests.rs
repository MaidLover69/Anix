//! Release checks: when tracks are checked and given up, the alert wording, the order and
//! pacing of the extension search, and the table that makes every alert go out once.

use anix_lib::db;
use anix_lib::error::AppError;
use anix_lib::models::{AnimeQuery, Category, EpisodeItem, ProviderData, SourcesResult, StreamItem};
use anix_lib::notifier::{Airing, Bell, Snapshot};
use anix_lib::providers::languages::{LanguagePrefs, Languages};
use anix_lib::providers::AnimeProvider;
use anix_lib::release_check::{
    self, CheckRow, CheckSettings, Decision, Hit, CHECK_INTERVAL_S, FIRST_CHECK_DELAY_S, MAX_CHECKS,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const AIR: i64 = 1_800_000_000;
const HOUR: i64 = 3600;
const LIMIT: i64 = 3 * HOUR;

fn row(track: Category) -> CheckRow {
    CheckRow { media_id: 7, episode: 5, track, airing_at: AIR, checks: 0, last_check_at: 0 }
}

/// Plays one track every ten seconds from the air time: the times it was checked and the
/// times a final note was due. `hit_at_check` ends the search at that check.
fn simulate(limit_s: i64, until: i64, hit_at_check: Option<u32>) -> (Vec<i64>, Vec<(i64, bool)>) {
    let mut row = row(Category::Sub);
    let mut checks = Vec::new();
    let mut finals = Vec::new();
    let mut open = true;
    let mut now = AIR;
    while now <= until && open {
        let plan = release_check::plan(std::slice::from_ref(&row), now, limit_s);
        for due in &plan.check {
            assert_eq!(due, &row);
            checks.push(now - AIR);
            row.checks += 1;
            row.last_check_at = now;
            if Some(row.checks) == hit_at_check {
                open = false;
            }
        }
        for (_, announce) in &plan.give_up {
            finals.push((now - AIR, *announce));
            open = false;
        }
        now += 10;
    }
    (checks, finals)
}

#[test]
fn checks_start_ten_minutes_after_air_then_every_fifteen_and_stop_at_the_limit() {
    let (checks, finals) = simulate(LIMIT, AIR + 10 * HOUR, None);
    let expected: Vec<i64> = (0..12).map(|k| FIRST_CHECK_DELAY_S + k * CHECK_INTERVAL_S).collect();
    assert_eq!(checks, expected);
    assert_eq!(checks[0], 10 * 60);
    assert_eq!(checks[1], 25 * 60);
    assert_eq!(checks.len() as u32, MAX_CHECKS);
    assert!(checks.iter().all(|t| *t <= LIMIT));
    // Exactly one final note, at the limit.
    assert_eq!(finals, vec![(LIMIT, true)]);
}

#[test]
fn a_hit_ends_the_track() {
    let (checks, finals) = simulate(LIMIT, AIR + 10 * HOUR, Some(3));
    assert_eq!(checks, vec![600, 1500, 2400]);
    assert!(finals.is_empty());
}

#[test]
fn a_changed_limit_applies_to_the_next_plan() {
    let (one_hour, finals) = simulate(HOUR, AIR + 10 * HOUR, None);
    assert_eq!(one_hour, vec![600, 1500, 2400, 3300]);
    assert_eq!(finals, vec![(HOUR, true)]);

    // A longer limit spaces the checks out so there are never more than twelve.
    let (six_hours, finals) = simulate(6 * HOUR, AIR + 10 * HOUR, None);
    assert_eq!(six_hours.len() as u32, MAX_CHECKS);
    assert!(six_hours.windows(2).all(|w| w[1] - w[0] == release_check::interval_s(6 * HOUR)));
    assert!(release_check::interval_s(6 * HOUR) > CHECK_INTERVAL_S);
    assert_eq!(finals, vec![(6 * HOUR, true)]);

    // The same row, planned under a new limit, follows it at once.
    let mut half_way = row(Category::Sub);
    half_way.checks = 6;
    half_way.last_check_at = AIR + 5100;
    assert_eq!(release_check::decide(&half_way, AIR + 5400, HOUR), Decision::GiveUp { announce: true });
    assert_eq!(release_check::decide(&half_way, AIR + 5400, LIMIT), Decision::Wait(AIR + 6000));
}

#[test]
fn sub_and_dub_are_planned_separately() {
    let mut sub = row(Category::Sub);
    let dub = row(Category::Dub);
    sub.checks = 1;
    sub.last_check_at = AIR + 600;
    let plan = release_check::plan(&[sub.clone(), dub.clone()], AIR + 700, LIMIT);
    assert_eq!(plan.check, vec![dub]);
    assert_eq!(plan.wake_at, Some(AIR + 1500));
}

#[test]
fn a_late_check_does_not_cause_a_burst_and_a_long_absence_ends_quietly() {
    // The computer slept through four slots: one check now, the next a full interval later.
    let mut late = row(Category::Sub);
    late.checks = 1;
    late.last_check_at = AIR + 600;
    assert_eq!(release_check::decide(&late, AIR + 4000, LIMIT), Decision::Check);
    late.checks = 2;
    late.last_check_at = AIR + 4000;
    assert_eq!(release_check::decide(&late, AIR + 4100, LIMIT), Decision::Wait(AIR + 4900));

    // Never checked before the limit (the computer was off): one check, then the final note.
    let never = row(Category::Sub);
    assert_eq!(release_check::decide(&never, AIR + LIMIT + 60, LIMIT), Decision::Check);
    let mut once = never.clone();
    once.checks = 1;
    once.last_check_at = AIR + LIMIT + 60;
    assert_eq!(release_check::decide(&once, AIR + LIMIT + 120, LIMIT), Decision::GiveUp { announce: true });

    // Back after days: closed without a note.
    assert_eq!(release_check::decide(&never, AIR + 3 * 86_400, LIMIT), Decision::GiveUp { announce: false });
}

fn bell(media_id: i64, enabled_at: i64) -> Bell {
    Bell { media_id, media_title: format!("Show {}", media_id), cover_image: None, enabled_at }
}

#[test]
fn checks_start_for_aired_belled_episodes_only() {
    let snapshot = Snapshot {
        fetched_at: AIR + 700,
        media_ids: [7, -100_052_991, 9].into_iter().collect(),
        airings: vec![
            Airing { media_id: 7, episode: 5, airing_at: AIR },
            Airing { media_id: 7, episode: 6, airing_at: AIR + 7 * 86_400 }, // not aired
            Airing { media_id: -100_052_991, episode: 3, airing_at: AIR - 600 }, // stand-in id
            Airing { media_id: 9, episode: 1, airing_at: AIR - 4 * HOUR },  // past the limit
            Airing { media_id: 8, episode: 1, airing_at: AIR },             // no bell
        ],
        estimated: [-100_052_991].into_iter().collect(),
    };
    let bells = vec![bell(7, AIR - 86_400), bell(-100_052_991, AIR - 86_400), bell(9, AIR - 86_400)];
    let started = release_check::starts(&snapshot, &bells, 0, AIR + 700, LIMIT);
    let keys: Vec<(i64, i32)> = started.iter().map(|a| (a.media_id, a.episode)).collect();
    assert_eq!(keys, vec![(-100_052_991, 3), (7, 5)]);

    // Not for what aired before the bell was switched on or while notifications were off.
    let late_bell = vec![bell(7, AIR + 1)];
    assert!(release_check::starts(&snapshot, &late_bell, 0, AIR + 700, LIMIT).is_empty());
    assert!(release_check::starts(&snapshot, &bells[..1], AIR, AIR + 700, LIMIT).is_empty());
}

#[test]
fn alert_wording_carries_the_language() {
    assert_eq!(
        release_check::found_body(5, &Category::Sub, Some("en")),
        "Episode 5 is out — Japanese audio · English subtitles"
    );
    assert_eq!(release_check::found_body(5, &Category::Dub, Some("hi")), "Episode 5 is out — Hindi audio");
    assert_eq!(release_check::found_body(5, &Category::Dub, None), "Episode 5 is out — dubbed audio");
    let prefs = LanguagePrefs::default();
    assert_eq!(
        release_check::missing_body(5, &Category::Sub, &prefs),
        "Episode 5 has aired, but no source has it yet — English subtitles"
    );
}

#[test]
fn settings_limit_and_main_track() {
    assert_eq!(CheckSettings::from_settings(&json!({})), CheckSettings::default());
    assert_eq!(CheckSettings::default().limit_s, LIMIT);
    let read = CheckSettings::from_settings(&json!({ "release_check_hours": 6, "prefer_dub": true }));
    assert_eq!(read.limit_s, 6 * HOUR);
    assert_eq!(read.main_track, Category::Dub);
    assert_eq!(CheckSettings::from_settings(&json!({ "release_check_hours": 0 })).limit_s, HOUR);
    assert_eq!(CheckSettings::from_settings(&json!({ "release_check_hours": 99 })).limit_s, 12 * HOUR);
    assert_eq!(CheckSettings::from_settings(&json!({ "release_check_hours": "x" })).limit_s, LIMIT);
}

// ---------------------------------------------------------------------------------------
// Asking the extensions
// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct Meter {
    in_flight: AtomicUsize,
    most: AtomicUsize,
    calls: Mutex<Vec<&'static str>>,
}

struct FakeSite {
    name: &'static str,
    languages: Languages,
    dub: bool,
    sub_episodes: Vec<f64>,
    dub_episodes: Vec<f64>,
    meter: Arc<Meter>,
}

#[async_trait]
impl AnimeProvider for FakeSite {
    fn name(&self) -> &'static str {
        self.name
    }

    fn supports_dub(&self) -> bool {
        self.dub
    }

    fn languages(&self) -> Option<Languages> {
        Some(self.languages.clone())
    }

    async fn get_episodes(&self, _query: &AnimeQuery) -> Result<ProviderData, AppError> {
        let now = self.meter.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.meter.most.fetch_max(now, Ordering::SeqCst);
        self.meter.calls.lock().unwrap().push(self.name);
        tokio::time::sleep(Duration::from_millis(20)).await;
        self.meter.in_flight.fetch_sub(1, Ordering::SeqCst);
        let list = |numbers: &[f64]| -> Vec<EpisodeItem> {
            numbers.iter().map(|n| EpisodeItem::new(format!("{}-{}", self.name, n), *n, None, None, false)).collect()
        };
        Ok(ProviderData { name: self.name.to_string(), sub: list(&self.sub_episodes), dub: list(&self.dub_episodes) })
    }

    async fn get_sources(&self, _episode: &EpisodeItem, _category: Category) -> Result<SourcesResult, AppError> {
        Ok(SourcesResult::new(vec![StreamItem::default()]))
    }
}

fn site(name: &'static str, sub: &[&str], dub: &[&str], sub_eps: &[f64], dub_eps: &[f64], meter: &Arc<Meter>) -> Arc<dyn AnimeProvider> {
    Arc::new(FakeSite {
        name,
        languages: Languages { sub: sub.iter().map(|s| s.to_string()).collect(), dub: dub.iter().map(|s| s.to_string()).collect() },
        dub: !dub.is_empty(),
        sub_episodes: sub_eps.to_vec(),
        dub_episodes: dub_eps.to_vec(),
        meter: meter.clone(),
    })
}

fn prefs(sub: &[&str], dub: &[&str], pinned: Option<&str>) -> LanguagePrefs {
    LanguagePrefs {
        sub: sub.iter().map(|s| s.to_string()).collect(),
        dub: dub.iter().map(|s| s.to_string()).collect(),
        strict: true,
        pinned: pinned.map(String::from),
    }
}

#[tokio::test]
async fn one_extension_at_a_time_in_language_order_until_the_first_hit() {
    let meter = Arc::new(Meter::default());
    let providers = vec![
        site("spanish", &["es"], &[], &[1.0, 2.0, 3.0, 4.0, 5.0], &[], &meter), // second language
        site("english-old", &["en"], &[], &[1.0, 2.0, 3.0, 4.0], &[], &meter),  // has not got it yet
        site("english-new", &["en"], &["en"], &[1.0, 2.0, 3.0, 4.0, 5.0], &[1.0], &meter),
        site("english-late", &["en"], &[], &[1.0, 2.0, 3.0, 4.0, 5.0], &[], &meter), // never asked
        site("hindi", &[], &["hi"], &[], &[1.0, 2.0, 3.0, 4.0, 5.0], &meter),
    ];
    let query = AnimeQuery::new(7, None, Some("Show"));
    let mut listings = vec![None; providers.len()];
    let prefs = prefs(&["en", "es"], &["hi", "en"], None);

    let hit = release_check::find_first(&providers, &query, 5, &Category::Sub, &prefs, &mut listings).await;
    assert_eq!(hit, Some(Hit { provider: "english-new".into(), language: Some("en".into()) }));
    assert_eq!(*meter.calls.lock().unwrap(), vec!["english-old", "english-new"]);
    assert_eq!(meter.most.load(Ordering::SeqCst), 1, "never more than one extension in flight");

    // The dub: Hindi first; english-new's list is reused, not fetched again.
    let hit = release_check::find_first(&providers, &query, 5, &Category::Dub, &prefs, &mut listings).await;
    assert_eq!(hit, Some(Hit { provider: "hindi".into(), language: Some("hi".into()) }));
    assert_eq!(*meter.calls.lock().unwrap(), vec!["english-old", "english-new", "hindi"]);

    // Nobody has episode 6: every provider in the user's languages is asked once, in order.
    let mut fresh = vec![None; providers.len()];
    meter.calls.lock().unwrap().clear();
    let miss = release_check::find_first(&providers, &query, 6, &Category::Sub, &prefs, &mut fresh).await;
    assert_eq!(miss, None);
    assert_eq!(*meter.calls.lock().unwrap(), vec!["english-old", "english-new", "english-late", "spanish"]);
    assert_eq!(meter.most.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_provider_picked_in_settings_is_asked_first() {
    let meter = Arc::new(Meter::default());
    let providers = vec![
        site("english", &["en"], &[], &[5.0], &[], &meter),
        site("picked", &["es"], &[], &[5.0], &[], &meter),
    ];
    let order = release_check::probe_order(&providers, &Category::Sub, &prefs(&["en", "es"], &["en"], Some("picked")));
    assert_eq!(order, vec![(1, Some("es".to_string())), (0, Some("en".to_string()))]);
    // A provider without a dub is never asked for one.
    assert!(release_check::probe_order(&providers, &Category::Dub, &prefs(&["en"], &["en"], None)).is_empty());
}

// ---------------------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_track_is_closed_once_and_bells_filter_what_is_pending() {
    let pool = db::init_db_in_memory().await.unwrap();
    let airing = Airing { media_id: -100_052_991, episode: 3, airing_at: AIR };
    for track in &release_check::TRACKS {
        release_check::start(&pool, &airing, track).await.unwrap();
        release_check::start(&pool, &airing, track).await.unwrap();
    }
    // No bell: nothing pending.
    assert!(release_check::pending(&pool).await.unwrap().is_empty());

    let pref = anix_lib::models::NotificationPreference {
        media_id: -100_052_991,
        enabled: true,
        media_title: "Stand-in".into(),
        cover_image: None,
    };
    db::save_notification_preference_at(&pool, &pref, AIR - 100).await.unwrap();
    let rows = release_check::pending(&pool).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].track, Category::Dub);
    assert_eq!(rows[1].track, Category::Sub);

    release_check::note_check(&pool, &rows[1], AIR + 600).await.unwrap();
    let sub = release_check::pending(&pool).await.unwrap().remove(1);
    assert_eq!((sub.checks, sub.last_check_at), (1, AIR + 600));

    let hit = Hit { provider: "x".into(), language: Some("en".into()) };
    assert!(release_check::finish(&pool, &sub, Some(&hit), AIR + 600).await.unwrap());
    // A second process closing the same track gets false, so it never alerts again.
    assert!(!release_check::finish(&pool, &sub, Some(&hit), AIR + 601).await.unwrap());
    assert!(!release_check::finish(&pool, &sub, None, AIR + LIMIT).await.unwrap());
    assert_eq!(release_check::pending(&pool).await.unwrap().len(), 1);
}
