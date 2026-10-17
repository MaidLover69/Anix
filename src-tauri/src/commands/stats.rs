//! The AniList half of the Library statistics: the connected account's whole anime list with
//! the fields the charts need, plus the voice actors of the main characters of the best-rated
//! completed titles. The local half (episodes and hours) comes from `get_watched_history`.

use crate::accounts;
use crate::clients::anilist::post_graphql;
use crate::error::AppError;
use crate::state::AppState;
use accounts::oauth::Service;
use accounts::session;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::{AppHandle, State};

/// How many completed titles are asked for their voice actors, and how many per request.
pub const VOICE_ACTOR_TITLES: usize = 50;
const VOICE_ACTOR_BATCH: usize = 25;
const TOP_VOICE_ACTORS: usize = 10;

const LIST_QUERY: &str = r#"
query ($userId: Int) {
    MediaListCollection(userId: $userId, type: ANIME) {
        lists {
            isCustomList
            entries {
                status
                score(format: POINT_100)
                progress
                repeat
                completedAt { year }
                updatedAt
                media {
                    id
                    title { userPreferred english romaji }
                    coverImage { large }
                    format
                    episodes
                    duration
                    genres
                    studios(isMain: true) { nodes { id name } }
                }
            }
        }
    }
}
"#;

const VOICE_ACTOR_QUERY: &str = r#"
query ($ids: [Int], $perPage: Int) {
    Page(page: 1, perPage: $perPage) {
        media(id_in: $ids, type: ANIME) {
            id
            characters(role: MAIN, perPage: 3) {
                edges {
                    voiceActors(language: JAPANESE) { id name { full } image { medium } }
                }
            }
        }
    }
}
"#;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsEntry {
    pub media_id: i64,
    pub title: String,
    pub cover: Option<String>,
    pub status: String,
    /// 0..100; 0 means not scored.
    pub score: i64,
    pub progress: i64,
    pub repeat: i64,
    pub episodes: Option<i64>,
    /// Minutes per episode.
    pub duration: Option<i64>,
    pub format: Option<String>,
    pub genres: Vec<String>,
    pub studios: Vec<String>,
    pub completed_year: Option<i64>,
    /// Unix seconds.
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoiceActorCount {
    pub id: i64,
    pub name: String,
    pub image: Option<String>,
    /// Distinct titles among the checked ones.
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AniListStats {
    pub entries: Vec<StatsEntry>,
    pub voice_actors: Vec<VoiceActorCount>,
    /// How many completed titles were asked for voice actors.
    pub voice_actor_titles: usize,
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// The entries of a `MediaListCollection` reply. Custom lists repeat entries of the status
/// lists, so they are skipped, and an anime is listed once.
pub fn parse_stats_entries(data: &Value) -> Vec<StatsEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    let lists = data["MediaListCollection"]["lists"].as_array().cloned().unwrap_or_default();
    for list in lists.iter().filter(|list| !list["isCustomList"].as_bool().unwrap_or(false)) {
        for entry in list["entries"].as_array().into_iter().flatten() {
            let media = &entry["media"];
            let Some(media_id) = media["id"].as_i64() else { continue };
            if !seen.insert(media_id) {
                continue;
            }
            let title = text(&media["title"]["english"])
                .or_else(|| text(&media["title"]["userPreferred"]))
                .or_else(|| text(&media["title"]["romaji"]))
                .unwrap_or_else(|| "Untitled".to_string());
            entries.push(StatsEntry {
                media_id,
                title,
                cover: text(&media["coverImage"]["large"]),
                status: text(&entry["status"]).unwrap_or_default(),
                score: entry["score"].as_f64().map(|s| s.round() as i64).unwrap_or(0).clamp(0, 100),
                progress: entry["progress"].as_i64().unwrap_or(0).max(0),
                repeat: entry["repeat"].as_i64().unwrap_or(0).max(0),
                episodes: media["episodes"].as_i64().filter(|n| *n > 0),
                duration: media["duration"].as_i64().filter(|n| *n > 0),
                format: text(&media["format"]),
                genres: media["genres"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(text)
                    .collect(),
                studios: media["studios"]["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|node| text(&node["name"]))
                    .collect(),
                completed_year: entry["completedAt"]["year"].as_i64(),
                updated_at: entry["updatedAt"].as_i64().unwrap_or(0),
            });
        }
    }
    entries
}

/// The completed titles whose voice actors are counted: the best scored first, then the most
/// recently updated, at most `limit`.
pub fn voice_actor_candidates(entries: &[StatsEntry], limit: usize) -> Vec<i64> {
    let mut completed: Vec<&StatsEntry> = entries.iter().filter(|e| e.status == "COMPLETED").collect();
    completed.sort_by(|a, b| b.score.cmp(&a.score).then(b.updated_at.cmp(&a.updated_at)));
    completed.into_iter().take(limit).map(|e| e.media_id).collect()
}

/// Counts each voice actor once per title across the `Page.media` lists of several replies.
/// AniList sends `voiceActors: null` when it withholds them; those simply add nothing.
pub fn tally_voice_actors(pages: &[Value], top: usize) -> Vec<VoiceActorCount> {
    let mut counts: HashMap<i64, VoiceActorCount> = HashMap::new();
    for page in pages {
        for media in page["Page"]["media"].as_array().into_iter().flatten() {
            let mut in_title = std::collections::HashSet::new();
            for edge in media["characters"]["edges"].as_array().into_iter().flatten() {
                for actor in edge["voiceActors"].as_array().into_iter().flatten() {
                    let (Some(id), Some(name)) = (actor["id"].as_i64(), text(&actor["name"]["full"])) else {
                        continue;
                    };
                    if !in_title.insert(id) {
                        continue;
                    }
                    counts
                        .entry(id)
                        .or_insert_with(|| VoiceActorCount {
                            id,
                            name,
                            image: text(&actor["image"]["medium"]),
                            count: 0,
                        })
                        .count += 1;
                }
            }
        }
    }
    let mut list: Vec<VoiceActorCount> = counts.into_values().filter(|va| va.count > 0).collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    list.truncate(top);
    list
}

/// The connected AniList account's list for the statistics. Two to three requests: the list,
/// then the voice actors in batches of 25 titles. A failed voice-actor request only leaves
/// that part empty.
#[tauri::command]
pub async fn fetch_anilist_stats(app: AppHandle, state: State<'_, AppState>) -> Result<AniListStats, String> {
    let (store, notifier) = accounts::parts(&app)?;
    let ctx = accounts::context(&state, &store, &notifier);
    let connected = store.is_connected(Service::AniList);
    let client = state.client.clone();

    let result: Result<AniListStats, AppError> = async {
        let user_id = session::anilist_viewer_id(&ctx).await?;
        let data = session::with_anilist(&ctx, |token| {
            let client = client.clone();
            async move {
                post_graphql(
                    &client,
                    &json!({ "query": LIST_QUERY, "variables": { "userId": user_id } }),
                    Some(&token),
                )
                .await
            }
        })
        .await?;
        let entries = parse_stats_entries(&data);

        let ids = voice_actor_candidates(&entries, VOICE_ACTOR_TITLES);
        let mut pages = Vec::new();
        for batch in ids.chunks(VOICE_ACTOR_BATCH) {
            let body = json!({
                "query": VOICE_ACTOR_QUERY,
                "variables": { "ids": batch, "perPage": batch.len() }
            });
            let page = session::with_anilist(&ctx, |token| {
                let client = client.clone();
                async move { post_graphql(&client, &body, Some(&token)).await }
            })
            .await;
            match page {
                Ok(page) => pages.push(page),
                Err(e) => {
                    tracing::warn!("could not load voice actors for the statistics: {}", e);
                    break;
                }
            }
        }

        Ok(AniListStats {
            voice_actors: tally_voice_actors(&pages, TOP_VOICE_ACTORS),
            voice_actor_titles: ids.len(),
            entries,
        })
    }
    .await;

    result.map_err(|e| session::error_message(Service::AniList, connected, &e))
}
