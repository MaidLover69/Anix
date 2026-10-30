//! The multi-source anime catalogue: each catalogue's parser against responses recorded from
//! the real services (`tests/fixtures/catalogue`), synthetic ids, the breaker, and falling
//! through from one catalogue to the next against local servers.

use anix_lib::catalogue::breaker::{self, Breaker, OPEN_FOR};
use anix_lib::catalogue::ids::{self, AnimeKey, ExtIds, Site};
use anix_lib::catalogue::model::{self, Found, SearchQuery};
use anix_lib::catalogue::{comick, imdb, jikan, kitsu, mal, order, schedule, shikimori};
use anix_lib::catalogue::{Catalogue, Endpoints, Source};
use anix_lib::clients::anilist::{interpret_graphql_response, parse_media_hint};
use anix_lib::clients::app_settings::CatalogueSource;
use anix_lib::error::AppError;
use anix_lib::{db, offline};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Response, Server, StatusCode};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/catalogue/{}", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", path, e))
}

fn fixture_json(name: &str) -> Value {
    serde_json::from_str(&fixture(name)).unwrap()
}

// ---------------------------------------------------------------------------- parsers

#[test]
fn anilist_disabled_answer_pauses_anilist() {
    let err = interpret_graphql_response(403, None, &fixture("anilist_disabled.json")).unwrap_err();
    assert!(err.to_string().contains("temporarily disabled"), "{}", err);
    assert!(breaker::trips(&err));
    assert!(offline::is_offline_error(&err));
}

#[test]
fn mal_search_maps_into_media() {
    let list = mal::parse_list(&fixture_json("mal_search.json"));
    assert_eq!(list.len(), 2);
    let season2 = &list[0];
    assert_eq!(season2.ids.mal, Some(59978));
    let m = &season2.media;
    assert_eq!(m.title.romaji.as_deref(), Some("Sousou no Frieren 2nd Season"));
    assert_eq!(m.title.english.as_deref(), Some("Frieren: Beyond Journey's End Season 2"));
    assert_eq!(m.title.native.as_deref(), Some("葬送のフリーレン 第2期"));
    assert_eq!(m.format.as_deref(), Some("TV"));
    assert_eq!(m.status.as_deref(), Some("FINISHED"));
    assert_eq!(m.season.as_deref(), Some("WINTER"));
    assert_eq!(m.season_year, Some(2026));
    assert_eq!(m.episodes, Some(10));
    assert_eq!(m.duration, Some(24));
    assert_eq!(m.average_score, Some(88));
    assert_eq!(m.score_mal, Some(8.84));
    // Demographics such as "Shounen" are not AniList genres.
    assert_eq!(m.genres, vec!["Adventure", "Drama", "Fantasy"]);
    assert!(m.cover_image.extra_large.as_deref().unwrap().ends_with("154528l.webp"));
    assert!(m.description.as_deref().unwrap().starts_with("Following the First-Class Mage Exam"));
    assert!(!m.is_adult);
    assert_eq!(season2.studios, vec!["Madhouse"]);
    assert_eq!(season2.synonyms, vec!["Frieren at the Funeral Season 2"]);
}

#[test]
fn mal_details_ranking_and_season_parse() {
    let details = mal::parse_details(&fixture_json("mal_details.json")).unwrap();
    assert_eq!(details.ids.mal, Some(52991));
    assert!(!details.related.is_empty());
    assert!(details.related.iter().all(|r| r.relation.is_some() && r.ids.mal.is_some()));
    assert!(details.related.iter().any(|r| r.relation.as_deref() == Some("SEQUEL")), "{:?}", details.related.iter().map(|r| &r.relation).collect::<Vec<_>>());

    assert!(!mal::parse_list(&fixture_json("mal_ranking.json")).is_empty());
    let season = mal::parse_list(&fixture_json("mal_season.json"));
    assert!(season.iter().any(|f| f.media.title.romaji.as_deref() == Some("One Piece")));
}

#[test]
fn kitsu_list_reads_mappings_per_anime() {
    let list = kitsu::parse_list(&fixture_json("kitsu_search.json"));
    assert_eq!(list.len(), 2);
    let first = &list[0];
    assert_eq!(first.ids, ExtIds { anilist: Some(154587), mal: Some(52991), kitsu: Some(46474), imdb: None });
    let m = &first.media;
    assert_eq!(m.title.romaji.as_deref(), Some("Sousou no Frieren"));
    assert_eq!(m.title.native.as_deref(), Some("葬送のフリーレン"));
    assert_eq!(m.format.as_deref(), Some("TV"));
    assert_eq!(m.status.as_deref(), Some("FINISHED"));
    assert_eq!(m.season.as_deref(), Some("FALL"));
    assert_eq!(m.season_year, Some(2023));
    assert_eq!(m.episodes, Some(28));
    assert_eq!(m.average_score, Some(89));
    assert!(m.banner_image.as_deref().unwrap().contains("cover_image"));
    assert_eq!(m.genres, vec!["Fantasy", "Adventure", "Drama", "Slice of Life"]);
    // The second result's mappings are its own.
    assert_ne!(list[1].ids.anilist, first.ids.anilist);
    assert!(list[1].ids.anilist.is_some());

    let one = kitsu::parse_document(&fixture_json("kitsu_details.json")).unwrap();
    assert_eq!(one.ids.kitsu, Some(46474));
    assert_eq!(one.ids.anilist, Some(154587));
}

#[test]
fn shikimori_search_and_details_parse() {
    let list = shikimori::parse_list(&fixture_json("shiki_search.json"));
    assert!(!list.is_empty());
    assert!(list.iter().all(|f| f.ids.mal.is_some() && f.media.description.is_none()));

    let json = fixture_json("shiki_details.json");
    let details = shikimori::parse_details(&json["data"]["animes"][0]).unwrap();
    assert_eq!(details.ids.mal, Some(52991));
    assert!(details.media.average_score.is_some());
    assert_eq!(details.studios, vec!["Madhouse"]);
    assert!(details.related.iter().any(|r| r.relation.as_deref() == Some("SEQUEL") && r.ids.mal == Some(59978)));
    let chars = &details.media.characters;
    assert!(!chars.is_empty() && chars.len() <= 18);
    assert_eq!(chars[0].role.as_deref(), Some("MAIN"));
}

#[test]
fn shikimori_next_episode_becomes_airing_data() {
    let list = shikimori::parse_list(&fixture_json("shiki_ongoing.json"));
    let one_piece = list.iter().find(|f| f.ids.mal == Some(21)).unwrap();
    let next = one_piece.media.next_airing_episode.as_ref().unwrap();
    assert_eq!(next.episode, 1181);
    // 2026-10-25T17:15:00+03:00
    assert_eq!(next.airing_at, 1_792_937_700);
    assert_eq!(model::parse_datetime("2026-10-25T14:15:00Z"), Some(1_792_937_700));
}

#[test]
fn comick_catalogue_parses_external_ids() {
    let list = comick::parse_list(&fixture_json("comick_search.json"));
    let frieren = &list[0];
    assert_eq!(frieren.ids.anilist, Some(154587));
    assert_eq!(frieren.ids.mal, Some(52991));
    assert_eq!(frieren.media.episodes, Some(28));
    assert_eq!(frieren.media.season_year, Some(2023));
    assert_eq!(frieren.media.title.english.as_deref(), Some("Frieren: Beyond Journey's End"));
    assert_eq!(frieren.media.cover_image.large.as_deref(), Some("https://meo.comick.pictures/D4qa6z.jpg"));
    assert_eq!(frieren.media.average_score, Some(92));
}

#[test]
fn imdb_suggestions_keep_series_and_films() {
    let list = imdb::parse(&fixture_json("imdb_suggestion.json"));
    assert_eq!(list[0].ids.imdb, Some(22248376));
    assert_eq!(list[0].media.format.as_deref(), Some("TV"));
    assert_eq!(list[0].media.season_year, Some(2023));
    assert!(list[0].media.cover_image.large.is_some());
    assert_eq!(imdb::suggestion_key("Frieren: Beyond"), "Frieren__Beyond");
}

/// Jikan did not answer while these fixtures were recorded (it timed out), so this sample is
/// written by hand in Jikan v4's documented shape.
#[test]
fn jikan_anime_parses() {
    let sample = json!({ "data": [{
        "mal_id": 52991, "title": "Sousou no Frieren", "title_english": "Frieren: Beyond Journey's End",
        "title_japanese": "葬送のフリーレン", "title_synonyms": ["Frieren at the Funeral"],
        "images": { "jpg": { "image_url": "https://cdn.myanimelist.net/images/anime/1015/138006.jpg", "large_image_url": "https://cdn.myanimelist.net/images/anime/1015/138006l.jpg" } },
        "trailer": { "youtube_id": "qgQBl5Go1a8" }, "type": "TV", "episodes": 28, "status": "Finished Airing",
        "duration": "24 min per ep", "rating": "PG-13 - Teens 13 or older", "score": 9.29, "members": 1200000,
        "favorites": 70000, "synopsis": "During their decade-long quest…", "season": "fall", "year": 2023,
        "studios": [{ "name": "Madhouse" }], "genres": [{ "name": "Adventure" }, { "name": "Drama" }, { "name": "Fantasy" }],
        "themes": [], "demographics": [{ "name": "Shounen" }]
    }]});
    let list = jikan::parse_list(&sample);
    let m = &list[0].media;
    assert_eq!(list[0].ids.mal, Some(52991));
    assert_eq!(m.format.as_deref(), Some("TV"));
    assert_eq!(m.status.as_deref(), Some("FINISHED"));
    assert_eq!(m.season.as_deref(), Some("FALL"));
    assert_eq!(m.duration, Some(24));
    assert_eq!(m.average_score, Some(93));
    assert_eq!(m.genres, vec!["Adventure", "Drama", "Fantasy"]);
    assert_eq!(jikan::duration_minutes("1 hr 50 min"), Some(110));
}

#[test]
fn mapping_services_parse() {
    let arm = ids::parse_arm(&fixture_json("arm_ids.json")).unwrap();
    assert_eq!(arm, ExtIds { anilist: Some(154587), mal: Some(52991), kitsu: Some(46474), imdb: Some(22248376) });
    let bulk = fixture_json("arm_bulk.json");
    assert_eq!(ids::parse_arm(&bulk[0]).unwrap().anilist, Some(154587));
    assert_eq!(ids::parse_arm(&bulk[1]), None);
    let by_imdb = fixture_json("arm_imdb.json");
    assert_eq!(ids::parse_arm(&by_imdb[0]).unwrap().anilist, Some(154587));
    let anizip = ids::parse_anizip(&fixture_json("anizip_mappings.json")).unwrap();
    assert_eq!(anizip, arm);
}

#[test]
fn details_json_has_anilist_shape() {
    let mut found = mal::parse_details(&fixture_json("mal_details.json")).unwrap();
    found.media.id = 154587;
    for (i, r) in found.related.iter_mut().enumerate() {
        r.media.id = -(100_000_000 + i as i32 + 1);
    }
    let json = model::anilist_json(&found);
    assert_eq!(json["id"], 154587);
    assert_eq!(json["idMal"], 52991);
    assert_eq!(json["title"]["romaji"], "Sousou no Frieren");
    assert!(json["coverImage"]["extraLarge"].is_string());
    assert_eq!(json["studios"]["edges"][0]["node"]["name"], "Madhouse");
    assert!(json["relations"]["edges"][0]["node"]["id"].as_i64().unwrap() < 0);
    assert!(json["relations"]["edges"][0]["relationType"].is_string());
    assert!(json["characters"]["edges"].as_array().unwrap().is_empty());
    // Provider matching reads the same JSON.
    let hint = parse_media_hint(&json);
    assert_eq!(hint.id_mal, Some(52991));
    assert_eq!(hint.titles[0], "Sousou no Frieren");
    assert!(hint.titles.iter().any(|t| t == "Frieren at the Funeral"));
    assert_eq!(hint.year, Some(2023));
    // The local-file matcher reads it too.
    let candidate = anix_lib::local_library::parse_candidate(&json).unwrap();
    assert_eq!(candidate.id, 154587);
    assert_eq!(candidate.start_year, Some(2023));
}

#[test]
fn genres_and_filters() {
    assert_eq!(model::anilist_genre("Science Fiction"), Some("Sci-Fi"));
    assert_eq!(model::anilist_genre("Magical Girl"), Some("Mahou Shoujo"));
    assert_eq!(model::anilist_genre("Shounen"), None);
    assert_eq!(model::season_from_date(&json!("2023-09-29")), (Some("FALL".to_string()), Some(2023)));
    assert_eq!(model::season_from_date(&json!("2023-12-30")), (Some("WINTER".to_string()), Some(2024)));
    assert_eq!(kitsu::category_slug("Sci-Fi"), "science-fiction");
    assert_eq!(kitsu::category_slug("Slice of Life"), "slice-of-life");
    let q = SearchQuery { genres: vec!["Drama".into()], status: Some("FINISHED".into()), ..Default::default() };
    let list = mal::parse_list(&fixture_json("mal_search.json"));
    assert!(list.iter().all(|f| q.keeps(&f.media)));
    let q = SearchQuery { genres: vec!["Mecha".into()], ..Default::default() };
    assert!(!list.iter().any(|f| q.keeps(&f.media)));
    // Several genres: every one is wanted.
    let q = SearchQuery { genres: vec!["Drama".into(), "Mecha".into()], ..Default::default() };
    assert!(!list.iter().any(|f| q.keeps(&f.media)));
}

#[test]
fn shikimori_december_start_is_next_years_winter() {
    let anime = json!({ "malId": "1", "status": "released", "airedOn": { "year": 2023, "month": 12 } });
    let found = shikimori::parse_anime(&anime).unwrap();
    assert_eq!((found.media.season.as_deref(), found.media.season_year), (Some("WINTER"), Some(2024)));
}

#[test]
fn mal_rankings_follow_the_sort() {
    let q = |sort: &str, status: Option<&str>, format: Option<&str>| SearchQuery {
        sort: Some(sort.into()),
        status: status.map(String::from),
        format: format.map(String::from),
        ..Default::default()
    };
    assert_eq!(mal::ranking_type(&q("SCORE_DESC", Some("RELEASING"), None)), Some("airing"));
    assert_eq!(mal::ranking_type(&q("SCORE_DESC", None, Some("TV"))), Some("tv"));
    assert_eq!(mal::ranking_type(&q("FAVOURITES_DESC", None, None)), Some("favorite"));
    // Score-ordered rankings do not answer another sort; "newest" has no ranking at all.
    assert_eq!(mal::ranking_type(&q("POPULARITY_DESC", None, Some("TV"))), None);
    assert_eq!(mal::ranking_type(&q("FAVOURITES_DESC", Some("RELEASING"), None)), None);
    assert_eq!(mal::ranking_type(&q("START_DATE_DESC", None, None)), None);
    assert_eq!(mal::previous_season((2026, "winter")), (2025, "fall"));
    assert_eq!(mal::previous_season((2026, "fall")), (2026, "summer"));
}

#[test]
fn kitsu_pages_fill_the_apps_page() {
    let q = SearchQuery { page: 2, per_page: 24, ..Default::default() };
    assert_eq!(kitsu::page_requests(&q, false), vec![(24, 12), (36, 12)]);
    // Filtered searches read the offset as a 1-based page number (offset / limit).
    assert_eq!(kitsu::page_requests(&q, true), vec![(36, 12), (48, 12)]);
    let first = SearchQuery { page: 1, per_page: 20, ..Default::default() };
    assert_eq!(kitsu::page_requests(&first, true), vec![(20, 20)]);
    assert_eq!(kitsu::page_requests(&SearchQuery { page: 1, per_page: 5, ..Default::default() }, false), vec![(0, 5)]);
}

#[tokio::test]
async fn comick_refuses_filters_it_cannot_apply() {
    let q = SearchQuery { format: Some("TV".into()), page: 1, per_page: 10, ..Default::default() };
    let err = comick::search(&reqwest::Client::new(), "http://127.0.0.1:9", &q).await.unwrap_err();
    assert!(!breaker::trips(&err), "{:?}", err);
}

// ---------------------------------------------------------------------------- ids

#[test]
fn synthetic_ids_round_trip_and_never_collide() {
    for site in [Site::Mal, Site::Kitsu, Site::Imdb] {
        for native in [1, 52991, 99_999_999] {
            let id = ids::synthetic_id(site, native).unwrap();
            assert!(id < 0);
            assert!(i32::try_from(id).is_ok(), "{} must fit the frontend's ids", id);
            assert_eq!(ids::decode(id), Some(AnimeKey::Other(site, native)));
        }
    }
    assert_eq!(ids::synthetic_id(Site::Mal, 52991), Some(-100_052_991));
    assert_eq!(ids::synthetic_id(Site::Mal, 0), None);
    assert_eq!(ids::synthetic_id(Site::Kitsu, 100_000_000), None);
    assert_eq!(ids::decode(154587), Some(AnimeKey::AniList(154587)));
    assert_eq!(ids::decode(0), None);
    assert_eq!(ids::decode(-52991), None);
    assert_eq!(ids::decode(-400_000_001), None);
    assert_eq!(ids::imdb_number("tt0388629"), Some(388629));
    assert_eq!(ids::imdb_text(388629), "tt0388629");
    // MAL first, then Kitsu, then IMDb.
    let ids = ExtIds { anilist: None, mal: None, kitsu: Some(46474), imdb: Some(22248376) };
    assert_eq!(ids.synthetic(), Some(-200_046_474));
}

#[tokio::test]
async fn mapping_cache_remembers_answers() {
    let pool = db::init_db_in_memory().await.unwrap();
    let now = 1_800_000_000;
    let known = ExtIds { anilist: Some(154587), mal: Some(52991), kitsu: Some(46474), imdb: None };
    ids::remember(&pool, &known, now).await.unwrap();
    ids::remember(&pool, &ExtIds { mal: Some(1), ..Default::default() }, now).await.unwrap();
    // A later "unknown" never forgets a known AniList id.
    ids::remember(&pool, &ExtIds { mal: Some(52991), ..Default::default() }, now).await.unwrap();

    let hits = ids::cached_anilist(&pool, Site::Mal, &[52991, 1, 2], now).await.unwrap();
    assert_eq!(hits.get(&52991), Some(&Some(154587)));
    assert_eq!(hits.get(&1), Some(&None));
    assert_eq!(hits.get(&2), None);
    let later = ids::cached_anilist(&pool, Site::Mal, &[1], now + ids::NEGATIVE_TTL_S + 1).await.unwrap();
    assert!(later.is_empty(), "a negative answer is asked again after a week");
    let sites = ids::cached_sites(&pool, 154587).await.unwrap();
    assert_eq!(sites, known);
}

// ---------------------------------------------------------------------------- breaker

#[test]
fn breaker_pauses_failed_catalogues_for_a_while() {
    let breaker = Breaker::default();
    let t0 = Instant::now();
    let down: Result<(), AppError> = Err(AppError::Network("AniList HTTP 403: disabled".into()));
    breaker.record_at("AniList", &down, t0);
    assert!(breaker.is_open_at("AniList", t0 + Duration::from_secs(1)));
    assert!(!breaker.is_open_at("AniList", t0 + OPEN_FOR + Duration::from_secs(1)));
    assert!(!breaker.is_open_at("Kitsu", t0));

    // A bad request or a missing anime is not the catalogue's fault.
    let not_found: Result<(), AppError> = Err(AppError::Parse("not found".into()));
    breaker.record_at("Kitsu", &not_found, t0);
    assert!(!breaker.is_open_at("Kitsu", t0));
    let unsupported: Result<(), AppError> = Err(model::unsupported("Comick", "list by genre"));
    breaker.record_at("Comick", &unsupported, t0);
    assert!(!breaker.is_open_at("Comick", t0));

    // A long Retry-After is honoured up to a limit; a success closes the breaker.
    let limited: Result<(), AppError> = Err(AppError::RateLimited { retry_after_s: 600 });
    breaker.record_at("MyAnimeList", &limited, t0);
    assert!(breaker.is_open_at("MyAnimeList", t0 + Duration::from_secs(590)));
    breaker.record_at("MyAnimeList", &Ok::<(), AppError>(()), t0);
    assert!(!breaker.is_open_at("MyAnimeList", t0 + Duration::from_secs(1)));
}

#[test]
fn preference_goes_first() {
    use Source::*;
    assert_eq!(order(CatalogueSource::Auto), vec![AniList, Mal, Kitsu, Shikimori, Comick, Jikan]);
    assert_eq!(order(CatalogueSource::Kitsu), vec![Kitsu, AniList, Mal, Shikimori, Comick, Jikan]);
    assert_eq!(CatalogueSource::from_setting("MyAnimeList"), CatalogueSource::MyAnimeList);
    assert_eq!(CatalogueSource::from_setting("nonsense"), CatalogueSource::Auto);
}

// ---------------------------------------------------------------------------- schedule

#[test]
fn weekly_slots_fill_the_window() {
    let week = schedule::WEEK_S;
    let at = 1_000_000;
    // Two weeks before to two weeks after the known airing: episodes 9..=12 of 12 are in.
    let slots = schedule::weekly(at, 10, Some(12), true, at - week, at + 3 * week);
    assert_eq!(slots, vec![(at - week, 9), (at, 10), (at + week, 11), (at + 2 * week, 12)]);
    assert_eq!(schedule::weekly(at, 1, None, false, at - 10, at + 10), vec![(at, 1)]);
    assert!(schedule::weekly(at, 1, None, false, at + 1, at + 10).is_empty());
    assert!(schedule::entry_id(154587, 3) < 0);
    assert_ne!(schedule::entry_id(154587, 3), schedule::entry_id(154587, 4));
}

#[test]
fn mal_broadcast_is_converted_from_jst() {
    // Started Friday 2026-01-16, broadcast Fridays 23:00 JST = 14:00 UTC.
    let node = json!({
        "start_date": "2026-01-16",
        "broadcast": { "day_of_the_week": "friday", "start_time": "23:00" }
    });
    let first = schedule::mal_first_broadcast(&node).unwrap();
    assert_eq!(model::parse_datetime("2026-01-16T14:00:00Z"), Some(first));
    // A start date before the broadcast weekday moves to that weekday.
    let node = json!({
        "start_date": "2026-01-14",
        "broadcast": { "day_of_the_week": "friday", "start_time": "01:30" }
    });
    assert_eq!(schedule::mal_first_broadcast(&node), model::parse_datetime("2026-01-15T16:30:00Z"));
    // Ramparts of Ice S2: "starts" Friday 2026-10-02 in a Thursday 23:56 slot, a late-night
    // airing of the Thursday: the first episode is Thursday's, not the next week's.
    let node = json!({
        "start_date": "2026-10-02",
        "broadcast": { "day_of_the_week": "thursday", "start_time": "23:56" }
    });
    assert_eq!(schedule::mal_first_broadcast(&node), model::parse_datetime("2026-10-01T14:56:00Z"));

    let season = fixture_json("mal_season.json");
    let from = model::parse_datetime("2026-10-22T00:00:00Z").unwrap();
    let slots = schedule::from_mal(&season, from, from + schedule::WEEK_S);
    // One Piece has no episode total and began in 1999: its count is not believable, so it is
    // listed without a number instead of being left out.
    let one_piece = slots.iter().find(|(f, _)| f.ids.mal == Some(21)).expect("long-runner kept");
    assert!(one_piece.1.iter().all(|(_, ep)| *ep == schedule::UNKNOWN_EPISODE), "{:?}", one_piece.1);
}

// ---------------------------------------------------------------------------- fall-through

struct TestServer {
    base: String,
    log: Arc<Mutex<Vec<(String, String, Vec<(String, String)>)>>>,
}

impl TestServer {
    fn hits(&self, prefix: &str) -> usize {
        self.log.lock().unwrap().iter().filter(|(_, url, _)| url.starts_with(prefix)).count()
    }

    fn header(&self, prefix: &str, name: &str) -> Option<String> {
        self.log.lock().unwrap().iter().find(|(_, url, _)| url.starts_with(prefix)).and_then(|(_, _, headers)| {
            headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
        })
    }

    fn endpoints(&self) -> Endpoints {
        let at = |path: &str| format!("{}/{}", self.base, path);
        Endpoints {
            anilist: at("anilist"),
            mal: at("mal"),
            kitsu: at("kitsu"),
            shikimori: at("shiki"),
            comick: at("comick"),
            jikan: at("jikan"),
            imdb: at("imdb"),
            arm: at("arm"),
            anizip: at("anizip"),
        }
    }
}

fn serve<F>(route: F) -> TestServer
where
    F: Fn(&str, &str, &str) -> (u16, String) + Send + Sync + 'static,
{
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let log = Arc::new(Mutex::new(Vec::new()));
    let seen = log.clone();
    let route = Arc::new(route);
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let (seen, route) = (seen.clone(), route.clone());
            // One thread per request, so a slow answer does not hold up the others.
            std::thread::spawn(move || {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let method = request.method().to_string();
                let url = request.url().to_string();
                let headers = request.headers().iter().map(|h| (h.field.to_string(), h.value.to_string())).collect();
                seen.lock().unwrap().push((method.clone(), url.clone(), headers));
                let (status, text) = route(&method, &url, &body);
                let response = Response::from_string(text)
                    .with_status_code(StatusCode(status))
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
            });
        }
    });
    TestServer { base: format!("http://127.0.0.1:{}", port), log }
}

/// arm.haglund.dev's bulk answer for the MAL ids it knows.
fn arm_route(body: &str) -> String {
    let asked: Vec<Value> = serde_json::from_str(body).unwrap_or_default();
    let known = [(52991, 154587), (59978, 182255)];
    let answers: Vec<Value> = asked
        .iter()
        .map(|q| {
            let mal = q["myanimelist"].as_i64();
            known
                .iter()
                .find(|(m, _)| Some(*m) == mal)
                .map(|(m, a)| json!({ "anilist": a, "myanimelist": m }))
                .unwrap_or(Value::Null)
        })
        .collect();
    json!(answers).to_string()
}

fn down(status: u16) -> (u16, String) {
    (status, r#"{"error":"down"}"#.to_string())
}

async fn catalogue(server: &TestServer, preference: CatalogueSource) -> Catalogue {
    let pool = db::init_db_in_memory().await.unwrap();
    Catalogue::with_endpoints(reqwest::Client::new(), Some(pool), server.endpoints(), Some(preference))
        .with_timeout(Duration::from_secs(3))
}

fn frieren() -> SearchQuery {
    SearchQuery { query: Some("frieren".into()), page: 1, per_page: 10, hide_adult: true, ..Default::default() }
}

#[tokio::test]
async fn anilist_down_falls_through_to_myanimelist() {
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/anime?") => (200, fixture("mal_search.json")),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;

    let served = catalogue.search(&frieren()).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    assert!(served.fallback);
    assert_eq!(served.notice(), Some("MyAnimeList"));
    let ids: Vec<i32> = served.value.iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![182255, 154587], "AniList ids come from arm.haglund.dev");
    assert_eq!(server.header("/mal/", "X-MAL-CLIENT-ID").as_deref(), Some(anix_lib::commands::mal::MAL_CLIENT_ID));

    // AniList is now paused, and the mapping is cached: no second AniList or arm request.
    let again = catalogue.search(&frieren()).await.unwrap();
    assert_eq!(again.source, Source::Mal);
    assert!(again.fallback);
    assert_eq!(server.hits("/anilist"), 1);
    assert_eq!(server.hits("/arm"), 1);
}

#[tokio::test]
async fn myanimelist_down_falls_through_to_kitsu() {
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/") => down(503),
        ("GET", u) if u.starts_with("/kitsu/anime?") => (200, fixture("kitsu_search.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let served = catalogue.search(&frieren()).await.unwrap();
    assert_eq!(served.source, Source::Kitsu);
    assert_eq!(served.value[0].id, 154587, "from Kitsu's own mappings");
    assert_eq!(served.value[0].id_mal, Some(52991));
    assert_eq!(server.hits("/arm"), 0);
    assert!(catalogue.breaker().is_open("MyAnimeList"));
}

#[tokio::test]
async fn every_catalogue_down_leaves_the_offline_copy() {
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        _ => down(502),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let q = frieren();
    let err = catalogue.search(&q).await.unwrap_err();
    assert!(offline::is_offline_error(&err), "{:?}", err);
    assert!(err.to_string().contains("AniList"), "the first outage is reported: {}", err);
    // Every catalogue was asked once, IMDb last.
    for prefix in ["/anilist", "/mal", "/kitsu", "/shiki", "/comick", "/jikan", "/imdb"] {
        assert_eq!(server.hits(prefix), 1, "{}", prefix);
    }

    // `search_anime` then serves the saved copy of the same search.
    let pool = db::init_db_in_memory().await.unwrap();
    let key = offline::search_key(q.text(), &q.genres, None, None, None, 1, 10, true);
    let saved = json!([{ "id": 154587 }]);
    offline::store(&pool, offline::Kind::Search, &key, &saved).await.unwrap();
    let (value, _) = offline::load(&pool, offline::Kind::Search, &key).await.unwrap().unwrap();
    assert_eq!(value, saved);

    // With everything paused, nothing is asked and the error still means "unreachable".
    let err = catalogue.search(&q).await.unwrap_err();
    assert!(offline::is_offline_error(&err));
    assert_eq!(server.hits("/anilist"), 1);
}

#[tokio::test]
async fn a_slow_catalogue_is_given_up_on() {
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => {
            std::thread::sleep(Duration::from_millis(900));
            (200, r#"{"data":{"Page":{"media":[]}}}"#.to_string())
        }
        ("GET", u) if u.starts_with("/mal/anime?") => (200, fixture("mal_search.json")),
        ("POST", "/arm/ids") => (200, arm_route("[]")),
        _ => down(500),
    });
    let pool = db::init_db_in_memory().await.unwrap();
    let catalogue = Catalogue::with_endpoints(reqwest::Client::new(), Some(pool), server.endpoints(), Some(CatalogueSource::Auto))
        .with_timeout(Duration::from_millis(300));
    let served = catalogue.search(&frieren()).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    assert!(catalogue.breaker().is_open("AniList"), "a timeout pauses AniList");
    // arm answered with the wrong length, so the results keep synthetic MAL ids.
    assert!(served.value.iter().all(|m| ids::decode(m.id as i64) == Some(AnimeKey::Other(Site::Mal, m.id_mal.unwrap() as i64))));
}

#[tokio::test]
async fn preferred_catalogue_is_asked_first_without_a_notice() {
    let server = serve(|method, url, _| match (method, url) {
        ("GET", u) if u.starts_with("/kitsu/anime?") => (200, fixture("kitsu_search.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Kitsu).await;
    let served = catalogue.search(&frieren()).await.unwrap();
    assert_eq!(served.source, Source::Kitsu);
    assert_eq!(served.notice(), None);
    assert_eq!(server.hits("/anilist"), 0);
}

#[tokio::test]
async fn details_of_an_anilist_id_come_from_myanimelist() {
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/arm/ids?") => (200, fixture("arm_ids.json")),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        ("GET", u) if u.starts_with("/mal/anime/52991?") => (200, fixture("mal_details.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let served = catalogue.details(154587).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    let json = served.value;
    assert_eq!(json["id"], 154587, "the requested id is kept");
    assert_eq!(json["idMal"], 52991);
    let relations = json["relations"]["edges"].as_array().unwrap();
    let sequel = relations.iter().find(|r| r["relationType"] == "SEQUEL").unwrap();
    assert_eq!(sequel["node"]["id"], 182255, "related MAL ids are mapped too");
    assert!(relations.iter().any(|r| r["node"]["id"].as_i64().unwrap() < 0), "unmapped ones are synthetic");

    // The id lookup is cached for the next time.
    let _ = catalogue.details(154587).await.unwrap();
    assert_eq!(server.hits("/arm/ids?"), 1);
}

#[tokio::test]
async fn details_of_a_synthetic_id_skip_anilist() {
    let server = serve(|method, url, body| match (method, url) {
        ("GET", u) if u.starts_with("/mal/anime/52991?") => (200, fixture("mal_details.json")),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let id = ids::synthetic_id(Site::Mal, 52991).unwrap();
    let served = catalogue.details(id).await.unwrap();
    assert_eq!(served.value["id"], id);
    assert_eq!(served.value["title"]["romaji"], "Sousou no Frieren");
    assert_eq!(server.hits("/anilist"), 0);
    // Not shown as a fallback: AniList cannot know this anime at all.
    assert!(!served.fallback);

    let hint = catalogue.hint(id).await.unwrap();
    assert_eq!(hint.id_mal, Some(52991));
}

#[tokio::test]
async fn schedule_falls_back_to_shikimori() {
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("POST", "/shiki") => (200, fixture("shiki_ongoing.json")),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let from = model::parse_datetime("2026-10-22T00:00:00Z").unwrap();
    let to = from + schedule::WEEK_S;
    let served = catalogue.schedule(from, to, true).await.unwrap();
    assert_eq!(served.source, Source::Shikimori);
    let one_piece = ids::synthetic_id(Site::Mal, 21).unwrap();
    let entry = served.value.iter().find(|e| e.media_id == one_piece).unwrap();
    assert_eq!(entry.episode, 1181);
    assert_eq!(entry.airing_at, 1_792_937_700);
    assert!(entry.id < 0);
    // The Apothecary Diaries: episode 4 on 2026-10-23, no episode beyond its 12.
    assert!(served.value.iter().any(|e| e.media_title.contains("Apothecary") && e.episode == 4));
    assert!(served.value.windows(2).all(|w| w[0].airing_at <= w[1].airing_at));

    // Release notifications: the episode that aired a week before the next one is listed.
    let airings = catalogue.airings(&[one_piece], 1_792_937_700 - 8 * 86_400, 1_792_937_700 + 3600).await.unwrap();
    let episodes: Vec<i32> = airings.iter().map(|a| a.episode).collect();
    assert_eq!(episodes, vec![1180, 1181]);
}

#[tokio::test]
async fn a_paused_anilist_still_means_offline() {
    // MAL and the others cannot find the anime (404): with AniList paused, the caller must
    // still serve its saved copy instead of an error page.
    let server = serve(|_, _, _| (404, r#"{"error":"not_found"}"#.to_string()));
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    catalogue.breaker().record("AniList", &Err::<(), _>(AppError::Network("AniList HTTP 403".into())));
    let err = catalogue.search(&frieren()).await.unwrap_err();
    assert!(offline::is_offline_error(&err), "{:?}", err);
    // MyAnimeList was reached, so the app is not offline.
    assert!(!anix_lib::catalogue::is_all_down(&err), "{:?}", err);
    assert_eq!(server.hits("/anilist"), 0);
}

#[tokio::test]
async fn slow_id_mapping_does_not_pause_the_catalogue() {
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/anime?") => (200, fixture("mal_search.json")),
        ("POST", "/arm/ids") => {
            std::thread::sleep(Duration::from_millis(600));
            (200, arm_route(body))
        }
        _ => down(500),
    });
    let pool = db::init_db_in_memory().await.unwrap();
    let catalogue = Catalogue::with_endpoints(reqwest::Client::new(), Some(pool), server.endpoints(), Some(CatalogueSource::Auto))
        .with_timeout(Duration::from_millis(300));
    let served = catalogue.search(&frieren()).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    assert!(!catalogue.breaker().is_open("MyAnimeList"));
    assert_eq!(served.value.iter().map(|m| m.id).collect::<Vec<_>>(), vec![182255, 154587]);
}

#[tokio::test]
async fn ani_zip_maps_ids_while_arm_is_down() {
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/anime?") => (200, fixture("mal_search.json")),
        ("GET", "/anizip/mappings?mal_id=52991") => (200, fixture("anizip_mappings.json")),
        ("GET", u) if u.starts_with("/anizip/") => (404, "{}".to_string()),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let served = catalogue.search(&frieren()).await.unwrap();
    assert_eq!(server.hits("/arm"), 1);
    let mapped = served.value.iter().find(|m| m.id_mal == Some(52991)).unwrap();
    assert_eq!(mapped.id, 154587, "from api.ani.zip");
    // arm is paused now: the next search goes straight to ani.zip, and the answer is cached.
    let again = catalogue.search(&frieren()).await.unwrap();
    assert!(again.value.iter().any(|m| m.id == 154587));
    assert_eq!(server.hits("/arm"), 1);
    assert_eq!(server.hits("/anizip/mappings?mal_id=52991"), 1);
}

#[tokio::test]
async fn mal_schedule_reads_last_season_and_the_next() {
    let server = serve(|method, url, _| match (method, url) {
        ("GET", u) if u.starts_with("/mal/anime/season/") => (200, fixture("mal_season.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::MyAnimeList).await;
    let from = model::parse_datetime("2026-09-29T00:00:00Z").unwrap();
    let served = catalogue.schedule(from, from + schedule::WEEK_S, true).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    for season in ["/mal/anime/season/2026/spring?", "/mal/anime/season/2026/summer?", "/mal/anime/season/2026/fall?"] {
        assert_eq!(server.hits(season), 1, "{}", season);
    }
}

// ---------------------------------------------------------------------------- live

/// Asks every real catalogue once: `cargo test --test catalogue_tests -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn live_catalogues_answer() {
    let client = anix_lib::clients::http::http_client().unwrap();
    let urls = Endpoints::default();
    let q = SearchQuery { query: Some("frieren".into()), page: 1, per_page: 5, hide_adult: true, ..Default::default() };
    let browse = SearchQuery::list("POPULARITY_DESC", 5, true);
    let mut working = 0;
    let mut report = |name: &str, result: Result<Vec<Found>, AppError>| match result {
        Ok(list) => {
            println!("{:<10} OK   {} results, first: {:?}", name, list.len(), list.first().map(|f| f.media.title.preferred()));
            if !list.is_empty() {
                working += 1;
            }
        }
        Err(e) => println!("{:<10} FAIL {}", name, e),
    };

    let anilist = anix_lib::clients::anilist::AniListClient::new(client.clone())
        .search(&q)
        .await
        .map(|list| list.into_iter().map(|media| Found { media, ..Default::default() }).collect());
    report("AniList", anilist);
    let mal_id = anix_lib::commands::mal::MAL_CLIENT_ID;
    report("MAL", mal::search(&client, &urls.mal, mal_id, &q).await);
    report("MAL list", mal::search(&client, &urls.mal, mal_id, &browse).await);
    report("Kitsu", kitsu::search(&client, &urls.kitsu, &q).await);
    let filtered = SearchQuery { genres: vec!["Sci-Fi".into()], status: Some("RELEASING".into()), ..browse.clone() };
    report("Kitsu genre", kitsu::search(&client, &urls.kitsu, &filtered).await);
    let action = |page| SearchQuery { genres: vec!["Action".into()], page, per_page: 24, ..browse.clone() };
    report("Kitsu p1", kitsu::search(&client, &urls.kitsu, &action(1)).await);
    report("Kitsu p2", kitsu::search(&client, &urls.kitsu, &action(2)).await);
    report("Shikimori", shikimori::search(&client, &urls.shikimori, &q).await);
    report("Shiki new", shikimori::search(&client, &urls.shikimori, &SearchQuery::list("START_DATE_DESC", 5, true)).await);
    report("Comick", comick::search(&client, &urls.comick, &q).await);
    report("Jikan", tokio::time::timeout(Duration::from_secs(8), jikan::search(&client, &urls.jikan, &q)).await.unwrap_or_else(|_| Err(AppError::Network("timed out".into()))));
    report("IMDb", imdb::search(&client, &urls.imdb, "frieren").await);

    let catalogue = Catalogue::new(client.clone(), Some(db::init_db_in_memory().await.unwrap()));
    match catalogue.search(&q).await {
        Ok(served) => println!("catalogue  OK   via {} (fallback {}), ids {:?}", served.source.label(), served.fallback, served.value.iter().map(|m| m.id).collect::<Vec<_>>()),
        Err(e) => println!("catalogue  FAIL {}", e),
    }
    match catalogue.details(154587).await {
        Ok(served) => println!("details    OK   via {}: {} ({} relations)", served.source.label(), served.value["title"]["romaji"], served.value["relations"]["edges"].as_array().map_or(0, Vec::len)),
        Err(e) => println!("details    FAIL {}", e),
    }
    let now = db::current_unix_timestamp();
    match catalogue.schedule(now, now + schedule::WEEK_S, true).await {
        Ok(served) => println!("schedule   OK   via {}: {} entries", served.source.label(), served.value.len()),
        Err(e) => println!("schedule   FAIL {}", e),
    }
    let by_mal = Catalogue::with_endpoints(client.clone(), None, Endpoints::default(), Some(CatalogueSource::MyAnimeList));
    match by_mal.schedule(now, now + schedule::WEEK_S, true).await {
        Ok(served) => println!("MAL sched  OK   via {}: {} entries", served.source.label(), served.value.len()),
        Err(e) => println!("MAL sched  FAIL {}", e),
    }
    assert!(working > 0, "no catalogue answered");
}

#[test]
fn keeps_applies_exclude_score_year_and_season() {
    use anix_lib::models::Media;
    let media = Media {
        genres: vec!["Drama".into(), "Fantasy".into()],
        average_score: Some(90),
        season: Some("FALL".into()),
        season_year: Some(2023),
        ..Default::default()
    };
    let keeps = |q: SearchQuery| q.keeps(&media);
    assert!(keeps(SearchQuery { exclude_genres: vec!["Mecha".into()], ..Default::default() }));
    assert!(!keeps(SearchQuery { exclude_genres: vec!["fantasy".into()], ..Default::default() }), "any excluded genre drops it");
    assert!(!keeps(SearchQuery { genres: vec!["Drama".into()], exclude_genres: vec!["Fantasy".into()], ..Default::default() }));
    assert!(keeps(SearchQuery { min_score: Some(90), ..Default::default() }), "the minimum itself is kept");
    assert!(!keeps(SearchQuery { min_score: Some(91), ..Default::default() }));
    let unscored = Media { average_score: None, ..media.clone() };
    assert!(!SearchQuery { min_score: Some(10), ..Default::default() }.keeps(&unscored), "no score, no match");
    assert!(keeps(SearchQuery { year: Some(2023), season: Some("FALL".into()), ..Default::default() }));
    assert!(!keeps(SearchQuery { year: Some(2024), ..Default::default() }));
    assert!(!keeps(SearchQuery { season: Some("SPRING".into()), ..Default::default() }));
}

#[test]
fn mal_recommendations_reach_the_details_json() {
    let mut found = mal::parse_details(&fixture_json("mal_details.json")).unwrap();
    found.media.id = 154587;
    let recommended = found.related.iter().filter(|r| r.relation.as_deref() == Some(model::RECOMMENDATION)).count();
    assert_eq!(recommended, 10);
    for (i, r) in found.related.iter_mut().enumerate() {
        r.media.id = -(100_000_000 + i as i32 + 1);
    }
    let json = model::anilist_json(&found);
    let recs = json["recommendations"]["nodes"].as_array().unwrap();
    assert_eq!(recs.len(), 10);
    assert_eq!(recs[0]["mediaRecommendation"]["title"]["romaji"], "Violet Evergarden");
    assert!(recs[0]["mediaRecommendation"]["coverImage"]["extraLarge"].is_string());
    // Recommendations are not relations.
    let relations = json["relations"]["edges"].as_array().unwrap();
    assert_eq!(relations.len(), found.related.len() - 10);
    assert!(relations.iter().all(|r| r["relationType"] != model::RECOMMENDATION));
}

// ---------------------------------------------------------------------------- fuller fallback schedule

/// A MAL seasonal answer: One Piece (also on Shikimori), a 12-episode show only MAL lists, and
/// Doraemon, a long-runner without an episode total.
fn mal_season_with_extras() -> String {
    let node = |id: i64, title: &str, start: &str, day: &str, time: &str, episodes: i64| {
        json!({ "node": {
            "id": id, "title": title, "status": "currently_airing", "media_type": "tv",
            "start_date": start, "num_episodes": episodes,
            "broadcast": { "day_of_the_week": day, "start_time": time },
            "main_picture": { "medium": "https://cdn.example/a.jpg", "large": "https://cdn.example/a.jpg" }
        }})
    };
    json!({ "data": [
        node(21, "One Piece", "1999-10-20", "sunday", "23:15", 0),
        node(60000, "Only On MAL", "2026-10-02", "friday", "23:00", 12),
        node(8687, "Doraemon (2005)", "2005-04-22", "friday", "19:00", 0),
    ]})
    .to_string()
}

#[test]
fn schedule_merge_keeps_the_first_answer_per_mal_id() {
    let slot = |mal: Option<i64>, title: &str, episode: i32| {
        let mut f = Found { ids: ExtIds { mal, ..Default::default() }, ..Default::default() };
        f.media.title.romaji = Some(title.to_string());
        (f, vec![(1_000, episode)])
    };
    let merged = schedule::merge(
        vec![slot(Some(21), "One Piece (Shikimori)", 1181), slot(Some(235), "Conan", 1217)],
        vec![slot(Some(21), "One Piece (MAL)", schedule::UNKNOWN_EPISODE), slot(Some(60000), "Only On MAL", 4), slot(None, "No MAL id", 2)],
    );
    let titles: Vec<String> = merged.iter().map(|(f, _)| f.media.title.romaji.clone().unwrap()).collect();
    assert_eq!(titles, vec!["One Piece (Shikimori)", "Conan", "Only On MAL", "No MAL id"]);
    assert_eq!(merged[0].1, vec![(1_000, 1181)], "the first list's numbers win");
}

#[tokio::test]
async fn fallback_schedule_merges_shikimori_and_myanimelist() {
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("POST", "/shiki") => (200, fixture("shiki_ongoing.json")),
        ("GET", u) if u.starts_with("/mal/anime/season/") => (200, mal_season_with_extras()),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let from = model::parse_datetime("2026-10-22T00:00:00Z").unwrap();
    let served = catalogue.schedule(from, from + schedule::WEEK_S, true).await.unwrap();
    assert_eq!(served.source, Source::Shikimori);
    assert!(served.fallback);
    assert!(server.hits("/mal/anime/season/") >= 2, "MAL was asked to fill the gaps");

    let of = |mal: i64| {
        let id = ids::synthetic_id(Site::Mal, mal).unwrap();
        served.value.iter().filter(|e| e.media_id == id).collect::<Vec<_>>()
    };
    // One Piece is on both: once, with Shikimori's real number.
    let one_piece = of(21);
    assert_eq!(one_piece.len(), 1);
    assert_eq!(one_piece[0].episode, 1181);
    // Shown only by MAL: its counted episode (4th Friday since 2 October).
    let only_mal = of(60000);
    assert_eq!(only_mal.iter().map(|e| e.episode).collect::<Vec<_>>(), vec![4]);
    // A long-runner only MAL lists is kept, without a number.
    let doraemon = of(8687);
    assert_eq!(doraemon.len(), 1);
    assert_eq!(doraemon[0].episode, schedule::UNKNOWN_EPISODE);
    assert!(doraemon[0].id < 0);
    let mut ids_seen: Vec<i64> = served.value.iter().map(|e| e.id).collect();
    ids_seen.sort();
    ids_seen.dedup();
    assert_eq!(ids_seen.len(), served.value.len(), "entry ids are unique");
    assert!(served.value.windows(2).all(|w| w[0].airing_at <= w[1].airing_at));
}

#[tokio::test]
async fn fallback_schedule_survives_one_fallback_failing() {
    // Shikimori down: MAL alone answers (and Shikimori is not asked twice).
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/anime/season/") => (200, mal_season_with_extras()),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let from = model::parse_datetime("2026-10-22T00:00:00Z").unwrap();
    let served = catalogue.schedule(from, from + schedule::WEEK_S, true).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    assert_eq!(server.hits("/shiki"), 1);
    assert!(served.value.iter().any(|e| e.media_title == "Only On MAL"));
}

// ---------------------------------------------------------------------------- filtered pages

fn jikan_comedy() -> String {
    json!({ "data": [{
        "mal_id": 59978, "title": "Sousou no Frieren 2nd Season", "type": "TV", "status": "Currently Airing",
        "genres": [{ "name": "Adventure" }, { "name": "Comedy" }], "themes": [],
        "images": { "jpg": { "image_url": "https://cdn.example/f.jpg" } }
    }]})
    .to_string()
}

fn frieren_comedy() -> SearchQuery {
    SearchQuery { genres: vec!["Comedy".into()], ..frieren() }
}

#[tokio::test]
async fn a_page_the_filter_empties_falls_through_to_jikan() {
    // MAL's text search ignores genres: its page has entries, none with Comedy.
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/anime?") => (200, fixture("mal_search.json")),
        ("GET", u) if u.starts_with("/jikan/anime?") => (200, jikan_comedy()),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let served = catalogue.search(&frieren_comedy()).await.unwrap();
    assert_eq!(served.source, Source::Jikan);
    assert_eq!(served.value.iter().map(|m| m.id).collect::<Vec<_>>(), vec![182255]);
    assert_eq!(server.hits("/jikan/anime?"), 1);
    // Not a failure: MAL stays in use.
    assert!(!catalogue.breaker().is_open("MyAnimeList"));
}

#[tokio::test]
async fn a_later_page_the_filter_empties_ends_the_list() {
    // Page 2 came from MAL's list too; Jikan's page 2 would mix in another list.
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/anime?") => (200, fixture("mal_search.json")),
        ("GET", u) if u.starts_with("/jikan/anime?") => (200, jikan_comedy()),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let served = catalogue.search(&SearchQuery { page: 2, ..frieren_comedy() }).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    assert!(served.value.is_empty());
    assert_eq!(server.hits("/jikan/"), 0);
}

#[tokio::test]
async fn an_empty_page_is_still_nothing_found() {
    // A page that really is empty stops at MAL: Jikan is not asked and there is no error.
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/anime?") => (200, r#"{"data":[]}"#.to_string()),
        ("GET", u) if u.starts_with("/jikan/") => (200, jikan_comedy()),
        _ => down(500),
    });
    let first = catalogue(&server, CatalogueSource::Auto).await;
    let served = first.search(&frieren_comedy()).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    assert!(served.value.is_empty());
    assert_eq!(server.hits("/jikan/"), 0);

    // Every later catalogue failing after a filtered-out page: still the empty state.
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/mal/anime?") => (200, fixture("mal_search.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let served = catalogue.search(&frieren_comedy()).await.unwrap();
    assert!(served.value.is_empty());
    assert_eq!(served.source, Source::Mal);
    assert!(served.fallback);
}

// ---------------------------------------------------------------------------- fuller fallback pages

#[tokio::test]
async fn a_myanimelist_page_gets_characters_banner_and_trailer_from_the_others() {
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/arm/ids?") => (200, fixture("arm_ids.json")),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        ("GET", u) if u.starts_with("/mal/anime/52991?") => (200, fixture("mal_details.json")),
        ("POST", "/shiki") => (200, fixture("shiki_details.json")),
        ("GET", u) if u.starts_with("/kitsu/anime/46474") => (200, fixture("kitsu_details.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let served = catalogue.details(154587).await.unwrap();
    assert_eq!(served.source, Source::Mal);
    let json = served.value;
    assert_eq!(json["servedBy"], "MyAnimeList");
    assert!(json["characters"]["edges"].as_array().unwrap().len() > 0, "characters from Shikimori");
    assert!(json["bannerImage"].is_string(), "banner from Kitsu");
    assert_eq!(json["trailer"]["id"], "qgQunxD0qCk", "trailer from Kitsu");
    assert_eq!(server.hits("/shiki"), 1);
    assert_eq!(server.hits("/kitsu/anime/46474"), 1);
}

#[tokio::test]
async fn a_myanimelist_page_without_helpers_says_where_it_came_from() {
    let server = serve(|method, url, body| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/arm/ids?") => (200, fixture("arm_ids.json")),
        ("POST", "/arm/ids") => (200, arm_route(body)),
        ("GET", u) if u.starts_with("/mal/anime/52991?") => (200, fixture("mal_details.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let json = catalogue.details(154587).await.unwrap().value;
    assert_eq!(json["servedBy"], "MyAnimeList");
    assert!(json["characters"]["edges"].as_array().unwrap().is_empty());
    assert!(json["bannerImage"].is_null());
    // A failed helper does not fail the page, and is paused like any catalogue.
    assert!(catalogue.breaker().is_open("Shikimori"));
}

// ---------------------------------------------------------------------------- open claims, checked
// Answers recorded from the real services on 2026-10-22 (Jikan's from the Internet Archive,
// since Jikan does not answer from the recording network).

#[test]
fn shikimori_long_runner_counts_that_are_behind_lose_their_number() {
    let shiki = fixture_json("shiki_long_runners.json");
    let anilist = fixture_json("anilist_long_runners.json");
    let next_on_anilist = |mal: i64| {
        anilist["media"].as_array().unwrap().iter().find(|m| m["idMal"] == mal).unwrap()["nextAiringEpisode"]["episode"]
            .as_i64()
            .unwrap() as i32
    };
    let found: Vec<Found> = shiki["data"]["animes"].as_array().unwrap().iter().filter_map(shikimori::parse_anime).collect();
    let from = model::parse_datetime("2026-10-22T00:00:00Z").unwrap();
    let slots = schedule::from_shikimori(found, from, from + schedule::WEEK_S);
    let episode = |mal: i64| slots.iter().find(|(f, _)| f.ids.mal == Some(mal)).unwrap().1[0].1;
    // One Piece and Detective Conan: Shikimori's next episode is AniList's.
    assert_eq!(episode(21), next_on_anilist(21));
    assert_eq!(episode(235), next_on_anilist(235));
    // Crayon Shin-chan (1000 vs 1358) and Doraemon (530 vs 938): behind, so no number.
    assert_ne!(next_on_anilist(966), 1000);
    assert_eq!(episode(966), schedule::UNKNOWN_EPISODE);
    assert_eq!(episode(8687), schedule::UNKNOWN_EPISODE);
}

#[test]
fn comick_status_numbers_match_anilist() {
    // 1 and 2 agree with AniList's RELEASING and FINISHED for every recorded title. The two
    // with 4 were RELEASING on AniList the same day; Comick's 4 is its "hiatus" and is kept.
    let list = comick::parse_list(&fixture_json("comick_statuses.json"));
    let status_of = |anilist: i64| list.iter().find(|f| f.ids.anilist == Some(anilist)).unwrap().media.status.clone();
    assert_eq!(status_of(218363).as_deref(), Some("FINISHED"), "Agitato");
    assert_eq!(status_of(218217).as_deref(), Some("RELEASING"), "JK Dark Summoner");
    assert_eq!(status_of(217628).as_deref(), Some("HIATUS"), "Perfect World 7th Season");
}

#[test]
fn anilist_genre_filter_wants_every_genre() {
    // `genre_in: ["Romance", "Mecha"]`: all 25 results have both, so `keeps` matches AniList.
    let json = fixture_json("anilist_genre_in_romance_mecha.json");
    let media = json["data"]["Page"]["media"].as_array().unwrap();
    assert_eq!(media.len(), 25);
    let q = SearchQuery { genres: vec!["Romance".into(), "Mecha".into()], ..Default::default() };
    for m in media {
        let genres: Vec<String> = m["genres"].as_array().unwrap().iter().map(|g| g.as_str().unwrap().to_string()).collect();
        assert!(q.keeps(&anix_lib::models::Media { genres, ..Default::default() }), "{}", m["title"]["romaji"]);
    }
}

#[test]
fn kitsu_filters_by_every_category() {
    // `filter[categories]=romance,mecha` (as the app now sends it): every anime has both.
    let json = fixture_json("kitsu_two_categories.json");
    let slug_of: std::collections::HashMap<&str, &str> = json["included"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c["attributes"]["slug"].as_str().unwrap()))
        .collect();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 20);
    for anime in data {
        let slugs: Vec<&str> = anime["relationships"]["categories"]["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| slug_of[c["id"].as_str().unwrap()])
            .collect();
        assert!(slugs.contains(&"romance") && slugs.contains(&"mecha"), "{}", anime["attributes"]["canonicalTitle"]);
    }
}

#[tokio::test]
async fn kitsu_sends_every_genre() {
    let server = serve(|_, url, _| match url {
        u if u.starts_with("/kitsu/anime?") => (200, r#"{"data":[]}"#.to_string()),
        _ => down(500),
    });
    let q = SearchQuery { genres: vec!["Romance".into(), "Sci-Fi".into()], ..SearchQuery::list("POPULARITY_DESC", 20, true) };
    kitsu::search(&reqwest::Client::new(), &server.endpoints().kitsu, &q).await.unwrap();
    let asked = server.log.lock().unwrap()[0].1.clone();
    assert!(asked.contains("filter%5Bcategories%5D=romance%2Cscience-fiction"), "{}", asked);
}

#[test]
fn jikan_reads_real_answers() {
    let list = jikan::parse_list(&fixture_json("jikan_top_real.json"));
    assert_eq!(list.len(), 5);
    let frieren = &list[0];
    assert_eq!(frieren.ids.mal, Some(52991));
    assert_eq!(frieren.media.title.romaji.as_deref(), Some("Sousou no Frieren"));
    assert_eq!(frieren.media.format.as_deref(), Some("TV"));
    assert_eq!(frieren.media.status.as_deref(), Some("FINISHED"));
    assert_eq!(frieren.media.season.as_deref(), Some("FALL"));
    assert_eq!(frieren.media.duration, Some(24));
    assert!(frieren.media.cover_image.extra_large.as_deref().is_some_and(|u| u.ends_with("l.jpg")));
    // The video id is only in `embed_url` in real answers.
    assert_eq!(frieren.media.trailer.as_ref().and_then(|t| t.id.as_deref()), Some("ZEkwCGJ3o7M"));
    let airing = list.iter().find(|f| f.ids.mal == Some(61316)).unwrap();
    assert_eq!(airing.media.status.as_deref(), Some("RELEASING"));
    // "47 min" (an ONA, no "per ep").
    let ona = list.iter().find(|f| f.ids.mal == Some(61469)).unwrap();
    assert_eq!((ona.media.format.as_deref(), ona.media.duration), (Some("ONA"), Some(47)));

    let details = jikan::parse_anime(&fixture_json("jikan_details_real.json")["data"]).unwrap();
    assert_eq!(details.media.episodes, Some(28));
    assert_eq!(details.studios, vec!["Madhouse"]);
    assert!(details.media.genres.iter().any(|g| g == "Adventure"));
}

// ---------------------------------------------------------------------------- not found and offline

#[tokio::test]
async fn a_catalogue_that_has_no_such_anime_beats_a_paused_anilist() {
    // AniList's API is disabled; the id maps to a MyAnimeList id that MyAnimeList does not have.
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/arm/ids?") => (200, r#"{"anilist":999999,"myanimelist":999998}"#.to_string()),
        ("GET", u) if u.starts_with("/mal/anime/999998?") => (404, r#"{"error":"not_found"}"#.to_string()),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let err = catalogue.details(999999).await.unwrap_err();
    assert!(model::is_not_found(&err), "{:?}", err);
    assert!(!offline::is_offline_error(&err), "a missing anime is not a lost connection: {:?}", err);

    // The same while AniList is already paused.
    let err = catalogue.details(999999).await.unwrap_err();
    assert!(model::is_not_found(&err), "{:?}", err);
}

#[tokio::test]
async fn a_stand_in_id_no_catalogue_knows_is_not_found() {
    // IMDb 99: MyAnimeList and the others cannot look it up without their own id; IMDb has nothing.
    let server = serve(|method, url, _| match (method, url) {
        ("GET", u) if u.starts_with("/imdb/") => (200, r#"{"d":[]}"#.to_string()),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let err = catalogue.details(ids::synthetic_id(Site::Imdb, 99).unwrap()).await.unwrap_err();
    assert!(model::is_not_found(&err), "{:?}", err);
}

#[tokio::test]
async fn an_id_beyond_anilists_range_is_not_found_without_asking() {
    let server = serve(|_, _, _| down(500));
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let err = catalogue.details(3_000_000_000).await.unwrap_err();
    assert!(model::is_not_found(&err), "{:?}", err);
    assert_eq!(server.hits("/anilist"), 0);
    assert!(!catalogue.breaker().is_open("AniList"));
}

#[tokio::test]
async fn offline_only_when_every_catalogue_is_down() {
    let server = serve(|_, _, _| down(500));
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    // A paused AniList counts as down, like the others failing now.
    catalogue.breaker().record("AniList", &Err::<(), _>(AppError::Network("AniList HTTP 403".into())));
    let err = catalogue.search(&frieren()).await.unwrap_err();
    assert!(anix_lib::catalogue::is_all_down(&err), "every catalogue failed: {:?}", err);
    assert!(offline::is_offline_error(&err), "{:?}", err);
    assert!(err.to_string().contains("AniList"), "the first outage is still named: {}", err);
}

#[tokio::test]
async fn details_are_offline_when_every_catalogue_that_knows_the_id_is_down() {
    // The id mapping is known, but AniList and every catalogue fail.
    let server = serve(|method, url, _| match (method, url) {
        ("GET", u) if u.starts_with("/arm/ids?") => (200, fixture("arm_ids.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let err = catalogue.details(154587).await.unwrap_err();
    assert!(anix_lib::catalogue::is_all_down(&err), "{:?}", err);
}

#[tokio::test]
async fn details_nobody_else_can_look_up_are_not_offline_while_anilist_is_down() {
    // AniList's API is disabled and no mapping is known: the others cannot even be asked.
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let err = catalogue.details(154587).await.unwrap_err();
    assert!(offline::is_offline_error(&err), "the saved copy is still served: {:?}", err);
    assert!(!anix_lib::catalogue::is_all_down(&err), "{:?}", err);
}

#[tokio::test]
async fn the_schedule_is_offline_when_its_catalogues_are_down() {
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let err = catalogue.schedule(0, schedule::WEEK_S, true).await.unwrap_err();
    assert!(anix_lib::catalogue::is_all_down(&err), "{:?}", err);
}

#[tokio::test]
async fn a_saved_copy_answers_a_fallbacks_not_found() {
    // AniList's API is disabled and MyAnimeList has no anime under the mapped id (a stale mapping).
    let server = serve(|method, url, _| match (method, url) {
        ("POST", "/anilist") => (403, fixture("anilist_disabled.json")),
        ("GET", u) if u.starts_with("/arm/ids?") => (200, r#"{"anilist":999999,"myanimelist":999998}"#.to_string()),
        ("GET", u) if u.starts_with("/mal/anime/999998?") => (404, r#"{"error":"not_found"}"#.to_string()),
        _ => down(500),
    });
    let catalogue = catalogue(&server, CatalogueSource::Auto).await;
    let err = catalogue.details(999999).await.unwrap_err();
    assert!(model::is_not_found(&err), "{:?}", err);
    assert!(!anix_lib::catalogue::is_all_down(&err), "{:?}", err);

    let pool = db::init_db_in_memory().await.unwrap();
    assert_eq!(offline::saved_details(&pool, 999999, &err).await, None, "nothing saved: not found");
    let saved = json!({ "id": 999999 });
    offline::store(&pool, offline::Kind::Details, "999999", &saved).await.unwrap();
    assert_eq!(offline::saved_details(&pool, 999999, &err).await.map(|(v, _)| v), Some(saved));
    // A rejected request is not answered with saved data.
    let rejected = AppError::Database("locked".into());
    assert_eq!(offline::saved_details(&pool, 999999, &rejected).await, None);
}

#[test]
fn anilist_request_rejections_do_not_pause_anilist() {
    let rejected = [
        interpret_graphql_response(
            400,
            None,
            r#"{"errors":[{"message":"Int cannot represent non 32-bit signed integer value: 3000000000","status":400}],"data":null}"#,
        ),
        interpret_graphql_response(200, None, r#"{"errors":[{"message":"Syntax Error","status":400}],"data":null}"#),
        interpret_graphql_response(422, None, "{}"),
    ];
    for result in rejected {
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::Parse(_)), "{:?}", err);
        assert!(!breaker::trips(&err), "{:?}", err);
        assert!(!offline::is_offline_error(&err), "{:?}", err);
        // A tracker push AniList rejects is still dropped after its attempts, not kept forever.
        assert!(anix_lib::accounts::sync::is_refusal(&err), "{:?}", err);
    }

    let missing = interpret_graphql_response(404, None, r#"{"errors":[{"message":"Not Found.","status":404}]}"#).unwrap_err();
    assert!(model::is_not_found(&missing), "{:?}", missing);
    assert!(!breaker::trips(&missing));

    // Outages still pause it.
    for status in [403, 500, 502, 503] {
        let err = interpret_graphql_response(status, None, r#"{"errors":[{"message":"down","status":500}]}"#).unwrap_err();
        assert!(breaker::trips(&err), "{}: {:?}", status, err);
    }
}
