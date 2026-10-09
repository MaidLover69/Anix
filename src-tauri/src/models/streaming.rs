use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Category {
    #[serde(rename = "sub")]
    Sub,
    #[serde(rename = "dub")]
    Dub,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Sub => "sub",
            Category::Dub => "dub",
        }
    }
}

impl From<&str> for Category {
    fn from(s: &str) -> Self {
        if s.eq_ignore_ascii_case("dub") {
            Category::Dub
        } else {
            Category::Sub
        }
    }
}

impl From<String> for Category {
    fn from(s: String) -> Self {
        Category::from(s.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EpisodeItem {
    pub pipe_id: String,
    pub number: f64,
    pub title: Option<String>,
    pub image: Option<String>,
    pub synopsis: Option<String>,
    pub filler: bool,
}

impl EpisodeItem {
    pub fn new(
        pipe_id: String,
        number: f64,
        title: Option<String>,
        image: Option<String>,
        filler: bool,
    ) -> Self {
        Self {
            pipe_id,
            number,
            title,
            image,
            synopsis: None,
            filler,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProviderData {
    pub name: String,
    pub sub: Vec<EpisodeItem>,
    pub dub: Vec<EpisodeItem>,
}

impl ProviderData {
    pub fn empty(name: &str) -> Self {
        Self {
            name: name.to_string(),
            sub: Vec::new(),
            dub: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sub.is_empty() && self.dub.is_empty()
    }
}

/// What a provider is asked to find: the AniList/MAL ids plus the facts used to recognise the
/// right search hit on sites that are searched by title.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimeQuery {
    pub anilist_id: i64,
    pub mal_id: Option<i64>,
    /// Known titles, most preferred first (romaji, then English, then synonyms).
    pub titles: Vec<String>,
    /// The English title, when AniList has one; it is also among `titles`.
    pub english: Option<String>,
    pub year: Option<i32>,
    pub episodes: Option<i32>,
    pub format: Option<String>,
}

impl AnimeQuery {
    pub fn new(anilist_id: i64, mal_id: Option<i64>, title: Option<&str>) -> Self {
        let mut query = Self {
            anilist_id,
            mal_id,
            ..Default::default()
        };
        if let Some(t) = title {
            query.add_title(t);
        }
        query
    }

    pub fn add_title(&mut self, title: &str) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        if !self.titles.iter().any(|t| t.eq_ignore_ascii_case(title)) {
            self.titles.push(title.to_string());
        }
    }

    pub fn primary_title(&self) -> Option<&str> {
        self.titles.first().map(|t| t.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StreamItem {
    pub url: String,
    pub stream_type: String, // "hls", "mp4", "dash"
    pub quality: Option<String>,
    pub audio: Option<String>,
    #[serde(default)]
    pub subtitle_variant: Option<String>,
    pub referer: Option<String>,
    pub origin: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub is_active: bool,
}

impl StreamItem {
    pub fn new(
        url: String,
        stream_type: String,
        quality: Option<String>,
        audio: Option<String>,
        referer: Option<String>,
        is_active: bool,
    ) -> Self {
        Self {
            url,
            stream_type,
            quality,
            audio,
            subtitle_variant: None,
            referer,
            origin: None,
            headers: None,
            is_active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SubtitleItem {
    pub url: String,
    pub language: String,
    pub format: String,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SkipTimes {
    pub intro_start: Option<f64>,
    pub intro_end: Option<f64>,
    pub outro_start: Option<f64>,
    pub outro_end: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SourcesResult {
    pub streams: Vec<StreamItem>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleItem>,
    #[serde(default)]
    pub skip: Option<SkipTimes>,
    /// Name of the provider that served the streams; filled in by the provider manager.
    #[serde(default)]
    pub provider: Option<String>,
}

impl SourcesResult {
    pub fn new(streams: Vec<StreamItem>) -> Self {
        Self {
            streams,
            subtitles: Vec::new(),
            skip: None,
            provider: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProviderEpisodes {
    pub sub: Vec<EpisodeItem>,
    pub dub: Vec<EpisodeItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EpisodesResult {
    pub providers: HashMap<String, ProviderEpisodes>,
}
