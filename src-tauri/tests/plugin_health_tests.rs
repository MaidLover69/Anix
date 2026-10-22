//! The background extension check: when a round is due, and the results kept between runs.

use anix_lib::plugins::health::{read_record, round_due, HealthRecord, PluginHealth, IDLE_FOR, ROUND_EVERY};
use anix_lib::plugins::registry::changelog_of;
use serde_json::json;
use std::time::Duration;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[test]
fn a_round_runs_once_a_day_while_idle_and_only_when_switched_on() {
    assert_eq!(ROUND_EVERY.as_millis() as i64, DAY_MS);
    let now = 10 * DAY_MS;
    let idle = Some(IDLE_FOR + Duration::from_secs(1));
    assert!(round_due(true, 0, now, None), "never ran, nothing played yet");
    assert!(round_due(true, now - DAY_MS, now, idle));
    assert!(!round_due(true, now - DAY_MS + 60_000, now, idle), "less than a day ago");
    assert!(!round_due(true, 0, now, Some(Duration::from_secs(30))), "something was just played");
    assert!(!round_due(false, 0, now, None), "switched off");
}

#[test]
fn saved_results_are_read_back_and_a_missing_file_is_empty() {
    let dir = std::env::temp_dir().join(format!("anix-health-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plugin-health.json");
    assert_eq!(read_record(&path), HealthRecord::default());

    let mut record = HealthRecord { last_round_ms: 1234, ..HealthRecord::default() };
    record.results.insert(
        "p".into(),
        PluginHealth { id: "p".into(), status: "ok".into(), latency_ms: 5, results: 2, error: None, checked_at_ms: 99 },
    );
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(read_record(&path), record);

    // Results saved before the timestamp existed still read.
    std::fs::write(&path, r#"{"results":{"q":{"id":"q","status":"empty","latency_ms":1,"results":0,"error":null}}}"#).unwrap();
    assert_eq!(read_record(&path).results["q"].checked_at_ms, 0);
    std::fs::write(&path, "not json").unwrap();
    assert_eq!(read_record(&path), HealthRecord::default());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn release_notes_come_from_the_manifest() {
    assert_eq!(changelog_of(&json!({ "changelog": "  Fixes movies.\n" })).as_deref(), Some("Fixes movies."));
    assert_eq!(changelog_of(&json!({ "changelog": "   " })), None);
    assert_eq!(changelog_of(&json!({ "changelog": 3 })), None);
    assert_eq!(changelog_of(&json!({})), None);
    assert_eq!(changelog_of(&json!({ "changelog": "x".repeat(5000) })).unwrap().len(), 2000);
}
