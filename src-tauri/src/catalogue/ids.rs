//! Anime ids across catalogues.
//!
//! AniList ids are the app's key for history, watchlist, trackers, extensions and local files.
//! A fallback catalogue's result gets its AniList id from the mapping cache (`catalogue_ids`),
//! from the mapping services (arm.haglund.dev, then api.ani.zip) or from the ids the catalogue
//! itself lists. When no AniList id is known the anime gets a **synthetic id**:
//!
//! `-(site × 100 000 000 + id on that site)`, with site 1 = MyAnimeList, 2 = Kitsu, 3 = IMDb
//! (the digits of the `tt…` id).
//!
//! AniList ids are positive, so the two never collide, and every synthetic id fits in an
//! `i32` (the smallest is −399 999 999).

use crate::error::AppError;
use serde_json::Value;
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::collections::HashMap;

pub const SITE_SPAN: i64 = 100_000_000;

/// A "no AniList id" answer is asked again after this long.
pub const NEGATIVE_TTL_S: i64 = 7 * 24 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    Mal = 1,
    Kitsu = 2,
    Imdb = 3,
}

impl Site {
    pub fn as_str(self) -> &'static str {
        match self {
            Site::Mal => "mal",
            Site::Kitsu => "kitsu",
            Site::Imdb => "imdb",
        }
    }

    fn from_code(code: i64) -> Option<Site> {
        match code {
            1 => Some(Site::Mal),
            2 => Some(Site::Kitsu),
            3 => Some(Site::Imdb),
            _ => None,
        }
    }
}

/// What an app-level anime id stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimeKey {
    AniList(i64),
    Other(Site, i64),
}

pub fn synthetic_id(site: Site, id: i64) -> Option<i64> {
    (1..SITE_SPAN).contains(&id).then(|| -(site as i64 * SITE_SPAN + id))
}

pub fn decode(id: i64) -> Option<AnimeKey> {
    if id > 0 {
        return Some(AnimeKey::AniList(id));
    }
    let raw = id.checked_neg()?;
    let site = Site::from_code(raw / SITE_SPAN)?;
    let native = raw % SITE_SPAN;
    (native > 0).then_some(AnimeKey::Other(site, native))
}

/// `tt0388629` → 388629.
pub fn imdb_number(id: &str) -> Option<i64> {
    id.strip_prefix("tt")?.parse().ok().filter(|n| *n > 0)
}

pub fn imdb_text(number: i64) -> String {
    format!("tt{:07}", number)
}

/// The ids one anime is known by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtIds {
    pub anilist: Option<i64>,
    pub mal: Option<i64>,
    pub kitsu: Option<i64>,
    pub imdb: Option<i64>,
}

impl ExtIds {
    pub fn get(&self, site: Site) -> Option<i64> {
        match site {
            Site::Mal => self.mal,
            Site::Kitsu => self.kitsu,
            Site::Imdb => self.imdb,
        }
    }

    /// The synthetic id for an anime without an AniList id, from the most widely mapped site.
    pub fn synthetic(&self) -> Option<i64> {
        [Site::Mal, Site::Kitsu, Site::Imdb]
            .into_iter()
            .find_map(|site| self.get(site).and_then(|id| synthetic_id(site, id)))
    }

    pub fn merge(&mut self, other: ExtIds) {
        self.anilist = self.anilist.or(other.anilist);
        self.mal = self.mal.or(other.mal);
        self.kitsu = self.kitsu.or(other.kitsu);
        self.imdb = self.imdb.or(other.imdb);
    }
}

fn positive(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|id| *id > 0)
}

/// One arm.haglund.dev record (`null` for an unknown id).
pub fn parse_arm(value: &Value) -> Option<ExtIds> {
    value.is_object().then(|| ExtIds {
        anilist: positive(&value["anilist"]),
        mal: positive(&value["myanimelist"]),
        kitsu: positive(&value["kitsu"]),
        imdb: value["imdb"].as_str().and_then(imdb_number),
    })
}

/// The `mappings` of an api.ani.zip answer.
pub fn parse_anizip(value: &Value) -> Option<ExtIds> {
    let m = value.get("mappings")?;
    let ids = ExtIds {
        anilist: positive(&m["anilist_id"]),
        mal: positive(&m["mal_id"]),
        kitsu: positive(&m["kitsu_id"]),
        imdb: m["imdb_id"].as_str().and_then(imdb_number),
    };
    (ids != ExtIds::default()).then_some(ids)
}

const SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS catalogue_ids (
        site        TEXT NOT NULL,
        ext_id      INTEGER NOT NULL,
        anilist_id  INTEGER,
        checked_at  INTEGER NOT NULL,
        PRIMARY KEY (site, ext_id)
    );
    CREATE INDEX IF NOT EXISTS idx_catalogue_ids_anilist ON catalogue_ids(anilist_id);

    -- Stand-in ids whose rows were copied to the AniList id learned later
    -- (`db::merge_stand_in_ids`). The stand-in rows are kept; lists leave them out.
    -- merged_at = 0 means the copy has not run yet; each mapping is copied once.
    CREATE TABLE IF NOT EXISTS catalogue_merged_ids (
        stand_in    INTEGER PRIMARY KEY,
        anilist_id  INTEGER NOT NULL,
        merged_at   INTEGER NOT NULL
    );
"#;

/// Creates the mapping cache; called from `db::run_migrations`.
pub async fn create_tables(conn: &mut SqliteConnection) -> Result<(), AppError> {
    sqlx::query(SCHEMA).execute(conn).await?;
    Ok(())
}

/// Saves what is known about one anime: each site id points at the AniList id, or at none
/// when `ids.anilist` is unknown.
pub async fn remember(pool: &SqlitePool, ids: &ExtIds, now: i64) -> Result<(), AppError> {
    for site in [Site::Mal, Site::Kitsu, Site::Imdb] {
        let Some(ext_id) = ids.get(site) else { continue };
        // A known AniList id is never replaced by "none".
        sqlx::query(
            r#"
            INSERT INTO catalogue_ids (site, ext_id, anilist_id, checked_at) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(site, ext_id) DO UPDATE SET
                anilist_id = COALESCE(excluded.anilist_id, catalogue_ids.anilist_id),
                checked_at = excluded.checked_at
            "#,
        )
        .bind(site.as_str())
        .bind(ext_id)
        .bind(ids.anilist)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Cached answers for `ext_ids` of one site: `Some(id)` mapped, `None` known to have no AniList
/// entry (asked within [`NEGATIVE_TTL_S`]). Ids never asked about are missing from the map.
pub async fn cached_anilist(
    pool: &SqlitePool,
    site: Site,
    ext_ids: &[i64],
    now: i64,
) -> Result<HashMap<i64, Option<i64>>, AppError> {
    let mut found = HashMap::new();
    for ext_id in ext_ids {
        let row = sqlx::query("SELECT anilist_id, checked_at FROM catalogue_ids WHERE site = ?1 AND ext_id = ?2")
            .bind(site.as_str())
            .bind(ext_id)
            .fetch_optional(pool)
            .await?;
        if let Some(row) = row {
            let anilist: Option<i64> = row.get("anilist_id");
            let checked_at: i64 = row.get("checked_at");
            if anilist.is_some() || now - checked_at < NEGATIVE_TTL_S {
                found.insert(*ext_id, anilist);
            }
        }
    }
    Ok(found)
}

/// The site ids cached for one AniList id.
pub async fn cached_sites(pool: &SqlitePool, anilist_id: i64) -> Result<ExtIds, AppError> {
    let rows = sqlx::query("SELECT site, ext_id FROM catalogue_ids WHERE anilist_id = ?1")
        .bind(anilist_id)
        .fetch_all(pool)
        .await?;
    let mut ids = ExtIds { anilist: Some(anilist_id), ..Default::default() };
    for row in rows {
        let ext_id: i64 = row.get("ext_id");
        match row.get::<String, _>("site").as_str() {
            "mal" => ids.mal = ids.mal.or(Some(ext_id)),
            "kitsu" => ids.kitsu = ids.kitsu.or(Some(ext_id)),
            "imdb" => ids.imdb = ids.imdb.or(Some(ext_id)),
            _ => {}
        }
    }
    Ok(ids)
}
