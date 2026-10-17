//! Backend pieces of the UI features: desktop button layout and accent colour parsing, the
//! bulk "mark watched" history write, and the AniList statistics parsing.

use anix_lib::commands::desktop::{
    gnome_accent_hex, parse_gnome_button_layout, parse_kdeglobals_accent, parse_kwinrc_button_layout,
    parse_portal_accent, ButtonLayout, WindowButton,
};
use anix_lib::commands::library::highest_whole_episode;
use anix_lib::commands::stats::{parse_stats_entries, tally_voice_actors, voice_actor_candidates};
use anix_lib::db;
use anix_lib::models::HistoryEntry;
use serde_json::json;

use WindowButton::{Close, Maximize, Minimize};

#[test]
fn gnome_layout_splits_sides_and_drops_other_entries() {
    assert_eq!(
        parse_gnome_button_layout("'appmenu:minimize,maximize,close'\n"),
        Some(ButtonLayout { left: vec![], right: vec![Minimize, Maximize, Close] })
    );
    assert_eq!(
        parse_gnome_button_layout("'close,minimize,maximize:'"),
        Some(ButtonLayout { left: vec![Close, Minimize, Maximize], right: vec![] })
    );
    assert_eq!(
        parse_gnome_button_layout("'appmenu:close'"),
        Some(ButtonLayout { left: vec![], right: vec![Close] })
    );
    // No colon: everything is on the left.
    assert_eq!(
        parse_gnome_button_layout("'close'"),
        Some(ButtonLayout { left: vec![Close], right: vec![] })
    );
    assert_eq!(parse_gnome_button_layout(""), None);
}

#[test]
fn kwinrc_layout_reads_the_decoration_group_with_defaults() {
    let rc = "[Compositing]\nButtonsOnLeft=X\n\n[org.kde.kdecoration2]\nButtonsOnLeft=XIA\nButtonsOnRight=HS\n";
    assert_eq!(
        parse_kwinrc_button_layout(rc),
        Some(ButtonLayout { left: vec![Close, Minimize, Maximize], right: vec![] })
    );
    // A missing key is KWin's default.
    assert_eq!(
        parse_kwinrc_button_layout("[org.kde.kdecoration2]\nlibrary=org.kde.breeze\n"),
        Some(ButtonLayout { left: vec![], right: vec![Minimize, Maximize, Close] })
    );
    assert_eq!(parse_kwinrc_button_layout("[Windows]\nFocusPolicy=Click\n"), None);
}

#[test]
fn accent_colours_parse_from_every_source() {
    assert_eq!(
        parse_portal_accent("(<(0.20784313725490197, 0.51764705882352946, 0.89411764705882357)>,)"),
        Some("#3584e4".to_string())
    );
    // Out of range means "no accent".
    assert_eq!(parse_portal_accent("(<(-1.0, -1.0, -1.0)>,)"), None);
    assert_eq!(parse_portal_accent("Error: GDBus.Error"), None);

    assert_eq!(gnome_accent_hex("'purple'"), Some("#9141ac"));
    assert_eq!(gnome_accent_hex("'mauve'"), None);

    let kde = "[Colors:View]\nAccentColor=1,2,3\n[General]\nColorScheme=BreezeDark\nAccentColor=61,174,233\n";
    assert_eq!(parse_kdeglobals_accent(kde), Some("#3daee9".to_string()));
    assert_eq!(parse_kdeglobals_accent("[General]\nColorScheme=Breeze\n"), None);
}

#[test]
fn highest_whole_episode_ignores_specials() {
    assert_eq!(highest_whole_episode(&[1.0, 2.0, 2.5, 3.0]), Some(3.0));
    assert_eq!(highest_whole_episode(&[12.5]), None);
    assert_eq!(highest_whole_episode(&[]), None);
}

fn history(episode: f64, position_ms: i64, duration_ms: i64) -> HistoryEntry {
    HistoryEntry {
        anilist_id: 7,
        title: "Seven".to_string(),
        cover: None,
        episode_number: episode,
        episode_title: Some("Kept".to_string()),
        provider: "auto".to_string(),
        category: "sub".to_string(),
        position_ms,
        duration_ms,
        updated_at: 1,
        from_remote: false,
    }
}

#[tokio::test]
async fn bulk_mark_and_unmark_watched() {
    let pool = db::init_db_in_memory().await.unwrap();
    // Episode 2 was half watched with a known length.
    db::update_watch_progress(&pool, &history(2.0, 600_000, 1_440_000)).await.unwrap();

    let written = db::set_episodes_watched(&pool, 7, "Seven", Some("c.jpg"), &[1.0, 2.0, 3.0], true)
        .await
        .unwrap();
    assert_eq!(written, 3);
    assert_eq!(db::get_watched_episodes(&pool, 7).await.unwrap(), vec![1.0, 2.0, 3.0]);
    // The known length is kept and the position moved to its end; unknown lengths are 1 ms.
    assert_eq!(db::get_episode_progress(&pool, 7, 2.0).await.unwrap(), Some((1_440_000, 1_440_000)));
    assert_eq!(db::get_episode_progress(&pool, 7, 1.0).await.unwrap(), Some((1, 1)));

    let rows = db::get_watched_history(&pool).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.title == "Seven"));

    // Continue Watching treats the anime as finished up to episode 3.
    let cont = db::get_continue_watching(&pool).await.unwrap();
    assert_eq!(cont.len(), 1);
    assert_eq!(cont[0].episode_number, 3.0);
    assert_eq!(cont[0].cover.as_deref(), Some("c.jpg"));

    let unmarked = db::set_episodes_watched(&pool, 7, "Seven", None, &[1.0, 2.0], false).await.unwrap();
    assert_eq!(unmarked, 2);
    assert_eq!(db::get_watched_episodes(&pool, 7).await.unwrap(), vec![3.0]);
    assert_eq!(db::get_episode_progress(&pool, 7, 2.0).await.unwrap(), Some((0, 1_440_000)));

    // Unmarking an episode with no row writes nothing.
    assert_eq!(db::set_episodes_watched(&pool, 7, "Seven", None, &[9.0], false).await.unwrap(), 0);
}

#[test]
fn stats_entries_skip_custom_lists_and_duplicates() {
    let data = json!({
        "MediaListCollection": { "lists": [
            { "isCustomList": false, "entries": [
                { "status": "COMPLETED", "score": 90, "progress": 26, "repeat": 1,
                  "completedAt": { "year": 2026 }, "updatedAt": 100,
                  "media": { "id": 1, "title": { "english": "Cowboy Bebop", "romaji": "Kaubōi Bibappu" },
                             "coverImage": { "large": "a.png" }, "format": "TV", "episodes": 26,
                             "duration": 24, "genres": ["Action", "Sci-Fi"],
                             "studios": { "nodes": [{ "id": 14, "name": "Sunrise" }] } } },
                { "status": "CURRENT", "score": 0, "progress": 3, "completedAt": { "year": null },
                  "updatedAt": 50,
                  "media": { "id": 2, "title": { "userPreferred": "Two" }, "episodes": null,
                             "genres": [], "studios": { "nodes": [] } } }
            ]},
            { "isCustomList": true, "entries": [
                { "status": "COMPLETED", "score": 90, "media": { "id": 1, "title": {} } }
            ]}
        ]}
    });
    let entries = parse_stats_entries(&data);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].title, "Cowboy Bebop");
    assert_eq!(entries[0].studios, vec!["Sunrise".to_string()]);
    assert_eq!(entries[0].completed_year, Some(2026));
    assert_eq!(entries[0].repeat, 1);
    assert_eq!(entries[1].title, "Two");
    assert_eq!(entries[1].episodes, None);
    assert_eq!(entries[1].duration, None);

    assert_eq!(voice_actor_candidates(&entries, 10), vec![1]);
}

#[test]
fn voice_actors_count_once_per_title() {
    let va = |id: i64, name: &str| json!({ "id": id, "name": { "full": name }, "image": { "medium": null } });
    let pages = vec![
        json!({ "Page": { "media": [
            { "id": 1, "characters": { "edges": [
                { "voiceActors": [va(10, "A"), va(11, "B")] },
                { "voiceActors": [va(10, "A")] }
            ]}},
            { "id": 2, "characters": { "edges": [ { "voiceActors": [va(10, "A")] } ]}}
        ]}}),
        // AniList withholds them sometimes.
        json!({ "Page": { "media": [ { "id": 3, "characters": { "edges": [ { "voiceActors": null } ]}} ]}}),
    ];
    let tally = tally_voice_actors(&pages, 10);
    assert_eq!(tally.len(), 2);
    assert_eq!((tally[0].id, tally[0].count), (10, 2));
    assert_eq!((tally[1].id, tally[1].count), (11, 1));
    assert_eq!(tally_voice_actors(&pages, 1).len(), 1);
}
