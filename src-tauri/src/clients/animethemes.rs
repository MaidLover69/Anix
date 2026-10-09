use crate::clients::http::build_http_client;
use crate::error::AppError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AnimeThemeEntry {
    pub theme_type: String, // "OP" or "ED"
    pub sequence: Option<i32>, // 1, 2, 3...
    pub slug: String,       // "OP1", "ED2"
    pub song_title: Option<String>,
    pub artist_name: Option<String>,
    pub episodes: Option<String>,
    pub video_url: Option<String>,
    pub audio_url: Option<String>,
    pub resolution: Option<i32>,
}

pub struct AnimeThemesClient {
    client: Client,
}

impl AnimeThemesClient {
    pub fn new() -> Self {
        Self {
            client: build_http_client(),
        }
    }

    pub async fn fetch_themes(&self, mal_id: i64) -> Result<Vec<AnimeThemeEntry>, AppError> {
        let url = format!(
            "https://api.animethemes.moe/anime?filter[has]=resources&filter[site]=MyAnimeList&filter[external_id]={}&include=animethemes.animethemeentries.videos,animethemes.song.artists,animethemes.animethemeentries.videos.audio",
            mal_id
        );

        let res = self
            .client
            .get(&url)
            .header("User-Agent", "AnililiLinux/1.0")
            .send()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;

        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(vec![]);
        }
        if !res.status().is_success() {
            return Err(AppError::Network(format!(
                "AnimeThemes HTTP {}",
                res.status().as_u16()
            )));
        }

        let json: Value = res
            .json()
            .await
            .map_err(|e| AppError::Parse(e.to_string()))?;

        let mut results = Vec::new();

        let anime_arr = json
            .get("anime")
            .and_then(|a| a.as_array())
            .or_else(|| json.get("data").and_then(|d| d.as_array()));

        if let Some(animes) = anime_arr {
            for anime in animes {
                if let Some(themes) = anime.get("animethemes").and_then(|t| t.as_array()) {
                    for theme in themes {
                        let theme_type = theme.get("type").and_then(|t| t.as_str()).unwrap_or("OP").to_string();
                        let sequence = theme.get("sequence").and_then(|s| s.as_i64()).map(|v| v as i32);
                        let slug = theme.get("slug").and_then(|s| s.as_str()).map(String::from).unwrap_or_else(|| {
                            format!("{}{}", theme_type, sequence.unwrap_or(1))
                        });

                        let song = theme.get("song");
                        let song_title = song.and_then(|s| s.get("title")).and_then(|t| t.as_str()).map(String::from);
                        let artist_name = song
                            .and_then(|s| s.get("artists"))
                            .and_then(|a| a.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|art| art.get("name"))
                            .and_then(|n| n.as_str())
                            .map(String::from);

                        // Extract entries & video
                        if let Some(entries) = theme.get("animethemeentries").and_then(|e| e.as_array()) {
                            for entry in entries {
                                let episodes = entry.get("episodes").and_then(|e| e.as_str()).map(String::from);
                                let mut video_url = None;
                                let mut audio_url = None;
                                let mut resolution = None;

                                if let Some(videos) = entry.get("videos").and_then(|v| v.as_array()) {
                                    if let Some(first_vid) = videos.first() {
                                        video_url = first_vid.get("link").and_then(|l| l.as_str()).map(String::from);
                                        resolution = first_vid.get("resolution").and_then(|r| r.as_i64()).map(|v| v as i32);
                                        audio_url = first_vid.get("audio").and_then(|a| a.get("link")).and_then(|l| l.as_str()).map(String::from);
                                    }
                                }

                                results.push(AnimeThemeEntry {
                                    theme_type: theme_type.clone(),
                                    sequence,
                                    slug: slug.clone(),
                                    song_title: song_title.clone(),
                                    artist_name: artist_name.clone(),
                                    episodes,
                                    video_url,
                                    audio_url,
                                    resolution,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(results)
    }
}
