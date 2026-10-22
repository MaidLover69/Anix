use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HistoryEntry {
    pub anilist_id: i32,
    pub title: String,
    pub cover: Option<String>,
    pub episode_number: f64,
    pub episode_title: Option<String>,
    pub provider: String,
    pub category: String, // "sub" or "dub"
    pub position_ms: i64,
    pub duration_ms: i64,
    pub updated_at: i64,
    pub from_remote: bool,
}

impl HistoryEntry {
    pub fn new(
        anilist_id: i32,
        title: String,
        cover: Option<String>,
        episode_number: f64,
        episode_title: Option<String>,
        provider: String,
        category: String,
        position_ms: i64,
        duration_ms: i64,
        updated_at: i64,
        from_remote: bool,
    ) -> Self {
        Self {
            anilist_id,
            title,
            cover,
            episode_number,
            episode_title,
            provider,
            category,
            position_ms,
            duration_ms,
            updated_at,
            from_remote,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WatchlistEntry {
    pub anilist_id: i32,
    pub title: String,
    pub cover: Option<String>,
    pub format: Option<String>,
    pub average_score: Option<i32>,
    pub added_at: i64,
    /// CURRENT, PLANNING, COMPLETED, PAUSED or DROPPED.
    #[serde(default)]
    pub status: String,
    /// False while a status chosen here waits to be sent to the connected lists.
    #[serde(default)]
    pub synced: bool,
}

impl WatchlistEntry {
    pub fn new(
        anilist_id: i32,
        title: String,
        cover: Option<String>,
        format: Option<String>,
        average_score: Option<i32>,
        added_at: i64,
    ) -> Self {
        Self {
            anilist_id,
            title,
            cover,
            format,
            average_score,
            added_at,
            status: "PLANNING".to_string(),
            synced: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MediaListEntry {
    pub id: i32,
    pub progress: i32,
    pub score: f64,
    pub status: Option<String>,
    pub title: Option<String>,
    pub cover: Option<String>,
    pub format_str: Option<String>,
    pub total_episodes: Option<i32>,
    pub average_score: Option<i32>,
}

impl MediaListEntry {
    pub fn new(
        id: i32,
        progress: i32,
        score: f64,
        status: Option<String>,
        title: Option<String>,
        cover: Option<String>,
        format_str: Option<String>,
        total_episodes: Option<i32>,
        average_score: Option<i32>,
    ) -> Self {
        Self {
            id,
            progress,
            score,
            status,
            title,
            cover,
            format_str,
            total_episodes,
            average_score,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Viewer {
    pub id: i32,
    pub name: String,
    pub avatar_url: Option<String>,
    pub anime_count: i32,
    pub episodes_watched: i32,
    pub minutes_watched: i64,
    pub mean_score: f64,
}

impl Viewer {
    pub fn new(
        id: i32,
        name: String,
        avatar_url: Option<String>,
        anime_count: i32,
        episodes_watched: i32,
        minutes_watched: i64,
        mean_score: f64,
    ) -> Self {
        Self {
            id,
            name,
            avatar_url,
            anime_count,
            episodes_watched,
            minutes_watched,
            mean_score,
        }
    }
}
