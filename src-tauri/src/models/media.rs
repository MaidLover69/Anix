use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MediaTitle {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
    pub user_preferred: Option<String>,
}

impl MediaTitle {
    pub fn new(
        romaji: Option<String>,
        english: Option<String>,
        native: Option<String>,
        user_preferred: Option<String>,
    ) -> Self {
        Self {
            romaji,
            english,
            native,
            user_preferred,
        }
    }

    pub fn preferred(&self) -> String {
        if let Some(ref e) = self.english {
            if !e.is_empty() {
                return e.clone();
            }
        }
        if let Some(ref u) = self.user_preferred {
            if !u.is_empty() {
                return u.clone();
            }
        }
        if let Some(ref r) = self.romaji {
            if !r.is_empty() {
                return r.clone();
            }
        }
        self.native.clone().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CoverImage {
    pub large: Option<String>,
    pub extra_large: Option<String>,
    pub color: Option<String>,
}

impl CoverImage {
    pub fn new(large: Option<String>, extra_large: Option<String>, color: Option<String>) -> Self {
        Self {
            large,
            extra_large,
            color,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NextAiringEpisode {
    pub id: i32,
    pub airing_at: i64,
    pub time_until_airing: i64,
    pub episode: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Trailer {
    pub id: Option<String>,
    pub site: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CharacterName {
    pub full: Option<String>,
    pub native: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CharacterImage {
    pub large: Option<String>,
    pub medium: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Character {
    pub id: i32,
    pub name: Option<CharacterName>,
    pub image: Option<CharacterImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VoiceActor {
    pub id: i32,
    pub name: Option<CharacterName>,
    pub image: Option<CharacterImage>,
    pub language_v2: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CharacterEdge {
    pub role: Option<String>,
    pub node: Option<Character>,
    #[serde(default)]
    pub voice_actors: Vec<VoiceActor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RelationEdge {
    pub relation_type: Option<String>,
    pub node: Option<Media>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Media {
    pub id: i32,
    pub id_mal: Option<i32>,
    pub imdb_id: Option<String>,
    pub title: MediaTitle,
    pub cover_image: CoverImage,
    pub banner_image: Option<String>,
    pub description: Option<String>,
    pub synopsis_mal: Option<String>,
    pub synopsis_imdb: Option<String>,
    pub format: Option<String>,
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub episodes: Option<i32>,
    pub duration: Option<i32>,
    pub status: Option<String>,
    pub average_score: Option<i32>,
    pub score_mal: Option<f64>,
    pub rating_imdb: Option<String>,
    pub mean_score: Option<i32>,
    pub popularity: Option<i32>,
    pub favourites: Option<i32>,
    pub is_adult: bool,
    pub genres: Vec<String>,
    pub next_airing_episode: Option<NextAiringEpisode>,
    pub trailer: Option<Trailer>,
    #[serde(default)]
    pub characters: Vec<CharacterEdge>,
    #[serde(default)]
    pub relations: Vec<RelationEdge>,
}

impl Media {
    pub fn display_title(&self) -> String {
        self.title.preferred()
    }
}
