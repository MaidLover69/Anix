use crate::clients::{aniskip, jikan, konoha};
use crate::db;
use crate::models::{EpisodeItem, ProviderEpisodes, SkipTimes, SourcesResult};
use crate::providers::common::fmt_ep;
use crate::state::AppState;
use crate::web_session;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};
use tauri::State;

/// Largest episode number the catalog will invent placeholders up to.
const MAX_FILLED_EPISODES: i64 = 3000;

/// Map key for an episode number that keeps fractional episodes apart (12 and 12.5 differ).
pub fn episode_key(number: f64) -> i64 {
    (number * 100.0).round() as i64
}

/// How many whole episodes have aired, when that is known for certain: one less than the next
/// episode to air, or none for a show that has not started. `None` means it cannot be told from
/// these facts.
pub fn released_episode_cap(status: Option<&str>, next_airing_episode: Option<i32>) -> Option<i64> {
    match next_airing_episode {
        Some(next) => Some((next as i64 - 1).max(0)),
        None if status.map_or(false, |s| s.eq_ignore_ascii_case("NOT_YET_RELEASED")) => Some(0),
        None => None,
    }
}

/// Provider name → episode lists, serialised as a JSON object whose keys keep provider order.
pub struct ProviderEpisodeMap(pub Vec<(String, ProviderEpisodes)>);

impl Serialize for ProviderEpisodeMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, episodes) in &self.0 {
            map.serialize_entry(name, episodes)?;
        }
        map.end()
    }
}

async fn mal_episodes(state: &AppState, mal_id: Option<i64>) -> Vec<jikan::MalEpisodeInfo> {
    let Some(id) = mal_id.filter(|id| *id > 0) else {
        return Vec::new();
    };
    match jikan::fetch_mal_episodes(&state.client, id as i32).await {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!("MyAnimeList episode list for {} unavailable: {}", id, e.reason());
            Vec::new()
        }
    }
}

async fn konoha_episodes(anilist_id: i64) -> Vec<EpisodeItem> {
    match konoha::fetch_konoha_episodes(anilist_id).await {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!("Konoha episode list for {} unavailable: {}", anilist_id, e.reason());
            Vec::new()
        }
    }
}

fn apply_mal_info(episode: &mut EpisodeItem, info: &jikan::MalEpisodeInfo) {
    if let Some(title) = info.title.as_deref().filter(|t| !t.trim().is_empty()) {
        episode.title = Some(title.to_string());
    }
    episode.filler = info.filler;
    if episode.synopsis.is_none() {
        episode.synopsis = info.synopsis.clone();
    }
}

#[tauri::command]
pub async fn get_anime_catalog_episodes(
    state: State<'_, AppState>,
    anilist_id: i64,
    mal_id: Option<i64>,
    total_episodes: Option<i32>,
    title_romaji: Option<String>,
    status: Option<String>,
    next_airing_episode: Option<i32>,
) -> Result<Vec<EpisodeItem>, String> {
    let mut catalog: BTreeMap<i64, EpisodeItem> = BTreeMap::new();

    // Thumbnails and titles from the Konoha CDN, official titles and filler flags from MAL.
    let (konoha_eps, mal_eps) = tokio::join!(konoha_episodes(anilist_id), mal_episodes(&state, mal_id));
    for ep in konoha_eps {
        catalog.insert(episode_key(ep.number), ep);
    }
    for info in &mal_eps {
        if info.number <= 0.0 {
            continue;
        }
        let entry = catalog.entry(episode_key(info.number)).or_insert_with(|| EpisodeItem {
            pipe_id: format!("{}-{}", anilist_id, fmt_ep(info.number)),
            number: info.number,
            title: None,
            image: None,
            synopsis: None,
            filler: false,
        });
        apply_mal_info(entry, info);
    }

    // IMDb plots apply only when its season demonstrably is this anime (see the IMDb client).
    if let Some(title) = title_romaji.as_deref().filter(|t| !t.trim().is_empty()) {
        let expected = total_episodes
            .filter(|t| *t > 0)
            .or_else(|| Some(mal_eps.len() as i32).filter(|n| *n > 0));
        let synopses = state.imdb_client.fetch_episode_synopses(title, expected).await;
        for (ep_num, plot) in synopses {
            if let Some(entry) = catalog.get_mut(&episode_key(ep_num as f64)) {
                entry.synopsis = Some(plot);
            }
        }
    }

    let known_max = catalog
        .values()
        .map(|ep| ep.number.floor() as i64)
        .max()
        .unwrap_or(0);
    let is_releasing = status
        .as_deref()
        .map_or(false, |s| s.eq_ignore_ascii_case("RELEASING"));

    // Placeholders are added only for episodes known to exist: never past what has aired, and
    // for an airing show with no schedule never past the last episode some source lists.
    let fill_to = match released_episode_cap(status.as_deref(), next_airing_episode) {
        Some(cap) => {
            catalog.retain(|key, _| *key <= cap * 100);
            cap
        }
        None if is_releasing => known_max,
        None => (total_episodes.unwrap_or(0) as i64).max(known_max),
    };
    for num in 1..=fill_to.min(MAX_FILLED_EPISODES) {
        catalog.entry(num * 100).or_insert_with(|| EpisodeItem {
            pipe_id: format!("{}-{}", anilist_id, num),
            number: num as f64,
            title: Some(format!("Episode {}", num)),
            image: None,
            synopsis: None,
            filler: false,
        });
    }

    Ok(catalog.into_values().collect())
}

#[tauri::command]
pub async fn get_all_episodes(
    state: State<'_, AppState>,
    anilist_id: i64,
    mal_id: Option<i64>,
    title_romaji: Option<String>,
) -> Result<ProviderEpisodeMap, String> {
    let (provider_lists, mal_eps, konoha_eps) = tokio::join!(
        state
            .provider_manager
            .get_all_episodes(anilist_id, mal_id, title_romaji.clone()),
        mal_episodes(&state, mal_id),
        konoha_episodes(anilist_id)
    );
    let provider_lists = provider_lists.map_err(|e| e.to_string())?;

    let mal_by_key: HashMap<i64, &jikan::MalEpisodeInfo> = mal_eps
        .iter()
        .filter(|ep| ep.number > 0.0)
        .map(|ep| (episode_key(ep.number), ep))
        .collect();

    let imdb_by_key: HashMap<i64, String> = match title_romaji.as_deref().filter(|t| !t.trim().is_empty()) {
        Some(title) => {
            let expected = Some(mal_eps.len() as i32).filter(|n| *n > 0);
            state
                .imdb_client
                .fetch_episode_synopses(title, expected)
                .await
                .into_iter()
                .map(|(ep_num, plot)| (episode_key(ep_num as f64), plot))
                .collect()
        }
        None => HashMap::new(),
    };

    let enrich = |episodes: &mut Vec<EpisodeItem>| {
        for ep in episodes.iter_mut() {
            let key = episode_key(ep.number);
            if let Some(info) = mal_by_key.get(&key) {
                apply_mal_info(ep, info);
            }
            if let Some(plot) = imdb_by_key.get(&key) {
                ep.synopsis = Some(plot.clone());
            }
        }
    };

    // Providers that are asked by episode number list the episodes the catalog knows about.
    let mut catalog_numbers: BTreeMap<i64, f64> = BTreeMap::new();
    for number in mal_eps.iter().map(|ep| ep.number).chain(konoha_eps.iter().map(|ep| ep.number)) {
        if number > 0.0 {
            catalog_numbers.insert(episode_key(number), number);
        }
    }
    let catalog_list = |anilist_id: i64| -> Vec<EpisodeItem> {
        catalog_numbers
            .values()
            .map(|number| EpisodeItem {
                pipe_id: format!("{}-{}", anilist_id, fmt_ep(*number)),
                number: *number,
                title: Some(format!("Episode {}", fmt_ep(*number))),
                image: None,
                synopsis: None,
                filler: false,
            })
            .collect()
    };

    let mut entries: Vec<(String, ProviderEpisodes)> = Vec::new();
    for pdata in provider_lists {
        let (mut sub, mut dub) = if state.provider_manager.is_number_addressed(&pdata.name) {
            (catalog_list(anilist_id), catalog_list(anilist_id))
        } else if pdata.is_empty() {
            // The anime is not on this provider; leaving it out keeps it from being offered.
            continue;
        } else {
            (pdata.sub, pdata.dub)
        };
        enrich(&mut sub);
        enrich(&mut dub);
        entries.push((pdata.name, ProviderEpisodes { sub, dub }));
    }

    Ok(ProviderEpisodeMap(entries))
}

#[tauri::command]
pub async fn fetch_mal_anime_synopsis(
    state: State<'_, AppState>,
    mal_id: i32,
) -> Result<jikan::MalAnimeDetails, String> {
    jikan::fetch_mal_details(&state.client, mal_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_episode_sources(
    state: State<'_, AppState>,
    anilist_id: i64,
    mal_id: Option<i64>,
    episode_number: f64,
    category: String,
    title_romaji: Option<String>,
    provider: Option<String>,
) -> Result<SourcesResult, String> {
    state
        .provider_manager
        .get_sources_for_episode(
            anilist_id,
            mal_id,
            episode_number,
            &category,
            title_romaji,
            provider.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_skip_times(
    mal_id: i64,
    episode_number: f64,
    duration_s: f64,
) -> Result<SkipTimes, String> {
    aniskip::fetch_skip_times(mal_id, episode_number, duration_s)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_konoha_episodes(anilist_id: i64) -> Result<Vec<EpisodeItem>, String> {
    konoha::fetch_konoha_episodes(anilist_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_filler_episodes(
    state: State<'_, AppState>,
    mal_id: i32,
) -> Result<Vec<i32>, String> {
    if let Ok(Some(cached)) = db::get_cached_filler_episodes(&state.pool, mal_id).await {
        return Ok(cached);
    }

    let episodes = jikan::fetch_filler_episodes(&state.client, mal_id)
        .await
        .map_err(|e| e.to_string())?;

    let _ = db::cache_filler_episodes(&state.pool, mal_id, &episodes).await;
    Ok(episodes)
}

/// A site that refused the in-app browser until the user verifies, and the providers it serves.
#[derive(Debug, Clone, Serialize)]
pub struct SiteVerificationStatus {
    pub site: String,
    pub label: String,
    pub origin: String,
    pub providers: Vec<String>,
}

/// The entries the frontend offers a "Verify" button for: each pending site with the keys of
/// the providers it serves. `providers_of` names the providers of a site key.
pub fn verification_entries(
    pending: Vec<web_session::SiteVerification>,
    providers_of: impl Fn(&str) -> Vec<String>,
) -> Vec<SiteVerificationStatus> {
    pending
        .into_iter()
        .map(|pending| SiteVerificationStatus {
            providers: providers_of(&pending.site),
            site: pending.site,
            label: pending.label,
            origin: pending.origin,
        })
        .collect()
}

/// Sites that currently need the user to verify before their providers can answer.
#[tauri::command]
pub async fn get_site_verification_status(
    state: State<'_, AppState>,
) -> Result<Vec<SiteVerificationStatus>, String> {
    Ok(verification_entries(web_session::verification_status(), |site| {
        // Plugins whose requests met the site's check, plus providers bound to the site.
        let mut names = crate::plugins::net::plugins_for_site(site);
        for name in state.provider_manager.providers_for_site(site) {
            if !names.iter().any(|known| known == name) {
                names.push(name.to_string());
            }
        }
        names
    }))
}

/// Shows the site's own window so the user can pass its check. The frontend hears the outcome
/// as a `site-verified` or `site-verification-cancelled` event.
#[tauri::command]
pub async fn open_site_verification(site: String) -> Result<(), String> {
    web_session::open_verification(&site)
        .await
        .map_err(|e| e.reason())
}
