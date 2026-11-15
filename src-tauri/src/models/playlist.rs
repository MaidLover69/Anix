use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPlaylist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub item_count: i64,
    pub covers: Vec<String>,
    /// A smart collection's filter as a URL query string; `None` for a normal playlist.
    #[serde(default)]
    pub rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPlaylistItem {
    pub id: String,
    pub playlist_id: String,
    pub anilist_id: i64,
    pub mal_id: Option<i64>,
    pub episode_num: f64,
    pub episode_title: Option<String>,
    pub series_title: String,
    pub series_cover: Option<String>,
    pub category: String,
    pub sort_order: i64,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalPlaylistItem {
    pub url: String,
    pub title: String,
    pub referer: Option<String>,
}
