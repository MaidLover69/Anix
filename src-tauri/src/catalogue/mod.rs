//! The anime catalogue behind Home, Discover, the anime page, the schedule, the local-file
//! matcher and provider title matching.
//!
//! AniList is asked first; when it fails or is paused ([`breaker`]) the next catalogue is
//! asked, in the order MyAnimeList → Kitsu → Shikimori → Comick → Jikan (and IMDb's
//! suggestion index as the last search). Settings › Catalogue source can put one of them
//! first. Every answer is mapped into the app's `Media` and AniList's details JSON, and keeps
//! AniList ids as the app's key ([`ids`]). The callers keep the offline cache as the last
//! resort.

pub mod breaker;
pub mod comick;
pub mod ids;
pub mod imdb;
pub mod jikan;
pub mod kitsu;
pub mod mal;
pub mod model;
pub mod schedule;
pub mod shikimori;

use crate::clients::anilist::{parse_media_hint, AniListClient, MediaHint, ANILIST_GRAPHQL_URL};
use crate::clients::app_settings::{self, CatalogueSource};
use crate::db::current_unix_timestamp;
use crate::error::AppError;
use crate::models::{AiringEntry, Media};
use crate::notifier::{AniListSchedule, Airing, MarkedAirings, ScheduleSource};
use async_trait::async_trait;
use breaker::{Breaker, Pacer};
use ids::{AnimeKey, ExtIds, Site};
pub use model::{Found, SearchQuery};
use once_cell::sync::OnceCell;
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// How long one catalogue gets to answer before the next one is asked.
pub const SOURCE_TIMEOUT: Duration = Duration::from_secs(7);
/// The schedule reads several pages and maps many ids.
pub const SCHEDULE_TIMEOUT: Duration = Duration::from_secs(15);
const MAPPING_TIMEOUT: Duration = Duration::from_secs(5);
/// arm.haglund.dev rejects larger ids, and the whole batch with them.
const ARM_MAX_ID: i64 = 50_000_000;
const ARM_BATCH: usize = 100;
const SCHEDULE_PAGES: i32 = 3;
const ARM: &str = "arm";
const ANIZIP: &str = "api.ani.zip";
/// ani.zip answers one anime per request (One Piece's answer is about 2 MB).
const ANIZIP_LOOKUPS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    AniList,
    Mal,
    Kitsu,
    Shikimori,
    Comick,
    Jikan,
    Imdb,
}

impl Source {
    pub const ALL: [Source; 7] = [
        Source::AniList,
        Source::Mal,
        Source::Kitsu,
        Source::Shikimori,
        Source::Comick,
        Source::Jikan,
        Source::Imdb,
    ];

    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Source::AniList => "AniList",
            Source::Mal => mal::LABEL,
            Source::Kitsu => kitsu::LABEL,
            Source::Shikimori => shikimori::LABEL,
            Source::Comick => comick::LABEL,
            Source::Jikan => jikan::LABEL,
            Source::Imdb => imdb::LABEL,
        }
    }

    /// The least time between two requests to the catalogue.
    fn gap(self) -> Duration {
        Duration::from_millis(match self {
            Source::AniList => 0,
            Source::Mal | Source::Shikimori => 250,
            Source::Kitsu | Source::Imdb => 200,
            Source::Comick => 300,
            // Jikan allows three requests a second.
            Source::Jikan => 400,
        })
    }

    fn preferred(pref: CatalogueSource) -> Source {
        match pref {
            CatalogueSource::Auto | CatalogueSource::AniList => Source::AniList,
            CatalogueSource::MyAnimeList => Source::Mal,
            CatalogueSource::Kitsu => Source::Kitsu,
            CatalogueSource::Shikimori => Source::Shikimori,
            CatalogueSource::Comick => Source::Comick,
        }
    }
}

/// The order catalogues are asked in: the preferred one, then the rest as usual. IMDb is
/// added by the searches that use it.
pub fn order(pref: CatalogueSource) -> Vec<Source> {
    let first = Source::preferred(pref);
    std::iter::once(first)
        .chain(Source::ALL.into_iter().filter(|s| *s != first && *s != Source::Imdb))
        .collect()
}

/// An answer and the catalogue that gave it.
#[derive(Debug, Clone, PartialEq)]
pub struct Served<T> {
    pub value: T,
    pub source: Source,
    /// AniList was asked for this (or paused) and another catalogue answered.
    pub fallback: bool,
}

impl<T> Served<T> {
    /// The catalogue to name in the "AniList is unavailable" notice, if any.
    pub fn notice(&self) -> Option<&'static str> {
        self.fallback.then(|| self.source.label())
    }
}

/// Where each catalogue lives; tests point them at local servers.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub anilist: String,
    pub mal: String,
    pub kitsu: String,
    pub shikimori: String,
    pub comick: String,
    pub jikan: String,
    pub imdb: String,
    pub arm: String,
    pub anizip: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            anilist: ANILIST_GRAPHQL_URL.to_string(),
            mal: crate::clients::mal::MAL_API_URL.to_string(),
            kitsu: "https://kitsu.app/api/edge".to_string(),
            shikimori: "https://shikimori.io/api/graphql".to_string(),
            comick: "https://api.comick.dev/v1.0".to_string(),
            jikan: "https://api.jikan.moe/v4".to_string(),
            imdb: "https://v3.sg.media-imdb.com/suggestion/x".to_string(),
            arm: "https://arm.haglund.dev/api/v2".to_string(),
            anizip: "https://api.ani.zip".to_string(),
        }
    }
}

pub struct Catalogue {
    client: Client,
    urls: Endpoints,
    anilist: AniListClient,
    /// The id-mapping cache; without it every id is looked up again.
    pool: Option<SqlitePool>,
    breaker: Breaker,
    pacer: Pacer,
    timeout: Duration,
    /// Fixed order for tests; otherwise read from the settings on every request.
    preference: Option<CatalogueSource>,
}

static GLOBAL: OnceCell<Arc<Catalogue>> = OnceCell::new();

/// Makes `catalogue` the one used by code without access to the app state.
pub fn install(catalogue: Arc<Catalogue>) {
    let _ = GLOBAL.set(catalogue);
}

pub fn global() -> Option<Arc<Catalogue>> {
    GLOBAL.get().cloned()
}

/// Titles, year and episode count of an anime for provider matching, from whichever
/// catalogue answers.
pub async fn media_hint(id: i64) -> Result<MediaHint, AppError> {
    match global() {
        Some(catalogue) => catalogue.hint(id).await,
        None => crate::clients::anilist::fetch_media_hint(&crate::clients::http::http_client()?, id).await,
    }
}

/// Debug builds only: `ANIX_CATALOGUE_DOWN=anilist,myanimelist` makes those catalogues fail
/// as if they were down, for trying the fallbacks by hand. Code that asks a catalogue
/// itself (the local-file matcher) checks it too.
pub fn simulated_down(source: Source) -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    std::env::var("ANIX_CATALOGUE_DOWN").is_ok_and(|list| {
        list.split(',').any(|name| name.trim().eq_ignore_ascii_case(source.label()))
    })
}

fn collect_ids<'a>(items: &'a mut [Found], out: &mut Vec<&'a mut ExtIds>) {
    for found in items {
        let Found { ids, related, .. } = found;
        out.push(ids);
        collect_ids(related, out);
    }
}

/// Gives every entry its app id (AniList id, else synthetic; 0 when it has neither).
fn assign_ids(items: &mut [Found]) {
    for f in items {
        f.media.id = f
            .ids
            .anilist
            .or_else(|| f.ids.synthetic())
            .and_then(|id| i32::try_from(id).ok())
            .unwrap_or(0);
        f.media.id_mal = f.media.id_mal.or_else(|| f.ids.mal.and_then(|id| i32::try_from(id).ok()));
        if f.media.imdb_id.is_none() {
            f.media.imdb_id = f.ids.imdb.map(ids::imdb_text);
        }
        assign_ids(&mut f.related);
    }
}

/// `request` (one catalogue's own requests) with `limit`; running out counts as that
/// catalogue failing.
async fn timed<T>(source: Source, limit: Duration, request: impl Future<Output = Result<T, AppError>>) -> Result<T, AppError> {
    tokio::time::timeout(limit, request)
        .await
        .unwrap_or_else(|_| Err(AppError::Network(format!("{} timed out", source.label()))))
}

impl Catalogue {
    pub fn new(client: Client, pool: Option<SqlitePool>) -> Self {
        Self::with_endpoints(client, pool, Endpoints::default(), None)
    }

    pub fn with_endpoints(
        client: Client,
        pool: Option<SqlitePool>,
        urls: Endpoints,
        preference: Option<CatalogueSource>,
    ) -> Self {
        Catalogue {
            anilist: AniListClient::with_base_url(client.clone(), &urls.anilist),
            client,
            urls,
            pool,
            breaker: Breaker::default(),
            pacer: Pacer::default(),
            timeout: SOURCE_TIMEOUT,
            preference,
        }
    }

    /// A shorter per-catalogue limit (tests).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn breaker(&self) -> &Breaker {
        &self.breaker
    }

    /// Where AniList's GraphQL API is asked (a local server in tests).
    pub fn anilist_url(&self) -> &str {
        &self.urls.anilist
    }

    pub fn order(&self) -> Vec<Source> {
        order(self.preference.unwrap_or_else(|| app_settings::current().catalogue_source))
    }

    /// Asks `sources` in turn until one answers. A paused catalogue is skipped; a failure
    /// that says the catalogue is down pauses it. When all fail, the error is one that
    /// means "unreachable" if any was (a paused one counts), so the caller serves its saved
    /// copy. Each call times its own catalogue requests with [`timed`], not its id mapping.
    async fn run<T, F, Fut>(&self, sources: &[Source], call: F) -> Result<Served<T>, AppError>
    where
        F: Fn(Source) -> Fut,
        Fut: Future<Output = Result<T, AppError>>,
    {
        let mut anilist_missed = false;
        let mut first_error: Option<AppError> = None;
        let mut offline_error: Option<AppError> = None;
        for &source in sources {
            if self.breaker.is_open(source.label()) {
                anilist_missed |= source == Source::AniList;
                offline_error.get_or_insert_with(|| {
                    AppError::Network(format!("{} is paused after failing a moment ago", source.label()))
                });
                continue;
            }
            self.pacer.wait(source.label(), source.gap()).await;
            let result = if simulated_down(source) {
                Err(AppError::Network(format!("{} HTTP 503: switched off by ANIX_CATALOGUE_DOWN", source.label())))
            } else {
                call(source).await
            };
            self.breaker.record(source.label(), &result);
            match result {
                Ok(value) => {
                    return Ok(Served { value, source, fallback: anilist_missed && source != Source::AniList })
                }
                Err(e) => {
                    tracing::info!("{} could not answer: {}", source.label(), e);
                    anilist_missed |= source == Source::AniList;
                    if breaker::trips(&e) && offline_error.is_none() {
                        offline_error = Some(e.clone());
                    }
                    first_error.get_or_insert(e);
                }
            }
        }
        Err(offline_error.or(first_error).unwrap_or_else(|| {
            AppError::Unreachable("every anime catalogue failed a moment ago; trying again shortly".to_string())
        }))
    }

    // ------------------------------------------------------------------ search and lists

    pub async fn search(&self, q: &SearchQuery) -> Result<Served<Vec<Media>>, AppError> {
        let mut sources = self.order();
        if q.text().is_some() {
            sources.push(Source::Imdb);
        }
        let filtered_out = std::sync::Mutex::new(None);
        let result = self
            .run(&sources, |source| {
                let filtered_out = &filtered_out;
                async move {
                    let result = self.search_on(source, q).await;
                    if result.as_ref().is_err_and(model::is_filtered_out) {
                        filtered_out.lock().unwrap().get_or_insert(source);
                    }
                    result
                }
            })
            .await;
        // A catalogue had results but none fit the filters, and no other could answer: that is
        // still "nothing found", not an error page.
        match (result, filtered_out.into_inner().unwrap()) {
            (Err(_), Some(source)) => {
                let position = |s: Source| sources.iter().position(|x| *x == s);
                let fallback = position(Source::AniList).is_some_and(|anilist| Some(anilist) < position(source));
                Ok(Served { value: Vec::new(), source, fallback })
            }
            (result, _) => result,
        }
    }

    async fn search_on(&self, source: Source, q: &SearchQuery) -> Result<Vec<Media>, AppError> {
        if source == Source::AniList {
            let search = self.anilist.search(q);
            return timed(source, self.timeout, search).await;
        }
        let found = timed(source, self.timeout, async {
            Ok(match source {
                Source::Mal => {
                    mal::search(&self.client, &self.urls.mal, crate::commands::mal::MAL_CLIENT_ID, q).await?
                }
                Source::Kitsu => kitsu::search(&self.client, &self.urls.kitsu, q).await?,
                Source::Shikimori => shikimori::search(&self.client, &self.urls.shikimori, q).await?,
                Source::Comick => comick::search(&self.client, &self.urls.comick, q).await?,
                Source::Jikan => jikan::search(&self.client, &self.urls.jikan, q).await?,
                Source::Imdb => {
                    let found = imdb::search(&self.client, &self.urls.imdb, q.text().unwrap_or("")).await?;
                    model::kept(imdb::LABEL, q, found)?
                }
                Source::AniList => Vec::new(),
            })
        })
        .await?;
        Ok(self.finish(found).await)
    }

    /// Resolves ids and returns the entries that have one, each anime once.
    async fn finish(&self, mut found: Vec<Found>) -> Vec<Media> {
        self.resolve(&mut found).await;
        let mut seen = HashSet::new();
        found
            .into_iter()
            .map(|f| f.media)
            .filter(|m| m.id != 0 && seen.insert(m.id))
            .collect()
    }

    // ------------------------------------------------------------------ details

    /// One anime in AniList's details shape. `id` is an AniList id or a synthetic one.
    pub async fn details(&self, id: i64) -> Result<Served<Value>, AppError> {
        let key = ids::decode(id).ok_or_else(|| AppError::Parse(format!("Anime with ID {} not found", id)))?;
        let mut sources = self.order();
        match key {
            AnimeKey::AniList(_) => {}
            AnimeKey::Other(site, _) => {
                sources.retain(|s| *s != Source::AniList);
                if site == Site::Imdb {
                    sources.push(Source::Imdb);
                }
            }
        }
        let site_ids = tokio::sync::OnceCell::new();
        self.run(&sources, |source| {
            let site_ids = &site_ids;
            async move {
                if source == Source::AniList {
                    let details = timed(source, self.timeout, self.anilist.fetch_details(id)).await?;
                    if let Some(mal) = details["idMal"].as_i64() {
                        self.remember(&[ExtIds { anilist: Some(id), mal: Some(mal), ..Default::default() }]).await;
                    }
                    return Ok(details);
                }
                let known = *site_ids.get_or_init(|| self.site_ids(key)).await;
                let mut found = timed(source, self.timeout, self.details_on(source, &known)).await?;
                // The mapping knows ids the catalogue may not list (Kitsu often has no MAL id).
                found.ids.merge(known);
                self.resolve(std::slice::from_mut(&mut found)).await;
                self.fill_gaps(source, &mut found).await;
                // The page asked for this id; keep it even when a mapping is found later.
                found.media.id = id as i32;
                let mut json = model::anilist_json(&found);
                // The page names it where a section stays empty ("Characters aren't available from …").
                json["servedBy"] = json!(source.label());
                Ok(json)
            }
        })
        .await
    }

    async fn details_on(&self, source: Source, known: &ExtIds) -> Result<Found, AppError> {
        let missing = |what: &str| model::unsupported(source.label(), &format!("find this anime without its {} id", what));
        match source {
            Source::Mal => {
                let mal_id = known.mal.ok_or_else(|| missing("MyAnimeList"))?;
                mal::details(&self.client, &self.urls.mal, crate::commands::mal::MAL_CLIENT_ID, mal_id).await
            }
            Source::Kitsu => kitsu::details(&self.client, &self.urls.kitsu, known.kitsu.ok_or_else(|| missing("Kitsu"))?).await,
            Source::Shikimori => {
                shikimori::details(&self.client, &self.urls.shikimori, known.mal.ok_or_else(|| missing("MyAnimeList"))?).await
            }
            Source::Jikan => jikan::details(&self.client, &self.urls.jikan, known.mal.ok_or_else(|| missing("MyAnimeList"))?).await,
            Source::Imdb => {
                let tt = known.imdb.ok_or_else(|| missing("IMDb"))?;
                let found = imdb::search(&self.client, &self.urls.imdb, &ids::imdb_text(tt)).await?;
                found
                    .into_iter()
                    .find(|f| f.ids.imdb == Some(tt))
                    .ok_or_else(|| AppError::Parse(format!("IMDb: anime {} not found", ids::imdb_text(tt))))
            }
            Source::Comick => Err(model::unsupported(comick::LABEL, "show one anime by id")),
            Source::AniList => Err(model::unsupported("AniList", "answer here")),
        }
    }

    /// Characters (Shikimori), banner and trailer (Kitsu) for a page whose catalogue has none of
    /// them, as MyAnimeList's. Best effort: each is asked once, side by side, and a failure
    /// leaves the gap.
    async fn fill_gaps(&self, source: Source, found: &mut Found) {
        let m = &found.media;
        let shikimori = (m.characters.is_empty() && source != Source::Shikimori).then_some(found.ids.mal).flatten();
        let kitsu = ((m.banner_image.is_none() || m.trailer.is_none()) && source != Source::Kitsu)
            .then_some(found.ids.kitsu)
            .flatten();
        let (from_shikimori, from_kitsu) = tokio::join!(
            async {
                match shikimori {
                    Some(mal) => self.ask_extra(Source::Shikimori, shikimori::details(&self.client, &self.urls.shikimori, mal)).await,
                    None => None,
                }
            },
            async {
                match kitsu {
                    Some(kitsu_id) => self.ask_extra(Source::Kitsu, kitsu::details(&self.client, &self.urls.kitsu, kitsu_id)).await,
                    None => None,
                }
            }
        );
        let media = &mut found.media;
        for extra in [from_shikimori, from_kitsu].into_iter().flatten() {
            if media.characters.is_empty() {
                media.characters = extra.media.characters;
            }
            media.banner_image = media.banner_image.take().or(extra.media.banner_image);
            media.trailer = media.trailer.take().or(extra.media.trailer);
        }
    }

    /// One request to another catalogue for a page's missing parts, unless it is paused.
    async fn ask_extra(&self, source: Source, request: impl Future<Output = Result<Found, AppError>>) -> Option<Found> {
        if self.breaker.is_open(source.label()) || simulated_down(source) {
            return None;
        }
        self.pacer.wait(source.label(), source.gap()).await;
        let result = timed(source, self.timeout.min(MAPPING_TIMEOUT), request).await;
        self.breaker.record(source.label(), &result);
        result.map_err(|e| tracing::info!("{} could not fill in the page: {}", source.label(), e)).ok()
    }

    pub async fn hint(&self, id: i64) -> Result<MediaHint, AppError> {
        let served = self.details(id).await?;
        Ok(parse_media_hint(&served.value))
    }

    // ------------------------------------------------------------------ schedule

    fn schedule_order(&self) -> Vec<Source> {
        let mut sources = self.order();
        sources.retain(|s| matches!(s, Source::AniList | Source::Mal | Source::Shikimori));
        // Shikimori knows real episode numbers; MAL's are counted, unless MAL is preferred.
        if sources.first() != Some(&Source::Mal) {
            sources.sort_by_key(|s| match s {
                Source::AniList => 0,
                Source::Shikimori => 1,
                _ => 2,
            });
        }
        sources
    }

    pub async fn schedule(&self, from: i64, to: i64, hide_adult: bool) -> Result<Served<Vec<AiringEntry>>, AppError> {
        let sources = self.schedule_order();
        let limit = self.timeout.max(SCHEDULE_TIMEOUT);
        let failed = std::sync::Mutex::new(HashSet::new());
        let (sources, failed) = (&sources, &failed);
        self.run(sources, |source| async move {
            if source == Source::AniList {
                return timed(source, limit, self.anilist.fetch_airing_schedule(from, to)).await;
            }
            let mut slots = self.fallback_slots(source, from, to, hide_adult, limit).await.inspect_err(|_| {
                failed.lock().unwrap().insert(source);
            })?;
            // Shikimori lists its most popular ongoing shows and MAL the seasonal lists, so each
            // misses some of the week: the other one fills the gaps. Shikimori's entry wins for
            // an anime both have, since its episode numbers are real.
            for &other in sources.iter().filter(|s| **s != Source::AniList && **s != source) {
                if failed.lock().unwrap().contains(&other) || self.breaker.is_open(other.label()) || simulated_down(other) {
                    continue;
                }
                self.pacer.wait(other.label(), other.gap()).await;
                let more = self.fallback_slots(other, from, to, hide_adult, limit).await;
                self.breaker.record(other.label(), &more);
                match more {
                    Ok(more) if other == Source::Shikimori => slots = schedule::merge(more, slots),
                    Ok(more) => slots = schedule::merge(slots, more),
                    Err(e) => {
                        tracing::info!("{} could not add to the schedule: {}", other.label(), e);
                        failed.lock().unwrap().insert(other);
                    }
                }
            }
            let mut found: Vec<Found> = slots.iter().map(|(f, _)| f.clone()).collect();
            self.resolve(&mut found).await;
            for ((f, _), resolved) in slots.iter_mut().zip(found) {
                *f = resolved;
            }
            Ok(schedule::entries(slots, hide_adult))
        })
        .await
    }

    /// One fallback catalogue's weekly slots in the window, with `limit` for its requests.
    async fn fallback_slots(&self, source: Source, from: i64, to: i64, hide_adult: bool, limit: Duration) -> Result<Vec<schedule::Slots>, AppError> {
        timed(source, limit, async {
            Ok(match source {
                Source::Shikimori => {
                    let mut found = Vec::new();
                    for page in 1..=SCHEDULE_PAGES {
                        if page > 1 {
                            self.pacer.wait(source.label(), source.gap()).await;
                        }
                        let vars = json!({ "status": "ongoing", "order": "popularity", "limit": 50, "page": page, "censored": hide_adult });
                        let list = shikimori::animes(&self.client, &self.urls.shikimori, vars, false).await?;
                        let full = list.len() == 50;
                        found.extend(list.iter().filter_map(shikimori::parse_anime));
                        if !full {
                            break;
                        }
                    }
                    schedule::from_shikimori(found, from, to)
                }
                Source::Mal => {
                    // A show from last season may still air (MAL lists it there only), and a
                    // window across a season change needs the next season's list too.
                    let first = mal::current_season(from.max(0));
                    let mut seasons = vec![mal::previous_season(first), first];
                    let last = mal::current_season(to.max(0));
                    if last != first {
                        seasons.push(last);
                    }
                    let mut slots: Vec<schedule::Slots> = Vec::new();
                    for (i, (year, season)) in seasons.into_iter().enumerate() {
                        if i > 0 {
                            self.pacer.wait(source.label(), source.gap()).await;
                        }
                        let json = mal::season(&self.client, &self.urls.mal, crate::commands::mal::MAL_CLIENT_ID, year, season).await?;
                        for slot in schedule::from_mal(&json, from, to) {
                            if !slots.iter().any(|(f, _)| f.ids.mal == slot.0.ids.mal) {
                                slots.push(slot);
                            }
                        }
                    }
                    slots
                }
                _ => return Err(model::unsupported(source.label(), "give an airing schedule")),
            })
        })
        .await
    }

    /// The airings of `media_ids` in the window, for release notifications: AniList, else
    /// Shikimori's next episode (and the one a week before it, which has just aired).
    pub async fn airings(&self, media_ids: &[i64], from: i64, to: i64) -> Result<Vec<Airing>, AppError> {
        Ok(self.airings_from(&[Source::AniList, Source::Shikimori], media_ids, from, to).await?.value)
    }

    /// `airings`, asking only `sources` and saying which one answered.
    pub async fn airings_from(
        &self,
        sources: &[Source],
        media_ids: &[i64],
        from: i64,
        to: i64,
    ) -> Result<Served<Vec<Airing>>, AppError> {
        let served = self
            .run(sources, |source| async move {
                let limit = self.timeout.max(SCHEDULE_TIMEOUT);
                if source == Source::AniList {
                    let schedule = AniListSchedule::with_url(self.client.clone(), self.urls.anilist.clone());
                    return timed(source, limit, schedule.fetch(media_ids, from, to)).await;
                }
                let by_mal = self.mal_ids_of(media_ids).await;
                if by_mal.is_empty() {
                    return Ok(Vec::new());
                }
                let by_mal = &by_mal;
                timed(source, limit, async move {
                    let mut airings = Vec::new();
                    let mal_ids: Vec<i64> = by_mal.keys().copied().collect();
                    for chunk in mal_ids.chunks(50) {
                        let ids: Vec<String> = chunk.iter().map(i64::to_string).collect();
                        let vars = json!({ "ids": ids.join(","), "limit": 50 });
                        let list = shikimori::animes(&self.client, &self.urls.shikimori, vars, false).await?;
                        for found in list.iter().filter_map(shikimori::parse_anime) {
                            let (Some(next), Some(media_id)) =
                                (&found.media.next_airing_episode, found.ids.mal.and_then(|m| by_mal.get(&m)))
                            else {
                                continue;
                            };
                            let weekly = matches!(found.media.format.as_deref(), Some("TV") | Some("ONA"));
                            let start = if weekly { next.airing_at - schedule::WEEK_S } else { next.airing_at };
                            let episode = if weekly { next.episode - 1 } else { next.episode };
                            for (airing_at, episode) in schedule::weekly(start, episode, found.media.episodes, weekly, from, to) {
                                airings.push(Airing { media_id: *media_id, episode, airing_at });
                            }
                        }
                    }
                    Ok(airings)
                })
                .await
            })
            .await?;
        Ok(served)
    }

    /// MAL id → app id for the given app ids, from synthetic ids, the cache and arm.
    async fn mal_ids_of(&self, media_ids: &[i64]) -> HashMap<i64, i64> {
        let mut out = HashMap::new();
        let mut lookup = Vec::new();
        for &id in media_ids {
            match ids::decode(id) {
                Some(AnimeKey::Other(Site::Mal, mal)) => {
                    out.insert(mal, id);
                }
                Some(AnimeKey::AniList(anilist)) => {
                    let cached = match &self.pool {
                        Some(pool) => ids::cached_sites(pool, anilist).await.ok().and_then(|ids| ids.mal),
                        None => None,
                    };
                    match cached {
                        Some(mal) => {
                            out.insert(mal, id);
                        }
                        None => lookup.push(anilist),
                    }
                }
                _ => {}
            }
        }
        for chunk in lookup.chunks(ARM_BATCH) {
            let body: Vec<Value> = chunk.iter().map(|id| json!({ "anilist": id })).collect();
            let Some(answers) = self.arm_bulk(&body).await else { break };
            let mut learned = Vec::new();
            for (anilist, answer) in chunk.iter().zip(answers) {
                if let Some(mal) = answer.and_then(|ids| ids.mal) {
                    out.insert(mal, *anilist);
                    learned.push(ExtIds { anilist: Some(*anilist), mal: Some(mal), ..Default::default() });
                }
            }
            self.remember(&learned).await;
        }
        out
    }

    // ------------------------------------------------------------------ ids

    async fn remember(&self, list: &[ExtIds]) {
        let Some(pool) = &self.pool else { return };
        let now = current_unix_timestamp();
        for ids in list {
            if let Err(e) = ids::remember(pool, ids, now).await {
                tracing::warn!("could not save an anime id mapping: {}", e);
                return;
            }
        }
        // A newly learned AniList id may belong to an anime the user has data for under a
        // stand-in id.
        if list.iter().any(|ids| ids.anilist.is_some()) {
            if let Err(e) = crate::db::merge_stand_in_ids(pool).await {
                tracing::warn!("could not merge stand-in ids: {}", e);
            }
        }
    }

    /// POSTs a batch to arm.haglund.dev; one answer (or `None` for an unknown id) per entry.
    async fn arm_bulk(&self, body: &[Value]) -> Option<Vec<Option<ExtIds>>> {
        if body.is_empty() || self.breaker.is_open(ARM) {
            return None;
        }
        let request = self.client.post(format!("{}/ids", self.urls.arm)).timeout(MAPPING_TIMEOUT).json(body);
        let result = model::read_json("arm.haglund.dev", request).await;
        self.breaker.record(ARM, &result);
        match result {
            Ok(Value::Array(list)) if list.len() == body.len() => Some(list.iter().map(ids::parse_arm).collect()),
            Ok(_) => None,
            Err(e) => {
                tracing::info!("anime id mapping failed: {}", e);
                None
            }
        }
    }

    fn anizip_request(&self, param: &str, id: i64) -> reqwest::RequestBuilder {
        self.client.get(format!("{}/mappings", self.urls.anizip)).query(&[(param, id)]).timeout(MAPPING_TIMEOUT)
    }

    /// One anime's ids from api.ani.zip by `param` (`anilist_id`, `mal_id`, `kitsu_id`).
    async fn anizip(&self, param: &str, id: i64) -> Option<ExtIds> {
        if self.breaker.is_open(ANIZIP) {
            return None;
        }
        let result = model::read_json(ANIZIP, self.anizip_request(param, id)).await;
        self.breaker.record(ANIZIP, &result);
        result.ok().as_ref().and_then(ids::parse_anizip)
    }

    /// Fills in the AniList ids of `items` (and their related entries) from the cache and
    /// arm.haglund.dev (api.ani.zip for a few when arm fails), then gives each its app id.
    pub async fn resolve(&self, items: &mut [Found]) {
        let now = current_unix_timestamp();
        {
            let mut all: Vec<&mut ExtIds> = Vec::new();
            collect_ids(items, &mut all);

            let known: Vec<ExtIds> = all.iter().filter(|entry| entry.anilist.is_some()).map(|entry| **entry).collect();
            self.remember(&known).await;

            // Cached answers, by the first site each entry has.
            let mut pending: Vec<&mut ExtIds> = Vec::new();
            for entry in all.into_iter().filter(|entry| entry.anilist.is_none()) {
                let hit = match &self.pool {
                    Some(pool) => {
                        let mut hit = None;
                        for site in [Site::Mal, Site::Kitsu, Site::Imdb] {
                            let Some(ext) = entry.get(site) else { continue };
                            if let Ok(map) = ids::cached_anilist(pool, site, &[ext], now).await {
                                if let Some(answer) = map.get(&ext) {
                                    hit = Some(*answer);
                                    break;
                                }
                            }
                        }
                        hit
                    }
                    None => None,
                };
                match hit {
                    Some(answer) => entry.anilist = answer,
                    None => pending.push(entry),
                }
            }

            // The rest from arm.haglund.dev, in batches.
            let mut learned = Vec::new();
            let mut by_site: Vec<(&mut ExtIds, Value)> = Vec::new();
            let mut imdb_only: Vec<&mut ExtIds> = Vec::new();
            for entry in pending {
                let query = match (entry.mal, entry.kitsu) {
                    (Some(mal), _) if mal <= ARM_MAX_ID => Some(json!({ "myanimelist": mal })),
                    (_, Some(kitsu)) if kitsu <= ARM_MAX_ID => Some(json!({ "kitsu": kitsu })),
                    _ => None,
                };
                match query {
                    Some(q) => by_site.push((entry, q)),
                    None if entry.imdb.is_some() => imdb_only.push(entry),
                    None => {}
                }
            }
            let mut unanswered: Vec<&mut ExtIds> = Vec::new();
            while !by_site.is_empty() {
                let rest = by_site.split_off(by_site.len().min(ARM_BATCH));
                let chunk = std::mem::replace(&mut by_site, rest);
                let body: Vec<Value> = chunk.iter().map(|(_, q)| q.clone()).collect();
                let answers = if unanswered.is_empty() { self.arm_bulk(&body).await } else { None };
                let Some(answers) = answers else {
                    unanswered.extend(chunk.into_iter().map(|(entry, _)| entry));
                    continue;
                };
                for ((entry, _), answer) in chunk.into_iter().zip(answers) {
                    if let Some(answer) = answer {
                        entry.merge(answer);
                    }
                    learned.push(*entry);
                }
            }
            // arm is down or paused: api.ani.zip, one anime per request, side by side.
            if !self.breaker.is_open(ANIZIP) {
                let mut lookups = tokio::task::JoinSet::new();
                for (i, entry) in unanswered.iter().enumerate().take(ANIZIP_LOOKUPS) {
                    let (param, id) = match (entry.mal, entry.kitsu) {
                        (Some(mal), _) => ("mal_id", mal),
                        (_, Some(kitsu)) => ("kitsu_id", kitsu),
                        _ => continue,
                    };
                    let request = self.anizip_request(param, id);
                    lookups.spawn(async move { (i, model::read_json(ANIZIP, request).await) });
                }
                while let Some(Ok((i, result))) = lookups.join_next().await {
                    self.breaker.record(ANIZIP, &result);
                    if let Some(answer) = result.ok().as_ref().and_then(ids::parse_anizip) {
                        unanswered[i].merge(answer);
                        learned.push(*unanswered[i]);
                    }
                }
            }
            // IMDb results only come from the last-resort search; a few lookups at most.
            for entry in imdb_only.into_iter().take(5) {
                if self.breaker.is_open(ARM) {
                    break;
                }
                let tt = ids::imdb_text(entry.imdb.unwrap_or_default());
                let request = self.client.get(format!("{}/imdb", self.urls.arm)).query(&[("id", tt)]).timeout(MAPPING_TIMEOUT);
                let result = model::read_json("arm.haglund.dev", request).await;
                self.breaker.record(ARM, &result);
                if let Ok(Value::Array(list)) = result {
                    if let Some(answer) = list.first().and_then(ids::parse_arm) {
                        entry.merge(answer);
                    }
                    learned.push(*entry);
                }
            }
            self.remember(&learned).await;
        }
        assign_ids(items);
    }

    /// The other catalogues' ids of one anime, for asking them about it.
    async fn site_ids(&self, key: AnimeKey) -> ExtIds {
        match key {
            AnimeKey::Other(site, id) => {
                let mut ids = ExtIds::default();
                match site {
                    Site::Mal => ids.mal = Some(id),
                    Site::Kitsu => ids.kitsu = Some(id),
                    Site::Imdb => ids.imdb = Some(id),
                }
                if site != Site::Mal {
                    let mut found = [Found { ids, ..Default::default() }];
                    self.resolve(&mut found).await;
                    ids = found[0].ids;
                }
                ids
            }
            AnimeKey::AniList(anilist) => {
                if let Some(pool) = &self.pool {
                    if let Ok(cached) = ids::cached_sites(pool, anilist).await {
                        if cached.mal.is_some() || cached.kitsu.is_some() {
                            return cached;
                        }
                    }
                }
                let mut found = None;
                if !self.breaker.is_open(ARM) {
                    let request = self
                        .client
                        .get(format!("{}/ids", self.urls.arm))
                        .query(&[("source", "anilist".to_string()), ("id", anilist.to_string())])
                        .timeout(MAPPING_TIMEOUT);
                    let result = model::read_json("arm.haglund.dev", request).await;
                    self.breaker.record(ARM, &result);
                    found = result.ok().as_ref().and_then(ids::parse_arm);
                }
                if found.is_none() {
                    found = self.anizip("anilist_id", anilist).await;
                }
                let mut ids = found.unwrap_or_default();
                ids.anilist = Some(anilist);
                self.remember(&[ids]).await;
                ids
            }
        }
    }
}

/// Release notifications read the schedule through the catalogue.
#[async_trait]
impl ScheduleSource for Catalogue {
    async fn fetch(&self, media_ids: &[i64], from_ts: i64, to_ts: i64) -> Result<Vec<Airing>, AppError> {
        self.airings(media_ids, from_ts, to_ts).await
    }

    /// Times from any catalogue but AniList are estimates. Stand-in ids, which AniList does not
    /// know, are asked of Shikimori only.
    async fn fetch_marked(&self, media_ids: &[i64], from_ts: i64, to_ts: i64) -> Result<MarkedAirings, AppError> {
        let (standins, known): (Vec<i64>, Vec<i64>) = media_ids.iter().partition(|id| **id < 0);
        let mut marked = MarkedAirings::default();
        if !known.is_empty() {
            let served = self.airings_from(&[Source::AniList, Source::Shikimori], &known, from_ts, to_ts).await?;
            if served.source != Source::AniList {
                marked.estimated.extend(&known);
            }
            marked.airings = served.value;
        }
        if !standins.is_empty() {
            match self.airings_from(&[Source::Shikimori], &standins, from_ts, to_ts).await {
                Ok(served) => {
                    marked.estimated.extend(&standins);
                    marked.airings.extend(served.value);
                }
                Err(e) if known.is_empty() => return Err(e),
                Err(e) => tracing::info!("No schedule for anime without an AniList id: {}", e),
            }
        }
        Ok(marked)
    }
}
