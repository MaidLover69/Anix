use crate::clients::app_settings;
use crate::error::AppError;
use crate::models::{AiringEntry, CoverImage, Media, MediaTitle};
use reqwest::header::RETRY_AFTER;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

pub const ANILIST_GRAPHQL_URL: &str = "https://graphql.anilist.co";

const MEDIA_FRAGMENT: &str = r#"
id
idMal
title {
    romaji
    english
    native
    userPreferred
}
coverImage {
    large
    extraLarge
    color
}
bannerImage
description
format
season
seasonYear
episodes
duration
status
averageScore
meanScore
popularity
favourites
isAdult
genres
"#;

const MEDIA_DETAILS_QUERY: &str = r#"
query ($id: Int) {
    Media(id: $id, type: ANIME) {
        id
        idMal
        title {
            romaji
            english
            native
            userPreferred
        }
        coverImage {
            large
            extraLarge
            color
        }
        bannerImage
        description
        format
        season
        seasonYear
        episodes
        duration
        status
        averageScore
        meanScore
        popularity
        favourites
        isAdult
        genres
        startDate { year month day }
        endDate { year month day }
        tags { name rank isMediaSpoiler }
        nextAiringEpisode {
            id
            airingAt
            timeUntilAiring
            episode
        }
        studios {
            edges {
                isMain
                node {
                    id
                    name
                }
            }
        }
        trailer {
            id
            site
            thumbnail
        }
        characters(sort: [ROLE, RELEVANCE], perPage: 18) {
            edges {
                role
                node {
                    id
                    name {
                        full
                        native
                    }
                    image {
                        large
                        medium
                    }
                }
                voiceActors(language: JAPANESE) {
                    id
                    name {
                        full
                        native
                    }
                    image {
                        large
                        medium
                    }
                    languageV2
                }
            }
        }
        relations {
            edges {
                relationType
                node {
                    id
                    idMal
                    title {
                        romaji
                        english
                        native
                        userPreferred
                    }
                    coverImage {
                        large
                        extraLarge
                        color
                    }
                    format
                    status
                    averageScore
                    seasonYear
                }
            }
        }
        recommendations(sort: [RATING_DESC], perPage: 15) {
            nodes {
                mediaRecommendation {
                    id
                    idMal
                    title {
                        romaji
                        english
                        native
                        userPreferred
                    }
                    coverImage {
                        large
                        extraLarge
                        color
                    }
                    format
                    status
                    averageScore
                    seasonYear
                    isAdult
                }
            }
        }
    }
}
"#;

/// The facts about an anime that provider search results are matched against.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaHint {
    pub id_mal: Option<i64>,
    /// Romaji first, then English, then synonyms.
    pub titles: Vec<String>,
    pub english: Option<String>,
    pub year: Option<i32>,
    pub episodes: Option<i32>,
    pub format: Option<String>,
}

const MEDIA_HINT_QUERY: &str = r#"
query ($id: Int) {
    Media(id: $id, type: ANIME) {
        idMal
        title {
            romaji
            english
        }
        synonyms
        seasonYear
        startDate {
            year
        }
        episodes
        format
    }
}
"#;

pub fn parse_media_hint(media: &Value) -> MediaHint {
    let mut titles: Vec<String> = Vec::new();
    let mut push = |value: &Value| {
        if let Some(title) = value.as_str().map(str::trim).filter(|t| !t.is_empty()) {
            if !titles.iter().any(|t| t.eq_ignore_ascii_case(title)) {
                titles.push(title.to_string());
            }
        }
    };
    push(&media["title"]["romaji"]);
    push(&media["title"]["english"]);
    if let Some(synonyms) = media["synonyms"].as_array() {
        for synonym in synonyms.iter().take(6) {
            push(synonym);
        }
    }

    MediaHint {
        id_mal: media["idMal"].as_i64(),
        titles,
        english: media["title"]["english"]
            .as_str()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(String::from),
        year: media["seasonYear"]
            .as_i64()
            .or_else(|| media["startDate"]["year"].as_i64())
            .map(|y| y as i32),
        episodes: media["episodes"].as_i64().map(|e| e as i32),
        format: media["format"].as_str().map(String::from),
    }
}

pub async fn fetch_media_hint(client: &Client, id: i64) -> Result<MediaHint, AppError> {
    let body = json!({ "query": MEDIA_HINT_QUERY, "variables": { "id": id } });
    let data = post_graphql(client, &body, None).await?;
    let media = &data["Media"];
    if media.is_null() {
        return Err(AppError::Parse(format!("Anime with ID {} not found", id)));
    }
    Ok(parse_media_hint(media))
}

fn first_graphql_error(json: &Value) -> Option<(String, Option<u64>)> {
    let first = json.get("errors")?.as_array()?.first()?;
    let message = first.get("message").and_then(Value::as_str).unwrap_or("unknown error");
    Some((message.to_string(), first.get("status").and_then(Value::as_u64)))
}

/// Turns an AniList HTTP status and body into the `data` object or the error they describe:
/// 429 is a rate limit, 401 (or a GraphQL "Invalid token") an auth failure, 400/404/422 (as
/// the HTTP status or the GraphQL error's) a rejected request, any other failing status or a
/// GraphQL `errors` array without data a network error carrying AniList's message.
pub fn interpret_graphql_response(
    status: u16,
    retry_after_s: Option<u64>,
    body: &str,
) -> Result<Value, AppError> {
    if status == StatusCode::TOO_MANY_REQUESTS.as_u16() {
        return Err(AppError::RateLimited {
            retry_after_s: retry_after_s.unwrap_or(60),
        });
    }

    let json: Option<Value> = serde_json::from_str(body).ok();
    let graphql_error = json.as_ref().and_then(first_graphql_error);

    if let Some((message, error_status)) = &graphql_error {
        if *error_status == Some(429) {
            return Err(AppError::RateLimited {
                retry_after_s: retry_after_s.unwrap_or(60),
            });
        }
        if status == 401 || *error_status == Some(401) || message.eq_ignore_ascii_case("Invalid token") {
            return Err(AppError::AuthRequired);
        }
    } else if status == 401 {
        return Err(AppError::AuthRequired);
    }

    // A request AniList refuses (bad variables such as an id beyond 32 bits, or a record it
    // does not have) is not an outage: a `Parse` error, which neither pauses AniList nor counts
    // as offline. 403 ("API disabled"), 5xx and the rest still do.
    let ok_status = (200..300).contains(&status);
    let data = json.as_ref().and_then(|j| j.get("data")).filter(|d| d.is_object());
    let refused = if ok_status { graphql_error.as_ref().and_then(|(_, s)| *s) } else { Some(status as u64) };
    let said = graphql_error.as_ref().map(|(m, _)| format!(": {}", m)).unwrap_or_default();
    match refused {
        // A missing record is reported as HTTP 404 with a null field; let the caller see it.
        Some(404) if data.is_some() => return Ok(data.unwrap().clone()),
        Some(404) => return Err(AppError::Parse(format!("AniList: not found (HTTP 404){}", said))),
        Some(code @ (400 | 422)) if !ok_status || data.is_none() => {
            return Err(AppError::Parse(format!("AniList rejected the request (HTTP {}){}", code, said)));
        }
        _ => {}
    }

    if !(200..300).contains(&status) {
        return Err(match graphql_error {
            Some((message, _)) => AppError::Network(format!("AniList HTTP {}: {}", status, message)),
            None => AppError::Network(format!("AniList HTTP {}", status)),
        });
    }

    let json = json.ok_or_else(|| AppError::Parse("AniList sent invalid JSON".to_string()))?;
    match json.get("data") {
        Some(data) if data.is_object() => Ok(data.clone()),
        _ => Err(match graphql_error {
            Some((message, _)) => AppError::Network(format!("AniList error: {}", message)),
            None => AppError::Parse("AniList response has no data".to_string()),
        }),
    }
}

/// Posts a GraphQL request to AniList and returns its `data` object.
pub async fn post_graphql(client: &Client, body: &Value, token: Option<&str>) -> Result<Value, AppError> {
    post_graphql_to(client, ANILIST_GRAPHQL_URL, body, token).await
}

pub async fn post_graphql_to(
    client: &Client,
    url: &str,
    body: &Value,
    token: Option<&str>,
) -> Result<Value, AppError> {
    let mut request = client.post(url).json(body);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let retry_after_s = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let text = response.text().await?;
    interpret_graphql_response(status, retry_after_s, &text)
}

/// The viewer's list entry for one anime.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListEntryState {
    pub status: String,
    pub progress: i32,
}

/// What list sync needs to know about one anime; `entry` is only filled for a signed-in request.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaListContext {
    pub id_mal: Option<i32>,
    pub episodes: Option<i32>,
    /// AniList `MediaStatus`: FINISHED, RELEASING, NOT_YET_RELEASED, CANCELLED, HIATUS.
    pub media_status: Option<String>,
    pub entry: Option<ListEntryState>,
}

const MEDIA_LIST_CONTEXT_QUERY: &str = r#"
query ($id: Int) {
    Media(id: $id, type: ANIME) {
        idMal
        episodes
        status
        mediaListEntry {
            id
            status
            progress
        }
    }
}
"#;

pub fn parse_media_list_context(media: &Value) -> MediaListContext {
    let entry = &media["mediaListEntry"];
    MediaListContext {
        id_mal: media["idMal"].as_i64().map(|v| v as i32),
        episodes: media["episodes"].as_i64().map(|v| v as i32),
        media_status: media["status"].as_str().map(String::from),
        entry: entry["status"].as_str().map(|status| ListEntryState {
            status: status.to_string(),
            progress: entry["progress"].as_i64().unwrap_or(0) as i32,
        }),
    }
}

pub struct AniListClient {
    client: Client,
    url: String,
}

impl AniListClient {
    pub fn new(client: Client) -> Self {
        Self::with_base_url(client, ANILIST_GRAPHQL_URL)
    }

    /// A client for another GraphQL endpoint (tests point it at a local server).
    pub fn with_base_url(client: Client, url: &str) -> Self {
        Self {
            client,
            url: url.to_string(),
        }
    }

    async fn graphql(&self, body: &Value, token: Option<&str>) -> Result<Value, AppError> {
        post_graphql_to(&self.client, &self.url, body, token).await
    }

    /// MAL id, episode count and airing status of one anime, plus the viewer's list entry when
    /// `access_token` is given. `None` when AniList has no such anime.
    pub async fn list_context(
        &self,
        access_token: Option<&str>,
        media_id: i32,
    ) -> Result<Option<MediaListContext>, AppError> {
        let body = json!({ "query": MEDIA_LIST_CONTEXT_QUERY, "variables": { "id": media_id } });
        let data = self.graphql(&body, access_token).await?;
        let media = &data["Media"];
        if media.is_null() {
            return Ok(None);
        }
        Ok(Some(parse_media_list_context(media)))
    }

    pub async fn fetch_media_list(
        &self,
        sort: &str,
        per_page: i32,
    ) -> Result<Vec<Media>, AppError> {
        let query = format!(
            r#"
            query ($sort: [MediaSort], $perPage: Int, $isAdult: Boolean) {{
                Page(perPage: $perPage) {{
                    media(sort: $sort, type: ANIME, isAdult: $isAdult) {{
                        {}
                    }}
                }}
            }}
            "#,
            MEDIA_FRAGMENT
        );

        let mut variables = serde_json::Map::new();
        variables.insert("sort".to_string(), json!([sort]));
        variables.insert("perPage".to_string(), json!(per_page));
        if app_settings::current().hide_adult_content {
            variables.insert("isAdult".to_string(), json!(false));
        }

        let body = json!({
            "query": query,
            "variables": variables
        });

        let data = self.graphql(&body, None).await?;
        let media_arr = data["Page"]["media"]
            .as_array()
            .ok_or_else(|| AppError::Parse("Invalid media array response".to_string()))?;

        let mut list = Vec::new();
        for item in media_arr {
            if let Ok(media) = parse_media_json(item) {
                list.push(media);
            }
        }
        Ok(list)
    }

    pub async fn search(&self, q: &crate::catalogue::model::SearchQuery) -> Result<Vec<Media>, AppError> {
        let gql_query = format!(
            r#"
            query ($page: Int, $perPage: Int, $search: String, $genre_in: [String], $genre_not_in: [String], $format: MediaFormat, $status: MediaStatus, $sort: [MediaSort], $isAdult: Boolean, $averageScore_greater: Int, $seasonYear: Int, $season: MediaSeason) {{
                Page(page: $page, perPage: $perPage) {{
                    media(search: $search, genre_in: $genre_in, genre_not_in: $genre_not_in, format: $format, status: $status, sort: $sort, type: ANIME, isAdult: $isAdult, averageScore_greater: $averageScore_greater, seasonYear: $seasonYear, season: $season) {{
                        {}
                    }}
                }}
            }}
            "#,
            MEDIA_FRAGMENT
        );

        let mut variables = serde_json::Map::new();
        variables.insert("page".to_string(), json!(q.page));
        variables.insert("perPage".to_string(), json!(q.per_page));

        if let Some(text) = q.text() {
            variables.insert("search".to_string(), json!(text));
        }
        if !q.genres.is_empty() {
            variables.insert("genre_in".to_string(), json!(q.genres));
        }
        if !q.exclude_genres.is_empty() {
            variables.insert("genre_not_in".to_string(), json!(q.exclude_genres));
        }
        if let Some(f) = q.format.as_deref().filter(|f| !f.trim().is_empty()) {
            variables.insert("format".to_string(), json!(f));
        }
        if let Some(s) = q.status.as_deref().filter(|s| !s.trim().is_empty()) {
            variables.insert("status".to_string(), json!(s));
        }
        // AniList's filter is "greater than", so one less keeps the minimum itself.
        if let Some(min) = q.min_score {
            variables.insert("averageScore_greater".to_string(), json!(min - 1));
        }
        if let Some(year) = q.year {
            variables.insert("seasonYear".to_string(), json!(year));
        }
        if let Some(season) = q.season.as_deref() {
            variables.insert("season".to_string(), json!(season));
        }
        variables.insert("sort".to_string(), json!([q.sort()]));
        if app_settings::current().hide_adult_content {
            variables.insert("isAdult".to_string(), json!(false));
        }

        let body = json!({
            "query": gql_query,
            "variables": variables
        });

        let data = self.graphql(&body, None).await?;
        let media_arr = data["Page"]["media"]
            .as_array()
            .ok_or_else(|| AppError::Parse("Invalid search media array response".to_string()))?;

        let mut list = Vec::new();
        for item in media_arr {
            if let Ok(media) = parse_media_json(item) {
                list.push(media);
            }
        }
        Ok(list)
    }

    pub async fn fetch_details(&self, id: i64) -> Result<serde_json::Value, AppError> {
        let body = json!({
            "query": MEDIA_DETAILS_QUERY,
            "variables": {
                "id": id
            }
        });

        let data = self.graphql(&body, None).await?;
        let media_obj = &data["Media"];
        if media_obj.is_null() {
            return Err(AppError::Parse(format!("Anime with ID {} not found", id)));
        }
        Ok(media_obj.clone())
    }

    pub async fn get_viewer(&self, access_token: &str) -> Result<crate::models::Viewer, AppError> {
        let query = r#"
            query {
                Viewer {
                    id
                    name
                    avatar {
                        large
                    }
                    statistics {
                        anime {
                            count
                            episodesWatched
                            minutesWatched
                            meanScore
                        }
                    }
                }
            }
        "#;

        let body = json!({ "query": query });
        let data = self.graphql(&body, Some(access_token)).await?;
        let viewer_val = &data["Viewer"];
        if viewer_val.is_null() {
            return Err(AppError::Parse("Viewer query returned null".to_string()));
        }

        let id = viewer_val["id"].as_i64().unwrap_or(0) as i32;
        let name = viewer_val["name"].as_str().unwrap_or("").to_string();
        let avatar_url = viewer_val["avatar"]["large"].as_str().map(String::from);
        let anime_stats = &viewer_val["statistics"]["anime"];
        let anime_count = anime_stats["count"].as_i64().unwrap_or(0) as i32;
        let episodes_watched = anime_stats["episodesWatched"].as_i64().unwrap_or(0) as i32;
        let minutes_watched = anime_stats["minutesWatched"].as_i64().unwrap_or(0);
        let mean_score = anime_stats["meanScore"].as_f64().unwrap_or(0.0);

        Ok(crate::models::Viewer {
            id,
            name,
            avatar_url,
            anime_count,
            episodes_watched,
            minutes_watched,
            mean_score,
        })
    }

    pub async fn get_user_library(
        &self,
        access_token: &str,
        user_id: i32,
        status_filter: Option<String>,
    ) -> Result<Vec<crate::models::MediaListEntry>, AppError> {
        let query = r#"
            query ($userId: Int, $status: MediaListStatus) {
                MediaListCollection(userId: $userId, type: ANIME, status: $status) {
                    lists {
                        name
                        status
                        entries {
                            id
                            mediaId
                            status
                            progress
                            score
                            media {
                                id
                                title {
                                    userPreferred
                                    english
                                    romaji
                                }
                                coverImage {
                                    large
                                    extraLarge
                                }
                                format
                                episodes
                                averageScore
                            }
                        }
                    }
                }
            }
        "#;

        let mut variables = serde_json::Map::new();
        variables.insert("userId".to_string(), json!(user_id));
        if let Some(ref s) = status_filter {
            if !s.trim().is_empty() {
                variables.insert("status".to_string(), json!(s));
            }
        }

        let body = json!({
            "query": query,
            "variables": variables
        });

        let data = self.graphql(&body, Some(access_token)).await?;
        let lists = data["MediaListCollection"]["lists"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let mut result = Vec::new();
        // An entry that is also on a custom list is returned once per list.
        let mut seen = std::collections::HashSet::new();
        for list in lists {
            if let Some(entries) = list["entries"].as_array() {
                for item in entries {
                    let entry_id = item["id"].as_i64().unwrap_or(0) as i32;
                    if !seen.insert(entry_id) {
                        continue;
                    }
                    let media_id = item["mediaId"].as_i64().unwrap_or(0) as i32;
                    let progress = item["progress"].as_i64().unwrap_or(0) as i32;
                    let score = item["score"].as_f64().unwrap_or(0.0);
                    let status_str = item["status"].as_str().map(String::from);

                    let media_val = &item["media"];
                    let title = media_val["title"]["userPreferred"]
                        .as_str()
                        .or_else(|| media_val["title"]["english"].as_str())
                        .or_else(|| media_val["title"]["romaji"].as_str())
                        .map(String::from);
                    let cover = media_val["coverImage"]["extraLarge"]
                        .as_str()
                        .or_else(|| media_val["coverImage"]["large"].as_str())
                        .map(String::from);
                    let format_str = media_val["format"].as_str().map(String::from);
                    let total_episodes = media_val["episodes"].as_i64().map(|v| v as i32);
                    let average_score = media_val["averageScore"].as_i64().map(|v| v as i32);

                    result.push(crate::models::MediaListEntry {
                        id: if media_id > 0 { media_id } else { entry_id },
                        progress,
                        score,
                        status: status_str,
                        title,
                        cover,
                        format_str,
                        total_episodes,
                        average_score,
                    });
                }
            }
        }

        Ok(result)
    }

    pub async fn save_list_entry(
        &self,
        access_token: &str,
        media_id: i32,
        status: Option<String>,
        progress: Option<i32>,
        score: Option<f64>,
    ) -> Result<crate::models::MediaListEntry, AppError> {
        let mutation = r#"
            mutation ($mediaId: Int, $status: MediaListStatus, $progress: Int, $score: Float) {
                SaveMediaListEntry(mediaId: $mediaId, status: $status, progress: $progress, score: $score) {
                    id
                    mediaId
                    status
                    progress
                    score
                }
            }
        "#;

        let mut variables = serde_json::Map::new();
        variables.insert("mediaId".to_string(), json!(media_id));
        if let Some(ref s) = status {
            variables.insert("status".to_string(), json!(s));
        }
        if let Some(p) = progress {
            variables.insert("progress".to_string(), json!(p));
        }
        if let Some(sc) = score {
            variables.insert("score".to_string(), json!(sc));
        }

        let body = json!({
            "query": mutation,
            "variables": variables
        });

        let data = self.graphql(&body, Some(access_token)).await?;
        let entry_val = &data["SaveMediaListEntry"];
        if entry_val.is_null() {
            return Err(AppError::Parse("SaveMediaListEntry returned null".to_string()));
        }

        let id = entry_val["mediaId"].as_i64().unwrap_or(media_id as i64) as i32;
        let res_progress = entry_val["progress"].as_i64().unwrap_or(0) as i32;
        let res_score = entry_val["score"].as_f64().unwrap_or(0.0);
        let res_status = entry_val["status"].as_str().map(String::from);

        Ok(crate::models::MediaListEntry {
            id,
            progress: res_progress,
            score: res_score,
            status: res_status,
            title: None,
            cover: None,
            format_str: None,
            total_episodes: None,
            average_score: None,
        })
    }

    pub async fn fetch_airing_schedule(
        &self,
        from_ts: i64,
        to_ts: i64,
    ) -> Result<Vec<AiringEntry>, AppError> {
        const AIRING_SCHEDULE_QUERY: &str = r#"
        query ($from: Int, $to: Int, $page: Int) {
            Page(page: $page, perPage: 50) {
                pageInfo {
                    hasNextPage
                }
                airingSchedules(airingAt_greater: $from, airingAt_lesser: $to) {
                    id
                    airingAt
                    episode
                    media {
                        id
                        title {
                            english
                            userPreferred
                            romaji
                        }
                        coverImage {
                            extraLarge
                            large
                        }
                        bannerImage
                        format
                        duration
                        averageScore
                        isAdult
                    }
                }
            }
        }
        "#;

        let hide_adult = app_settings::current().hide_adult_content;
        let mut all_entries = Vec::new();
        let mut page = 1;

        loop {
            let body = json!({
                "query": AIRING_SCHEDULE_QUERY,
                "variables": {
                    "from": from_ts,
                    "to": to_ts,
                    "page": page,
                }
            });

            let data = self.graphql(&body, None).await?;
            let page_data = &data["Page"];
            let schedules = page_data["airingSchedules"]
                .as_array()
                .ok_or_else(|| AppError::Parse("Failed to parse airingSchedules".to_string()))?;

            for s in schedules {
                let id = s["id"].as_i64().unwrap_or(0);
                let airing_at = s["airingAt"].as_i64().unwrap_or(0);
                let episode = s["episode"].as_i64().unwrap_or(0) as i32;

                let media = &s["media"];
                if hide_adult && media["isAdult"].as_bool().unwrap_or(false) {
                    continue;
                }
                let media_id = media["id"].as_i64().unwrap_or(0);

                let title_val = &media["title"];
                let media_title = title_val["english"]
                    .as_str()
                    .or_else(|| title_val["userPreferred"].as_str())
                    .or_else(|| title_val["romaji"].as_str())
                    .unwrap_or("Untitled")
                    .to_string();

                let cover_val = &media["coverImage"];
                let cover_image = cover_val["extraLarge"]
                    .as_str()
                    .or_else(|| cover_val["large"].as_str())
                    .map(String::from);

                let banner_image = media["bannerImage"].as_str().map(String::from);
                let format = media["format"].as_str().map(String::from);
                let duration = media["duration"].as_i64().map(|v| v as i32);
                let average_score = media["averageScore"].as_i64().map(|v| v as i32);

                all_entries.push(AiringEntry {
                    id,
                    airing_at,
                    episode,
                    media_id,
                    media_title,
                    cover_image,
                    banner_image,
                    format,
                    duration,
                    average_score,
                });
            }

            let has_next_page = page_data["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false);
            if !has_next_page || page >= 10 {
                break;
            }
            page += 1;
        }

        Ok(all_entries)
    }
}

pub fn parse_media_json(val: &serde_json::Value) -> Result<Media, AppError> {
    let id = val["id"]
        .as_i64()
        .ok_or_else(|| AppError::Parse("Missing media id".to_string()))? as i32;

    let id_mal = val["idMal"].as_i64().map(|v| v as i32);

    let title_val = &val["title"];
    let title = MediaTitle {
        romaji: title_val["romaji"].as_str().map(String::from),
        english: title_val["english"].as_str().map(String::from),
        native: title_val["native"].as_str().map(String::from),
        user_preferred: title_val["userPreferred"].as_str().map(String::from),
    };

    let cover_val = &val["coverImage"];
    let cover_image = CoverImage {
        large: cover_val["large"].as_str().map(String::from),
        extra_large: cover_val["extraLarge"].as_str().map(String::from),
        color: cover_val["color"].as_str().map(String::from),
    };

    let banner_image = val["bannerImage"].as_str().map(String::from);
    let description = val["description"].as_str().map(String::from);
    let format = val["format"].as_str().map(String::from);
    let season = val["season"].as_str().map(String::from);
    let season_year = val["seasonYear"].as_i64().map(|v| v as i32);
    let episodes = val["episodes"].as_i64().map(|v| v as i32);
    let duration = val["duration"].as_i64().map(|v| v as i32);
    let status = val["status"].as_str().map(String::from);
    let average_score = val["averageScore"].as_i64().map(|v| v as i32);
    let mean_score = val["meanScore"].as_i64().map(|v| v as i32);
    let popularity = val["popularity"].as_i64().map(|v| v as i32);
    let favourites = val["favourites"].as_i64().map(|v| v as i32);
    let is_adult = val["isAdult"].as_bool().unwrap_or(false);

    let genres = val["genres"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let next_airing_episode = if !val["nextAiringEpisode"].is_null() {
        let na = &val["nextAiringEpisode"];
        Some(crate::models::NextAiringEpisode {
            id: na["id"].as_i64().unwrap_or(0) as i32,
            airing_at: na["airingAt"].as_i64().unwrap_or(0),
            time_until_airing: na["timeUntilAiring"].as_i64().unwrap_or(0),
            episode: na["episode"].as_i64().unwrap_or(0) as i32,
        })
    } else {
        None
    };

    Ok(Media {
        id,
        id_mal,
        imdb_id: None,
        title,
        cover_image,
        banner_image,
        description,
        // AniList knows neither MAL's nor IMDb's synopsis or score; those stay unset here.
        synopsis_mal: None,
        synopsis_imdb: None,
        format,
        season,
        season_year,
        episodes,
        duration,
        status,
        average_score,
        score_mal: None,
        rating_imdb: None,
        mean_score,
        popularity,
        favourites,
        is_adult,
        genres,
        next_airing_episode,
        trailer: None,
        characters: Vec::new(),
        relations: Vec::new(),
    })
}
