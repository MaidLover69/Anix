use serde::{Deserialize, Serialize};

pub const P1080_BYTES: i64 = 576_716_800; // 550 MB
pub const P720_BYTES: i64 = 314_572_800;  // 300 MB
pub const P480_BYTES: i64 = 157_286_400;  // 150 MB
pub const P360_BYTES: i64 = 83_886_080;   // 80 MB
pub const HEADROOM_BYTES: i64 = 1_048_576_000; // 1 GB

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DownloadRecord {
    pub id: String,
    pub anilist_id: i64,
    pub episode_num: f64,
    pub episode_title: Option<String>,
    pub series_title: String,
    pub series_cover: Option<String>,
    pub provider: String,
    pub category: String,
    pub quality: String,
    /// `QUEUED`, `DOWNLOADING`, `COMPLETED` or `FAILED`.
    #[serde(default)]
    pub status: String,
    /// Fraction done, 0.0 to 1.0.
    #[serde(default)]
    pub progress: f64,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub duration_s: Option<f64>,
    pub error_msg: Option<String>,
    /// Milliseconds since the epoch. Written by the backend; a value sent by a caller is ignored.
    #[serde(default)]
    pub created_at: i64,
    /// Milliseconds since the epoch. Written by the backend; a value sent by a caller is ignored.
    #[serde(default)]
    pub updated_at: i64,
    /// ISO 639-1 code of the audio ("ja" for a sub), once the source is known.
    #[serde(default)]
    pub audio_lang: Option<String>,
    /// ISO 639-1 code of the subtitles of a sub, once the source is known.
    #[serde(default)]
    pub sub_lang: Option<String>,
}

impl DownloadRecord {
    pub fn new(
        id: String,
        anilist_id: i64,
        episode_num: f64,
        series_title: String,
        provider: String,
        category: String,
        quality: String,
        episode_title: Option<String>,
        series_cover: Option<String>,
        status: String,
        progress: f64,
        file_path: Option<String>,
        file_size: Option<i64>,
        duration_s: Option<f64>,
        error_msg: Option<String>,
        created_at: i64,
        updated_at: i64,
    ) -> Self {
        Self {
            id,
            anilist_id,
            episode_num,
            episode_title,
            series_title,
            series_cover,
            provider,
            category,
            quality,
            status,
            progress,
            file_path,
            file_size,
            duration_s,
            error_msg,
            created_at,
            updated_at,
            audio_lang: None,
            sub_lang: None,
        }
    }
}
