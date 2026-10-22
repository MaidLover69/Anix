//! "Remind me before it airs": the pure planner and the lead-time storage.

use anix_lib::db;
use anix_lib::models::NotificationPreference;
use anix_lib::notifier::Airing;
use anix_lib::reminders::{self, Reminder};
use std::collections::{HashMap, HashSet};

const AIRS: i64 = 1_800_000_000;

fn airing(media_id: i64, episode: i32, airing_at: i64) -> Airing {
    Airing { media_id, episode, airing_at }
}

#[test]
fn reminder_is_due_at_airing_minus_lead() {
    let leads = HashMap::from([(1, 10)]);
    let airings = [airing(1, 5, AIRS)];
    let none = HashSet::new();
    assert!(reminders::due(&leads, &airings, &none, AIRS - 601).is_empty(), "one second early");
    let due = reminders::due(&leads, &airings, &none, AIRS - 600);
    assert_eq!(due, vec![Reminder { media_id: 1, episode: 5, airing_at: AIRS, lead_minutes: 10 }]);
    assert_eq!(due[0].remind_at(), AIRS - 600);
    assert_eq!(due[0].body(AIRS - 600), "Episode 5 airs in 10 minutes");
    // Late (the app was closed): still due while the episode has not aired.
    assert_eq!(reminders::due(&leads, &airings, &none, AIRS - 30).len(), 1);
    assert_eq!(reminders::due(&leads, &airings, &none, AIRS - 30)[0].body(AIRS - 30), "Episode 5 airs in 1 minute");
}

#[test]
fn reminder_never_for_aired_episodes_or_bells_without_lead() {
    let airings = [airing(1, 5, AIRS), airing(2, 1, AIRS)];
    let none = HashSet::new();
    let leads = HashMap::from([(1, 60), (2, 0)]);
    assert!(reminders::due(&leads, &airings, &none, AIRS).is_empty(), "aired at this second");
    assert!(reminders::due(&leads, &airings, &none, AIRS + 60).is_empty(), "aired");
    let due = reminders::due(&leads, &airings, &none, AIRS - 60);
    assert_eq!(due.iter().map(|r| r.media_id).collect::<Vec<_>>(), vec![1], "lead 0 is Off; no bell lead is none");
}

#[test]
fn reminder_goes_out_once() {
    let leads = HashMap::from([(1, 30)]);
    // The same episode read twice (two schedule pages) still gives one reminder.
    let airings = [airing(1, 5, AIRS), airing(1, 5, AIRS)];
    assert_eq!(reminders::due(&leads, &airings, &HashSet::new(), AIRS - 100).len(), 1);
    let sent = HashSet::from([(1, 5)]);
    assert!(reminders::due(&leads, &airings, &sent, AIRS - 100).is_empty());
}

#[test]
fn next_due_at_is_the_nearest_future_reminder() {
    let leads = HashMap::from([(1, 10), (2, 60)]);
    let airings = [airing(1, 5, AIRS), airing(2, 3, AIRS + 7200), airing(1, 4, AIRS - 100_000)];
    let none = HashSet::new();
    assert_eq!(reminders::next_due_at(&leads, &airings, &none, AIRS - 10_000), Some(AIRS - 600));
    // Once the first is due it is not "next"; the following one is.
    assert_eq!(reminders::next_due_at(&leads, &airings, &none, AIRS - 600), Some(AIRS + 3600));
    assert_eq!(reminders::next_due_at(&leads, &airings, &none, AIRS + 7200), None);
}

#[tokio::test]
async fn lead_times_are_stored_per_bell_and_reminders_recorded_once() {
    let pool = db::init_db_in_memory().await.expect("in-memory database");
    let bell = NotificationPreference { media_id: 7, enabled: true, media_title: "Show".into(), cover_image: None };
    db::save_notification_preference(&pool, &bell).await.unwrap();

    assert!(reminders::lead_minutes(&pool).await.unwrap().is_empty(), "a new bell has no lead time");
    assert!(reminders::set_lead_minutes(&pool, 7, 15).await.is_err(), "only the offered choices");
    assert!(reminders::set_lead_minutes(&pool, 8, 10).await.is_err(), "no bell, no lead time");
    reminders::set_lead_minutes(&pool, 7, 30).await.unwrap();
    assert_eq!(reminders::lead_minutes(&pool).await.unwrap(), HashMap::from([(7, 30)]));

    // Switching the bell off hides the lead time; switching it on again keeps it.
    db::save_notification_preference(&pool, &NotificationPreference { enabled: false, ..bell.clone() }).await.unwrap();
    assert!(reminders::lead_minutes(&pool).await.unwrap().is_empty());
    db::save_notification_preference(&pool, &bell).await.unwrap();
    assert_eq!(reminders::lead_minutes(&pool).await.unwrap(), HashMap::from([(7, 30)]));

    let reminder = Reminder { media_id: 7, episode: 2, airing_at: AIRS, lead_minutes: 30 };
    assert!(reminders::record_sent(&pool, &reminder, AIRS - 1800).await.unwrap());
    assert!(!reminders::record_sent(&pool, &reminder, AIRS - 1700).await.unwrap(), "only once");
    assert_eq!(reminders::sent(&pool).await.unwrap(), HashSet::from([(7, 2)]));
}

#[tokio::test]
async fn migration_adds_the_column_to_an_existing_database_once() {
    let pool = db::init_db_in_memory().await.expect("in-memory database");
    // Running the migrations again (every start) must not fail on the added column.
    db::run_migrations(&pool).await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    reminders::set_lead_minutes(&pool, 1, 10).await.unwrap_err();
}
