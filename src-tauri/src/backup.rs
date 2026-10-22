//! Backup and restore: the watchlist (with statuses), watch history, playlists, settings and
//! search history in one JSON file, gzip-compressed when its name ends in `.gz`.
//!
//! Account logins are left out unless a passphrase is given; they are then sealed with it
//! (PBKDF2-HMAC-SHA256 → AES-256-CBC, then HMAC-SHA256 over the result). A file written by a
//! newer backup format is refused rather than half-read.

use crate::accounts::store::{account_keys, settings_lock};
use crate::commands::settings::{read_settings_file, write_settings_file};
use crate::error::AppError;
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::Sha256;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

pub const FORMAT: &str = "anix-backup";
/// The newest format this build writes and reads.
pub const VERSION: u32 = 1;

/// Restored in this order (playlist items need their playlist).
pub const TABLES: [&str; 4] = ["watchlist", "watch_history", "custom_playlists", "custom_playlist_items"];

const PBKDF2_ROUNDS: u32 = 210_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sealed {
    pub rounds: u32,
    pub salt: String,
    pub iv: String,
    pub data: String,
    pub mac: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub format: String,
    pub version: u32,
    pub app_version: String,
    /// Unix milliseconds.
    pub created_at: i64,
    /// Table name → array of row objects.
    pub tables: Map<String, Value>,
    /// The settings without account logins.
    pub settings: Value,
    pub search_history: Value,
    /// The account logins, sealed with the passphrase; absent without one.
    #[serde(default)]
    pub logins: Option<Sealed>,
}

/// What a backup holds or what a restore wrote.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub counts: BTreeMap<String, usize>,
    /// The file has sealed logins.
    pub has_logins: bool,
    /// The logins were restored (the passphrase matched).
    pub logins_restored: bool,
    pub search_history: Value,
}

// ---- Writing ------------------------------------------------------------------------------

async fn columns(pool: &SqlitePool, table: &str) -> Result<Vec<String>, AppError> {
    Ok(sqlx::query(&format!("PRAGMA table_info({})", table))
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect())
}

fn quoted(column: &str) -> String {
    format!("\"{}\"", column.replace('"', "\"\""))
}

/// Every row of `table` as a JSON array, built by SQLite.
async fn dump_table(pool: &SqlitePool, table: &str) -> Result<Value, AppError> {
    let pairs: Vec<String> = columns(pool, table)
        .await?
        .iter()
        .map(|c| format!("'{}', {}", c.replace('\'', "''"), quoted(c)))
        .collect();
    let sql = format!(
        "SELECT COALESCE(json_group_array(json_object({})), '[]') FROM {}",
        pairs.join(", "),
        table
    );
    let text: String = sqlx::query_scalar(&sql).fetch_one(pool).await?;
    serde_json::from_str(&text).map_err(|e| AppError::Parse(format!("{} rows: {}", table, e)))
}

/// Builds a backup of the database and the settings file at `settings_path`.
pub async fn create(
    pool: &SqlitePool,
    settings_path: &Path,
    search_history: Value,
    passphrase: Option<&str>,
) -> Result<Backup, AppError> {
    let mut tables = Map::new();
    for table in TABLES {
        tables.insert(table.to_string(), dump_table(pool, table).await?);
    }

    let stored = {
        let _guard = settings_lock();
        read_settings_file(settings_path).map_err(AppError::Other)?
    };
    let mut settings = stored.clone();
    let mut logins = Map::new();
    if let Some(object) = settings.as_object_mut() {
        for key in account_keys() {
            if let Some(value) = object.remove(key) {
                logins.insert(key.to_string(), value);
            }
        }
    }
    let logins = match passphrase.map(str::trim).filter(|p| !p.is_empty()) {
        Some(passphrase) if !logins.is_empty() => {
            Some(seal(passphrase, serde_json::to_vec(&Value::Object(logins)).unwrap_or_default().as_slice())?)
        }
        _ => None,
    };

    Ok(Backup {
        format: FORMAT.to_string(),
        version: VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: crate::db::current_unix_millis(),
        tables,
        settings,
        search_history: if search_history.is_array() { search_history } else { Value::Array(vec![]) },
        logins,
    })
}

/// The file contents: JSON, gzip-compressed when `gzip`.
pub fn to_bytes(backup: &Backup, gzip: bool) -> Result<Vec<u8>, AppError> {
    let json = serde_json::to_vec_pretty(backup).map_err(|e| AppError::Other(e.to_string()))?;
    if !gzip {
        return Ok(json);
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&json).and_then(|_| encoder.finish()).map_err(|e| AppError::Other(e.to_string()))
}

// ---- Reading ------------------------------------------------------------------------------

/// The largest (decompressed) backup this reads.
const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Reads a backup file's contents (plain or gzip JSON) and checks its format and version.
pub fn from_bytes(bytes: &[u8]) -> Result<Backup, AppError> {
    let mut json = Vec::new();
    if bytes.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_BYTES)
            .read_to_end(&mut json)
            .map_err(|e| AppError::Parse(format!("The backup file is damaged: {}", e)))?;
    } else {
        json = bytes.to_vec();
    }
    let value: Value = serde_json::from_slice(&json)
        .map_err(|_| AppError::Parse("This file is not an Anix backup.".to_string()))?;
    if value["format"].as_str() != Some(FORMAT) {
        return Err(AppError::Parse("This file is not an Anix backup.".to_string()));
    }
    let version = value["version"].as_u64().unwrap_or(0);
    if version > VERSION as u64 {
        return Err(AppError::Other(format!(
            "This backup was made by a newer version of Anix (backup format {}; this version reads format {}). Update Anix to restore it.",
            version, VERSION
        )));
    }
    serde_json::from_value(value).map_err(|e| AppError::Parse(format!("The backup file is damaged: {}", e)))
}

pub fn summary(backup: &Backup) -> Summary {
    Summary {
        counts: TABLES
            .iter()
            .map(|t| (t.to_string(), backup.tables.get(*t).and_then(Value::as_array).map_or(0, Vec::len)))
            .collect(),
        has_logins: backup.logins.is_some(),
        logins_restored: false,
        search_history: backup.search_history.clone(),
    }
}

/// Writes a backup into the database and the settings file. Rows with the same key are
/// updated (watch progress and playlists only by newer rows), other rows are kept. Logins are restored only with the matching passphrase; the
/// current logins are kept otherwise. A wrong passphrase stops the restore before anything is
/// written.
pub async fn restore(
    pool: &SqlitePool,
    settings_path: &Path,
    backup: &Backup,
    passphrase: Option<&str>,
) -> Result<Summary, AppError> {
    let logins = match (&backup.logins, passphrase.map(str::trim).filter(|p| !p.is_empty())) {
        (Some(sealed), Some(passphrase)) => Some(open(passphrase, sealed)?),
        _ => None,
    };
    if !backup.settings.is_object() {
        return Err(AppError::Parse("The backup's settings are damaged.".to_string()));
    }

    let mut tx = pool.begin().await?;
    for table in TABLES {
        let Some(rows) = backup.tables.get(table) else { continue };
        if !rows.is_array() {
            return Err(AppError::Parse(format!("The backup's {} is damaged.", table)));
        }
        // Only columns this database has; a missing value takes the column default.
        let info = sqlx::query(&format!("PRAGMA table_info({})", table)).fetch_all(&mut *tx).await?;
        let cols: Vec<String> = info.iter().map(|r| r.get::<String, _>("name")).collect();
        let keys: Vec<String> =
            info.iter().filter(|r| r.get::<i64, _>("pk") > 0).map(|r| quoted(&r.get::<String, _>("name"))).collect();
        let names: Vec<String> = cols.iter().map(|c| quoted(c)).collect();
        let values: Vec<String> = info
            .iter()
            .map(|r| {
                let c = r.get::<String, _>("name");
                let value = format!("json_extract(value, '$.\"{}\"')", c.replace('\'', "''").replace('"', ""));
                match r.get::<Option<String>, _>("dflt_value") {
                    Some(default) => format!("COALESCE({}, {})", value, default),
                    None => value,
                }
            })
            .collect();
        // An upsert, not INSERT OR REPLACE: REPLACE deletes the old row first, and deleting a
        // playlist cascades to its items. A row with `updated_at` is only overwritten by a newer
        // one, and `completed` never goes back to 0.
        let has_updated_at = cols.iter().any(|c| c == "updated_at");
        let sets: Vec<String> = cols
            .iter()
            .filter(|c| !keys.contains(&quoted(c)))
            .map(|c| match c.as_str() {
                "completed" => format!("{q} = MAX({t}.{q}, excluded.{q})", q = quoted(c), t = table),
                _ if has_updated_at => format!(
                    "{q} = CASE WHEN excluded.updated_at > {t}.updated_at THEN excluded.{q} ELSE {t}.{q} END",
                    q = quoted(c),
                    t = table
                ),
                _ => format!("{q} = excluded.{q}", q = quoted(c)),
            })
            .collect();
        let action = if sets.is_empty() { "NOTHING".to_string() } else { format!("UPDATE SET {}", sets.join(", ")) };
        let sql = format!(
            "INSERT INTO {} ({}) SELECT {} FROM json_each(?1) WHERE true ON CONFLICT({}) DO {}",
            table,
            names.join(", "),
            values.join(", "),
            keys.join(", "),
            action
        );
        sqlx::query(&sql).bind(rows.to_string()).execute(&mut *tx).await?;
    }
    tx.commit().await?;

    {
        let _guard = settings_lock();
        let stored = read_settings_file(settings_path).map_err(AppError::Other)?;
        let mut settings = backup.settings.clone();
        let object = settings.as_object_mut().expect("checked above");
        for key in account_keys() {
            // Logins only ever come from the sealed part, never from plain settings.
            object.remove(key);
            let source = logins.as_ref().unwrap_or(&stored);
            if let Some(value) = source.get(key) {
                object.insert(key.to_string(), value.clone());
            }
        }
        write_settings_file(settings_path, &settings).map_err(AppError::Other)?;
    }

    let mut summary = summary(backup);
    summary.logins_restored = logins.is_some();
    Ok(summary)
}

// ---- Sealing the logins -------------------------------------------------------------------

type HmacSha256 = Hmac<Sha256>;

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// PBKDF2-HMAC-SHA256 with one 32-byte block (RFC 8018).
pub fn pbkdf2(passphrase: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut u = hmac(passphrase, &[salt, &1u32.to_be_bytes()]);
    let mut out = u;
    for _ in 1..rounds {
        u = hmac(passphrase, &[&u]);
        out.iter_mut().zip(u.iter()).for_each(|(o, x)| *o ^= x);
    }
    out
}

/// The encryption and authentication keys for one passphrase and salt.
fn keys(passphrase: &str, salt: &[u8], rounds: u32) -> ([u8; 32], [u8; 32]) {
    let master = pbkdf2(passphrase.as_bytes(), salt, rounds);
    (hmac(&master, &[b"anix-backup-enc"]), hmac(&master, &[b"anix-backup-mac"]))
}

fn random(bytes: usize) -> Result<Vec<u8>, AppError> {
    let mut buffer = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut buffer))
        .map_err(|e| AppError::Other(format!("Could not read random bytes: {}", e)))?;
    Ok(buffer)
}

pub fn seal(passphrase: &str, plain: &[u8]) -> Result<Sealed, AppError> {
    let salt = random(16)?;
    let iv = random(16)?;
    let (enc, auth) = keys(passphrase, &salt, PBKDF2_ROUNDS);
    let data = cbc::Encryptor::<aes::Aes256>::new_from_slices(&enc, &iv)
        .map_err(|e| AppError::Other(e.to_string()))?
        .encrypt_padded_vec_mut::<Pkcs7>(plain);
    let mac = hmac(&auth, &[&salt, &iv, &data]);
    Ok(Sealed {
        rounds: PBKDF2_ROUNDS,
        salt: STANDARD.encode(salt),
        iv: STANDARD.encode(iv),
        data: STANDARD.encode(data),
        mac: STANDARD.encode(mac),
    })
}

/// The logins object inside `sealed`, or an error when the passphrase does not match.
pub fn open(passphrase: &str, sealed: &Sealed) -> Result<Value, AppError> {
    let wrong = || AppError::Other("The passphrase does not match this backup.".to_string());
    let damaged = || AppError::Parse("The backup's logins are damaged.".to_string());
    let decode = |text: &str| STANDARD.decode(text).map_err(|_| damaged());
    let (salt, iv, data, mac) = (decode(&sealed.salt)?, decode(&sealed.iv)?, decode(&sealed.data)?, decode(&sealed.mac)?);
    // A crafted file must not make the restore hang.
    if !(1..=10_000_000).contains(&sealed.rounds) {
        return Err(damaged());
    }
    let (enc, auth) = keys(passphrase, &salt, sealed.rounds);
    let mut check = <HmacSha256 as Mac>::new_from_slice(&auth).expect("HMAC takes any key length");
    check.update(&salt);
    check.update(&iv);
    check.update(&data);
    check.verify_slice(&mac).map_err(|_| wrong())?;
    let plain = cbc::Decryptor::<aes::Aes256>::new_from_slices(&enc, &iv)
        .map_err(|_| damaged())?
        .decrypt_padded_vec_mut::<Pkcs7>(&data)
        .map_err(|_| damaged())?;
    serde_json::from_slice::<Value>(&plain)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(damaged)
}
