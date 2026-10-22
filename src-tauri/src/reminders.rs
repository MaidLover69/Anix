//! "Remind me before it airs": a lead time per bell (Off, 10, 30 or 60 minutes) and the plan
//! of which reminders to show now.
//!
//! [`due`] and [`next_due_at`] are pure; the notifier loop feeds them the bells' lead times,
//! the airing times it already reads and the reminders sent so far, shows what [`due`]
//! returns and records each one with [`record_sent`], so a reminder goes out once.

use crate::error::AppError;
use crate::notifier::Airing;
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::collections::{HashMap, HashSet};

/// The lead times the bell menu offers, in minutes; 0 is Off.
pub const LEAD_CHOICES: [i64; 4] = [0, 10, 30, 60];

/// A reminder to show: `episode` of `media_id` airs at `airing_at`, `lead_minutes` after the
/// reminder is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub media_id: i64,
    pub episode: i32,
    pub airing_at: i64,
    pub lead_minutes: i64,
}

impl Reminder {
    /// Unix seconds at which the reminder is due.
    pub fn remind_at(&self) -> i64 {
        self.airing_at - self.lead_minutes * 60
    }

    /// "Episode 5 airs in 10 minutes"; `now` gives the minutes left when the reminder is late.
    pub fn body(&self, now: i64) -> String {
        self.body_in("en", now)
    }

    /// `body` in an interface language (`crate::i18n`).
    pub fn body_in(&self, lang: &str, now: i64) -> String {
        let minutes = ((self.airing_at - now) as f64 / 60.0).ceil().max(1.0) as i64;
        let key = if minutes == 1 { "reminder_one" } else { "reminder_other" };
        crate::i18n::fill(lang, key, &[("episode", &self.episode.to_string()), ("minutes", &minutes.to_string())])
    }
}

fn candidates<'a>(
    leads: &'a HashMap<i64, i64>,
    airings: &'a [Airing],
    sent: &'a HashSet<(i64, i32)>,
    now: i64,
) -> impl Iterator<Item = Reminder> + 'a {
    airings.iter().filter_map(move |a| {
        let lead = *leads.get(&a.media_id).filter(|lead| **lead > 0)?;
        // An episode that has aired gets no reminder; the airing notification covers it.
        if a.airing_at <= now || sent.contains(&(a.media_id, a.episode)) {
            return None;
        }
        Some(Reminder { media_id: a.media_id, episode: a.episode, airing_at: a.airing_at, lead_minutes: lead })
    })
}

/// The reminders to show at `now`: episodes of bells with a lead time that air within that
/// lead time, have not aired yet and have no reminder in `sent`. Ordered by airing time, each
/// episode once.
pub fn due(
    leads: &HashMap<i64, i64>,
    airings: &[Airing],
    sent: &HashSet<(i64, i32)>,
    now: i64,
) -> Vec<Reminder> {
    let mut seen = HashSet::new();
    let mut out: Vec<Reminder> = candidates(leads, airings, sent, now)
        .filter(|r| r.remind_at() <= now)
        .collect();
    out.sort_by_key(|r| (r.airing_at, r.media_id, r.episode));
    out.retain(|r| seen.insert((r.media_id, r.episode)));
    out
}

/// When the next reminder becomes due (after `now`), so the loop can wake for it.
pub fn next_due_at(
    leads: &HashMap<i64, i64>,
    airings: &[Airing],
    sent: &HashSet<(i64, i32)>,
    now: i64,
) -> Option<i64> {
    candidates(leads, airings, sent, now).map(|r| r.remind_at()).filter(|at| *at > now).min()
}

// ---------------------------------------------------------------------------- storage

/// Adds the lead-time column to the bells and the table of sent reminders; called from
/// `db::run_migrations` after the bells table exists. Only adds, so older data stays.
pub async fn create_tables(conn: &mut SqliteConnection) -> Result<(), AppError> {
    let columns = sqlx::query("PRAGMA table_info(notification_preferences)").fetch_all(&mut *conn).await?;
    if !columns.iter().any(|r| r.get::<String, _>("name") == "lead_minutes") {
        sqlx::query("ALTER TABLE notification_preferences ADD COLUMN lead_minutes INTEGER NOT NULL DEFAULT 0")
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sent_reminders (
            media_id      INTEGER NOT NULL,
            episode       INTEGER NOT NULL,
            airing_at     INTEGER NOT NULL,
            sent_at       INTEGER NOT NULL,
            PRIMARY KEY (media_id, episode)
        )
        "#,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Sets the lead time of an existing bell. Only the values in [`LEAD_CHOICES`] are accepted.
pub async fn set_lead_minutes(pool: &SqlitePool, media_id: i64, minutes: i64) -> Result<(), AppError> {
    if !LEAD_CHOICES.contains(&minutes) {
        return Err(AppError::Other(format!("{} minutes is not a reminder time", minutes)));
    }
    let done = sqlx::query("UPDATE notification_preferences SET lead_minutes = ?1 WHERE media_id = ?2")
        .bind(minutes)
        .bind(media_id)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::Other("Turn on the bell for this anime first".to_string()));
    }
    Ok(())
}

/// The lead time of every bell that is on and has one, by anime.
pub async fn lead_minutes(pool: &SqlitePool) -> Result<HashMap<i64, i64>, AppError> {
    let rows = sqlx::query(
        "SELECT media_id, lead_minutes FROM notification_preferences WHERE enabled = 1 AND lead_minutes > 0",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.get("media_id"), r.get("lead_minutes"))).collect())
}

/// `(media_id, episode)` of every reminder recorded by [`record_sent`].
pub async fn sent(pool: &SqlitePool) -> Result<HashSet<(i64, i32)>, AppError> {
    let rows = sqlx::query("SELECT media_id, episode FROM sent_reminders").fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| (r.get("media_id"), r.get("episode"))).collect())
}

/// Records that a reminder went out. Returns false when it already had, so it is not shown twice.
pub async fn record_sent(pool: &SqlitePool, reminder: &Reminder, now: i64) -> Result<bool, AppError> {
    let done = sqlx::query(
        "INSERT OR IGNORE INTO sent_reminders (media_id, episode, airing_at, sent_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(reminder.media_id)
    .bind(reminder.episode)
    .bind(reminder.airing_at)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}
