use crate::cache::L1Cache;
use crate::providers::matching::title_similarity;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:146.0) Gecko/20100101 Firefox/146.0";

static EXPLICIT_SEASON_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:season\s*(\d+)|(\d+)(?:st|nd|rd|th)\s+season)\b").expect("season regex")
});
/// Markers of a sequel, split cour or side release whose IMDb season cannot be told from the
/// title alone.
static AMBIGUOUS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:part\s*\d+|cour\s*\d+|final season|ii|iii|iv|vi|vii|movie|film|gekijouban|ova|oad|ona|specials?|recap)\b|\s[2-9]$",
    )
    .expect("ambiguous marker regex")
});
static EPISODE_CARD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"S\d+\.E(\d+)[^<]*</div>[\s\S]*?<div class="ipc-html-content-inner-div"[^>]*>(.*?)</div>"#)
        .expect("episode card regex")
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ImdbRatingResult {
    pub imdb_id: Option<String>,
    pub rating: Option<String>,
    pub votes: Option<String>,
}

/// One IMDb season: how many episodes it lists and the plots it has, by episode number.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImdbSeason {
    pub episode_count: usize,
    pub synopses: HashMap<i32, String>,
}

/// The series title to look up and the IMDb season an anime title refers to. A plain title is
/// season 1 and "… Season 2" / "… 2nd Season" is season 2 of the base title; a title with any
/// other sequel marker (Part 2, II, Final Season, Movie, OVA, …) has no known season.
pub fn imdb_season_for_title(title: &str) -> Option<(String, u32)> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    if let Some(cap) = EXPLICIT_SEASON_RE.captures(title) {
        let season: u32 = cap.get(1).or_else(|| cap.get(2))?.as_str().parse().ok()?;
        let whole = cap.get(0)?;
        let base = format!("{} {}", &title[..whole.start()], &title[whole.end()..]);
        let base = base
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim_end_matches(|c: char| c == ':' || c == '-' || c.is_whitespace())
            .to_string();
        if season == 0 || base.is_empty() || AMBIGUOUS_RE.is_match(&base) {
            return None;
        }
        return Some((base, season));
    }
    if AMBIGUOUS_RE.is_match(title) {
        return None;
    }
    Some((title.to_string(), 1))
}

/// True when IMDb's season can be the anime in question: both list the same number of episodes
/// (give or take one). An unknown expected count cannot confirm anything.
pub fn season_matches_expected(imdb_episode_count: usize, expected_episodes: Option<i32>) -> bool {
    match expected_episodes {
        Some(expected) if expected > 0 => (imdb_episode_count as i64 - expected as i64).abs() <= 1,
        _ => false,
    }
}

/// Parses an IMDb season page: the embedded `__NEXT_DATA__` JSON when present, otherwise the
/// rendered episode cards.
pub fn parse_season_page(html: &str) -> ImdbSeason {
    let mut season = ImdbSeason::default();

    const MARKER: &str = "<script id=\"__NEXT_DATA__\" type=\"application/json\">";
    if let Some(start_idx) = html.find(MARKER) {
        let json_start = start_idx + MARKER.len();
        if let Some(end_idx) = html[json_start..].find("</script>") {
            let json_str = &html[json_start..json_start + end_idx];
            if let Ok(next_data) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(items) = next_data
                    .pointer("/props/pageProps/contentData/section/episodes/items")
                    .and_then(|v| v.as_array())
                {
                    season.episode_count = items.len();
                    for item in items {
                        let ep_num = item["episode"]
                            .as_str()
                            .and_then(|s| s.parse::<i32>().ok())
                            .or_else(|| item["episode"].as_i64().map(|v| v as i32));
                        let plot = item["plot"].as_str().map(str::trim).filter(|p| !p.is_empty());
                        if let (Some(ep), Some(plot)) = (ep_num, plot) {
                            season.synopses.insert(ep, plot.to_string());
                        }
                    }
                }
            }
        }
    }

    if season.episode_count == 0 {
        let mut seen: Vec<i32> = Vec::new();
        for cap in EPISODE_CARD_RE.captures_iter(html) {
            if let Ok(ep_num) = cap[1].parse::<i32>() {
                if !seen.contains(&ep_num) {
                    seen.push(ep_num);
                }
                let plot = cap[2].trim();
                if !plot.is_empty() {
                    season.synopses.entry(ep_num).or_insert_with(|| plot.to_string());
                }
            }
        }
        season.episode_count = seen.len();
    }

    season
}

pub struct ImdbClient {
    client: Client,
    /// Season pages by "imdb id|season"; a failed fetch is not cached.
    seasons: L1Cache<String, Arc<ImdbSeason>>,
    /// Title lookups, including "nothing found".
    ids: L1Cache<String, Option<String>>,
}

impl ImdbClient {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            seasons: L1Cache::new(200, 6 * 3600),
            ids: L1Cache::new(500, 6 * 3600),
        }
    }

    /// Looks a title up in IMDb's suggestion index and returns the best-matching TV series.
    pub async fn fetch_rating(&self, title: &str) -> Option<ImdbRatingResult> {
        let clean_title: String = title
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let query_url = format!(
            "https://v3.sg.media-imdb.com/suggestion/x/{}.json",
            urlencoding::encode(clean_title.trim_matches('_'))
        );

        let res = self
            .client
            .get(&query_url)
            .header("User-Agent", "Mozilla/5.0 (X11; Linux x86_64)")
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }

        let json: serde_json::Value = res.json().await.ok()?;
        let series: Vec<&serde_json::Value> = json["d"]
            .as_array()?
            .iter()
            .filter(|s| matches!(s["q"].as_str(), Some("TV series") | Some("TV mini-series")))
            .collect();

        // IMDb ranks by popularity; a suggestion whose own title matches beats the first one.
        let best = series
            .iter()
            .find(|s| s["l"].as_str().map_or(false, |l| title_similarity(l, title) >= 0.8))
            .or_else(|| series.first())?;
        Some(ImdbRatingResult {
            imdb_id: best["id"].as_str().map(String::from),
            rating: None,
            votes: None,
        })
    }

    async fn imdb_id_for(&self, title: &str) -> Option<String> {
        if title.starts_with("tt") && title[2..].chars().all(|c| c.is_ascii_digit()) {
            return Some(title.to_string());
        }
        let key = title.to_lowercase();
        if let Some(cached) = self.ids.get(&key).await {
            return cached;
        }
        let found = self.fetch_rating(title).await.and_then(|r| r.imdb_id);
        self.ids.insert(key, found.clone()).await;
        found
    }

    async fn fetch_season(&self, imdb_id: &str, season: u32) -> Option<Arc<ImdbSeason>> {
        let key = format!("{}|{}", imdb_id, season);
        self.seasons
            .try_get_with(key, async {
                let url = format!(
                    "https://www.imdb.com/title/{}/episodes/?season={}",
                    imdb_id, season
                );
                let res = self
                    .client
                    .get(&url)
                    .header("User-Agent", BROWSER_USER_AGENT)
                    .header("Accept-Language", "en-US,en;q=0.9")
                    .timeout(REQUEST_TIMEOUT)
                    .send()
                    .await
                    .map_err(|e| e.to_string())?;
                if !res.status().is_success() {
                    return Err(format!("IMDb HTTP {}", res.status().as_u16()));
                }
                let html = res.text().await.map_err(|e| e.to_string())?;
                Ok::<_, String>(Arc::new(parse_season_page(&html)))
            })
            .await
            .map_err(|reason| tracing::warn!("IMDb season page for {} failed: {}", imdb_id, reason))
            .ok()
    }

    /// Episode synopses from IMDb, by episode number. They are returned only when the anime can
    /// be tied to one IMDb season: the title must name a season (or none at all, meaning the
    /// first) and that season must list `expected_episodes` episodes. Otherwise the map is
    /// empty, because another season's plots would be worse than none.
    pub async fn fetch_episode_synopses(
        &self,
        title_or_imdb_id: &str,
        expected_episodes: Option<i32>,
    ) -> HashMap<i32, String> {
        let Some((base_title, season)) = imdb_season_for_title(title_or_imdb_id) else {
            return HashMap::new();
        };
        let Some(imdb_id) = self.imdb_id_for(&base_title).await else {
            return HashMap::new();
        };
        let Some(found) = self.fetch_season(&imdb_id, season).await else {
            return HashMap::new();
        };
        if !season_matches_expected(found.episode_count, expected_episodes) {
            return HashMap::new();
        }
        found.synopses.clone()
    }
}
