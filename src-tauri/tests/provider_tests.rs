//! Helpers shared by every streaming source: episode numbers, title matching and stream headers.
//! Nothing here touches the network.

use anix_lib::models::{AnimeQuery, Category};
use anix_lib::providers::common::{fmt_ep, json_id, json_number};
use anix_lib::providers::manager::{stream_header_map, ProviderManager, NO_PLUGIN_MESSAGE};
use anix_lib::providers::matching::{
    match_score, normalize_title, pick_best, title_similarity, Candidate, MIN_MATCH_SCORE,
};
use serde_json::json;

fn query(title: &str) -> AnimeQuery {
    AnimeQuery::new(1, Some(1), Some(title))
}

// ---------------------------------------------------------------- basics

#[test]
fn category_conversion() {
    assert_eq!(Category::from("sub"), Category::Sub);
    assert_eq!(Category::from("SUB"), Category::Sub);
    assert_eq!(Category::from("dub"), Category::Dub);
    assert_eq!(Category::from("DUB"), Category::Dub);
    assert_eq!(Category::from("unknown"), Category::Sub);
    assert_eq!(Category::Sub.as_str(), "sub");
    assert_eq!(Category::Dub.as_str(), "dub");
}

#[test]
fn episode_numbers_keep_their_fraction() {
    assert_eq!(fmt_ep(1.0), "1");
    assert_eq!(fmt_ep(12.0), "12");
    assert_eq!(fmt_ep(12.5), "12.5");
    assert_eq!(fmt_ep(1061.25), "1061.25");
    assert_eq!(fmt_ep(0.0), "0");
}

// ---------------------------------------------------------------- un-escaping and extraction

// ---------------------------------------------------------------- title matching

#[test]
fn titles_are_normalised_before_comparison() {
    assert_eq!(normalize_title("Shingeki no Kyojin 2nd Season"), "shingeki no kyojin season 2");
    assert_eq!(normalize_title("Shingeki no Kyojin: Season 2"), "shingeki no kyojin season 2");
    assert_eq!(normalize_title("Jujutsu Kaisen (TV)"), "jujutsu kaisen");
    assert_eq!(title_similarity("Jujutsu Kaisen (TV)", "jujutsu kaisen"), 1.0);
}

#[test]
fn sequels_and_movies_do_not_match_the_series() {
    assert!(title_similarity("One Piece", "One Piece Movie 01") < MIN_MATCH_SCORE);
    assert!(title_similarity("Shingeki no Kyojin Season 2", "Shingeki no Kyojin") < MIN_MATCH_SCORE);
    assert!(title_similarity("Jujutsu Kaisen", "Jujutsu Kaisen 2nd Season") < MIN_MATCH_SCORE);
    assert!(title_similarity("Mob Psycho 100", "Mob Psycho 100 II") < MIN_MATCH_SCORE);
    assert!(title_similarity("Naruto", "Boruto") < MIN_MATCH_SCORE);
}

#[test]
fn best_candidate_wins_over_first_candidate() {
    let q = query("Shingeki no Kyojin Season 2");
    let candidates = vec![
        Candidate::new("s1", vec!["Shingeki no Kyojin".to_string()]),
        Candidate::new("s2", vec!["Shingeki no Kyojin 2nd Season".to_string()]),
        Candidate::new("s3", vec!["Shingeki no Kyojin Season 3".to_string()]),
    ];
    assert_eq!(pick_best(&q, candidates), Some("s2"));

    let only_wrong = vec![Candidate::new("s1", vec!["Shingeki no Kyojin".to_string()])];
    assert_eq!(pick_best(&q, only_wrong), None, "a different season must not be accepted");
    assert_eq!(pick_best::<&str>(&q, Vec::new()), None);
}

#[test]
fn year_and_episode_count_break_ties_between_same_titles() {
    let mut q = query("Hunter x Hunter");
    q.year = Some(2011);
    q.episodes = Some(148);
    let titles = vec!["Hunter x Hunter".to_string()];
    assert!(match_score(&q, &titles, Some(1999), Some(62)) < MIN_MATCH_SCORE);
    assert!(match_score(&q, &titles, Some(2011), Some(148)) >= MIN_MATCH_SCORE);
    // Facts the site does not state are not held against the candidate.
    assert!(match_score(&q, &titles, None, None) >= MIN_MATCH_SCORE);

    let candidates = vec![
        Candidate { item: "1999", titles: titles.clone(), year: Some(1999), episodes: Some(62) },
        Candidate { item: "2011", titles: titles.clone(), year: Some(2011), episodes: Some(148) },
    ];
    assert_eq!(pick_best(&q, candidates), Some("2011"));
}

#[test]
fn alternative_titles_from_the_query_are_used() {
    let mut q = query("Sousou no Frieren");
    let titles = vec!["Frieren: Beyond Journey's End".to_string()];
    assert!(match_score(&q, &titles, None, None) < MIN_MATCH_SCORE);
    q.add_title("Frieren: Beyond Journey's End");
    assert_eq!(match_score(&q, &titles, None, None), 1.0);
}

// ---------------------------------------------------------------- stream headers

#[test]
fn custom_headers_never_duplicate_the_referer() {
    let mut stream = anix_lib::models::StreamItem::new(
        "https://cdn.example/x.m3u8".to_string(),
        "hls".to_string(),
        None,
        None,
        Some("https://site.example/".to_string()),
        true,
    );
    let mut custom = std::collections::HashMap::new();
    custom.insert("Referer".to_string(), "https://embed.example/v/1".to_string());
    custom.insert("X-Token".to_string(), "t".to_string());
    stream.headers = Some(custom);
    stream.origin = Some("https://explicit.example".to_string());

    let headers = stream_header_map(&stream);
    assert_eq!(headers.get_all("referer").iter().count(), 1);
    assert_eq!(headers.get("referer").unwrap(), "https://embed.example/v/1");
    assert_eq!(headers.get("origin").unwrap(), "https://explicit.example");
    assert_eq!(headers.get("x-token").unwrap(), "t");
}

// ---------------------------------------------------------------- reading site data

#[test]
fn ids_and_numbers_are_read_whether_sent_as_text_or_as_numbers() {
    assert_eq!(json_id(&json!(36396)).as_deref(), Some("36396"));
    assert_eq!(json_id(&json!(" abc ")).as_deref(), Some("abc"));
    assert_eq!(json_id(&json!("")), None);
    assert_eq!(json_id(&json!(null)), None);
    assert_eq!(json_number(&json!("12.5")), Some(12.5));
    assert_eq!(json_number(&json!(3)), Some(3.0));
    assert_eq!(json_number(&json!("three")), None);
}

// ---------------------------------------------------------------- without plugins

#[tokio::test]
async fn a_manager_without_plugins_has_nothing_to_offer_and_says_so() {
    let manager = ProviderManager::new();
    assert!(manager.provider_names().is_empty());
    assert!(manager.get_all_episodes(21, None, Some("One Piece".to_string())).await.unwrap().is_empty());
    let error = manager
        .get_sources_for_episode(21, None, 1.0, "sub", Some("One Piece".to_string()), None)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), NO_PLUGIN_MESSAGE);
}
