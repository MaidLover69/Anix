//! Release file name parsing, grouping of scanned files into series, AniList match scoring
//! and the match cache of Library › Local files.

use anix_lib::commands::local_scanner::{parse_episode_and_title, LocalAnimeFile};
use anix_lib::db;
use anix_lib::local_library::{
    self, batched_search_body, build_library, candidates_of_alias, group_series,
    match_from_candidates, rank_candidates, score_candidate, search_terms, series_key,
    title_similarity, CandidateTitle, LibraryFile, MatchCandidate, MatchRequest, MatchStatus,
    AUTO_MATCH_SCORE,
};
use anix_lib::release_parser::{
    parse_file_name, parse_relative_path, parse_relative_paths, ParsedRelease, ReleaseKind,
};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

struct Case {
    name: &'static str,
    title: &'static str,
    episode: Option<f64>,
    season: Option<u32>,
    group: Option<&'static str>,
    kind: ReleaseKind,
}

const fn ep(name: &'static str, title: &'static str, episode: f64, group: Option<&'static str>) -> Case {
    Case { name, title, episode: Some(episode), season: None, group, kind: ReleaseKind::Episode }
}

const fn sep(
    name: &'static str,
    title: &'static str,
    season: u32,
    episode: f64,
    group: Option<&'static str>,
) -> Case {
    Case { name, title, episode: Some(episode), season: Some(season), group, kind: ReleaseKind::Episode }
}

const fn kind(
    name: &'static str,
    title: &'static str,
    episode: Option<f64>,
    group: Option<&'static str>,
    kind: ReleaseKind,
) -> Case {
    Case { name, title, episode, season: None, group, kind }
}

/// Real-world release names from many groups and naming schemes.
fn file_name_cases() -> Vec<Case> {
    use ReleaseKind::*;
    vec![
        ep("[SubsPlease] Sousou no Frieren - 04 (1080p) [9876FEDC].mkv", "Sousou no Frieren", 4.0, Some("SubsPlease")),
        ep("[Erai-raws] Show - 12 [1080p][Multiple Subtitle].mkv", "Show", 12.0, Some("Erai-raws")),
        ep("[Group-1] Show - 07 [720p].mp4", "Show", 7.0, Some("Group-1")),
        sep("Show.S01E03.1080p.WEB.mkv", "Show", 1, 3.0, None),
        ep("Show Episode 10.mp4", "Show", 10.0, None),
        ep("Show EP 2.webm", "Show", 2.0, None),
        ep("Show 08.mkv", "Show", 8.0, None),
        ep("Show - 06v2.mkv", "Show", 6.0, None),
        ep("Show - 13.5.mkv", "Show", 13.5, None),
        ep("[HorribleSubs] Boku no Hero Academia - 88 [1080p].mkv", "Boku no Hero Academia", 88.0, Some("HorribleSubs")),
        sep("[Judas] Shingeki no Kyojin S04E16 [1080p][HEVC x265 10bit][Multi-Subs].mkv", "Shingeki no Kyojin", 4, 16.0, Some("Judas")),
        ep("[ASW] Kimetsu no Yaiba - Katanakaji no Sato-hen - 01 [1080p HEVC][2A5D5E18].mkv", "Kimetsu no Yaiba - Katanakaji no Sato-hen", 1.0, Some("ASW")),
        ep("[Anime Time] Spy x Family - 01 [1080p][HEVC 10bit x265][AAC][Multi Sub].mkv", "Spy x Family", 1.0, Some("Anime Time")),
        sep("Jujutsu.Kaisen.S02E05.1080p.CR.WEB-DL.AAC2.0.H.264-VARYG.mkv", "Jujutsu Kaisen", 2, 5.0, Some("VARYG")),
        ep("[SubsPlease] Oshi no Ko - 11 (1080p) [7D5B0B2C].mkv", "Oshi no Ko", 11.0, Some("SubsPlease")),
        ep("[Commie] Steins;Gate - 01 [BD 720p AAC] [FE1C9E7C].mkv", "Steins;Gate", 1.0, Some("Commie")),
        ep("[TaigaSubs]_Toradora!_(2008)_-_01v2_-_Tiger_and_Dragon_[1280x720_H.264_FLAC][1234ABCD].mkv", "Toradora!", 1.0, Some("TaigaSubs")),
        kind("[Coalgirls]_Clannad_After_Story_OVA_(1280x720_Blu-Ray_FLAC)_[D2C7D29E].mkv", "Clannad After Story", None, Some("Coalgirls"), Ova),
        ep("[gg]_Kami_nomi_zo_Shiru_Sekai_II_-_01_[A1B2C3D4].mkv", "Kami nomi zo Shiru Sekai II", 1.0, Some("gg")),
        ep("[Exiled-Destiny]_Tokyo_Underground_Ep02v2_(41858470).mkv", "Tokyo Underground", 2.0, Some("Exiled-Destiny")),
        sep("Mob Psycho 100 - S03E01.mkv", "Mob Psycho 100", 3, 1.0, None),
        sep("[SubsPlease] Mushoku Tensei S2 - 01 (1080p) [5E3B1C0D].mkv", "Mushoku Tensei", 2, 1.0, Some("SubsPlease")),
        ep("[SubsPlease] Bocchi the Rock! - 12 (1080p) [ABCDEF12].mkv", "Bocchi the Rock!", 12.0, Some("SubsPlease")),
        ep("[Erai-raws] Kaguya-sama wa Kokurasetai - Ultra Romantic - 05 [1080p][Multiple Subtitle][ABCD1234].mkv", "Kaguya-sama wa Kokurasetai - Ultra Romantic", 5.0, Some("Erai-raws")),
        ep("[Nep_Blanc] Re Zero kara Hajimeru Isekai Seikatsu - 01 [1080p] [x265] [10Bit] [Dual Audio] [Subbed].mkv", "Re Zero kara Hajimeru Isekai Seikatsu", 1.0, Some("Nep_Blanc")),
        ep("[DameDesuYo] Made in Abyss - Retsujitsu no Ougonkyou - 01 (1920x1080 10bit AAC) [9DA36D03].mkv", "Made in Abyss - Retsujitsu no Ougonkyou", 1.0, Some("DameDesuYo")),
        ep("[Kametsu] Neon Genesis Evangelion - 26 (BD 1080p Hi10 FLAC) [1A2B3C4D].mkv", "Neon Genesis Evangelion", 26.0, Some("Kametsu")),
        ep("[Cleo] Cowboy Bebop - 05 [Dual Audio 10bit BD1080p][HEVC-x265].mkv", "Cowboy Bebop", 5.0, Some("Cleo")),
        ep("[SubsPlease] One Piece - 1089 (1080p) [AB12CD34].mkv", "One Piece", 1089.0, Some("SubsPlease")),
        kind("[Moozzi2] Violet Evergarden SP - 01 (BD 1920x1080 x.264 Flac).mkv", "Violet Evergarden", Some(1.0), Some("Moozzi2"), Special),
        ep("[SubsPlease] Tensei shitara Slime Datta Ken - 24.5 (1080p) [ABCD1234].mkv", "Tensei shitara Slime Datta Ken", 24.5, Some("SubsPlease")),
        ep("Death.Note.E01.Rebirth.720p.BluRay.x264.mkv", "Death Note", 1.0, None),
        ep("[Beatrice-Raws] Fullmetal Alchemist Brotherhood 01 [BDRip 1920x1080 x264 FLAC].mkv", "Fullmetal Alchemist Brotherhood", 1.0, Some("Beatrice-Raws")),
        ep("[Hi10]_Kino's_Journey_-_The_Beautiful_World_-_01_[BD_1080p][DB21E4F8].mkv", "Kino's Journey - The Beautiful World", 1.0, None),
        kind("[SallySubs] Kono Subarashii Sekai ni Shukufuku wo! 2 - OVA [BD 1080p FLAC] [1234ABCD].mkv", "Kono Subarashii Sekai ni Shukufuku wo! 2", None, Some("SallySubs"), Ova),
        ep("[SubsPlease] Dungeon Meshi - 01v2 (1080p) [ABCD1234].mkv", "Dungeon Meshi", 1.0, Some("SubsPlease")),
        ep("[Okay-Subs] Dandadan - 07 (1080p) [ABCD1234].mkv", "Dandadan", 7.0, Some("Okay-Subs")),
        sep("Cyberpunk Edgerunners S01E10 1080p WEB H264-SKYFiRE.mkv", "Cyberpunk Edgerunners", 1, 10.0, Some("SKYFiRE")),
        ep("[Ohys-Raws] Kusuriya no Hitorigoto - 01 (NTV 1280x720 x264 AAC).mp4", "Kusuriya no Hitorigoto", 1.0, Some("Ohys-Raws")),
        ep("[Golumpa] Fairy Tail - 175 [FuniDub 720p x264 AAC] [2A1B3C4D].mkv", "Fairy Tail", 175.0, Some("Golumpa")),
        ep("[SubsPlease] Kimetsu no Yaiba - Hashira Geiko-hen - 08 (1080p) [ABCD1234].mkv", "Kimetsu no Yaiba - Hashira Geiko-hen", 8.0, Some("SubsPlease")),
        ep("Naruto Shippuden - 001 - Homecoming.mkv", "Naruto Shippuden", 1.0, None),
        ep("[DB]Haikyuu!!_-_01_(Dual Audio_10bit_BD1080p_x265).mkv", "Haikyuu!!", 1.0, Some("DB")),
        ep("[Anime Land] Gintama 2017 - 05 (WEBRip 720p Hi444PP AAC) [ABCD1234].mp4", "Gintama", 5.0, Some("Anime Land")),
        sep("[Yameii] My Hero Academia - S07E03 [English Dub] [CR WEB-DL 1080p] [ABCD1234].mkv", "My Hero Academia", 7, 3.0, Some("Yameii")),
        sep("[ToonsHub] Solo Leveling S01E04 1080p CR WEB-DL AAC2.0 H.264 (Ore dake Level Up na Ken, Multi-Subs).mkv", "Solo Leveling", 1, 4.0, Some("ToonsHub")),
        ep("Kaguya-sama Love is War - 01.mp4", "Kaguya-sama Love is War", 1.0, None),
        ep("Hunter x Hunter (2011) - 148.mkv", "Hunter x Hunter", 148.0, None),
        ep("[Erai-raws] 86 - Eighty Six - 05 [1080p].mkv", "86 - Eighty Six", 5.0, Some("Erai-raws")),
        kind("Monogatari Series - Special 01.mkv", "Monogatari Series", Some(1.0), None, Special),
        sep("Re.Zero.S02E01.1080p.mkv", "Re Zero", 2, 1.0, None),
        ep("[SubsPlease] 22-7 - 01 (1080p) [ABCD1234].mkv", "22-7", 1.0, Some("SubsPlease")),
        sep("Show 2nd Season - 05.mkv", "Show", 2, 5.0, None),
        sep("Kaguya-sama wa Kokurasetai Season 2 - 03.mkv", "Kaguya-sama wa Kokurasetai", 2, 3.0, None),
        sep("[Erai-raws] Tensei shitara Slime Datta Ken 3rd Season - 05 [1080p].mkv", "Tensei shitara Slime Datta Ken", 3, 5.0, Some("Erai-raws")),
        kind("Show - OVA2.mkv", "Show", Some(2.0), None, Ova),
        kind("Show - OVA 01.mkv", "Show", Some(1.0), None, Ova),
        kind("Show - SP01.mkv", "Show", Some(1.0), None, Special),
        kind("Show - Movie.mkv", "Show", None, None, Movie),
        kind("Show - NCED1 [1080p].mkv", "Show", Some(1.0), None, Extra),
        kind("NCOP.mkv", "", None, None, Extra),
        ep("[Group] Show - 05 END [1080p].mkv", "Show", 5.0, Some("Group")),
        ep("Show_-_05_[720p].mkv", "Show", 5.0, None),
        // Anitomy reads "2.5" as the episode; the title's trailing number is moved back.
        ep("[Group] 2.5 Jigen no Ririsa - 05 [1080p].mkv", "2.5 Jigen no Ririsa", 5.0, Some("Group")),
        sep("[SubsPlease] Mushoku Tensei S2 - 13v2 (1080p).mkv", "Mushoku Tensei", 2, 13.0, Some("SubsPlease")),
    ]
}

#[test]
fn parses_real_world_release_names() {
    let cases = file_name_cases();
    assert!(cases.len() >= 40);
    for case in &cases {
        let parsed = parse_file_name(case.name);
        assert_eq!(parsed.title, case.title, "title of {}", case.name);
        assert_eq!(parsed.episode, case.episode, "episode of {}", case.name);
        assert_eq!(parsed.season, case.season, "season of {}", case.name);
        assert_eq!(parsed.release_group.as_deref(), case.group, "group of {}", case.name);
        assert_eq!(parsed.kind, case.kind, "kind of {}", case.name);
    }
}

#[test]
fn parses_release_details() {
    let p = parse_file_name("[SubsPlease] Sousou no Frieren - 04 (1080p) [9876FEDC].mkv");
    assert_eq!(p.resolution.as_deref(), Some("1080p"));
    assert_eq!(p.checksum.as_deref(), Some("9876FEDC"));
    assert_eq!(p.version, None);

    let p = parse_file_name(
        "[TaigaSubs]_Toradora!_(2008)_-_01v2_-_Tiger_and_Dragon_[1280x720_H.264_FLAC][1234ABCD].mkv",
    );
    assert_eq!(p.year, Some(2008));
    assert_eq!(p.version, Some(2));
    assert_eq!(p.episode_title.as_deref(), Some("Tiger and Dragon"));
    assert_eq!(p.resolution.as_deref(), Some("1280x720"));

    let p = parse_file_name("[Group] Show - 01-02 [1080p].mkv");
    assert_eq!((p.episode, p.episode_end), (Some(1.0), Some(2.0)));

    let p = parse_file_name("[Erai-raws] Shingeki no Kyojin - The Final Season Part 2 - 01 [1080p].mkv");
    assert_eq!(p.part, Some(2));
    assert_eq!(p.episode, Some(1.0));
}

#[test]
fn parses_folders_for_title_and_season() {
    let p = parse_relative_path("Attack on Titan/Season 2/Episode 03.mkv");
    assert_eq!((p.title.as_str(), p.season, p.episode), ("Attack on Titan", Some(2), Some(3.0)));

    let p = parse_relative_path("Steins;Gate/Season 1/Steins;Gate - 05.mkv");
    assert_eq!((p.title.as_str(), p.season, p.episode), ("Steins;Gate", Some(1), Some(5.0)));

    let p = parse_relative_path(
        "[Judas] Vinland Saga (Season 2) [1080p][HEVC x265 10bit][Multi-Subs] (Batch)/[Judas] Vinland Saga - S02E05.mkv",
    );
    assert_eq!((p.title.as_str(), p.season, p.episode), ("Vinland Saga", Some(2), Some(5.0)));

    let p = parse_relative_path("Show/Extras/Show - NCOP 01.mkv");
    assert_eq!(p.kind, ReleaseKind::Extra);
    let p = parse_relative_path("Show/Specials/Show - 02.mkv");
    assert_eq!(p.kind, ReleaseKind::Special);
}

#[test]
fn batch_folder_range_does_not_hide_episodes() {
    let results = parse_relative_paths(&[
        "Frieren (01-12) [Batch]/Frieren - 05 [1080p].mkv",
        "Frieren (01-12) [Batch]/Frieren - 06 [1080p].mkv",
    ]);
    assert_eq!(results[0].episode, Some(5.0));
    assert_eq!(results[1].episode, Some(6.0));
    assert_eq!(results[0].title, "Frieren");
}

#[test]
fn scanner_api_uses_the_parser() {
    assert_eq!(
        parse_episode_and_title("[SubsPlease] Sousou no Frieren - 04 (1080p) [9876FEDC].mkv"),
        ("Sousou no Frieren".to_string(), Some(4.0))
    );
    // Extras have no episode, so they are never played as episode 1.
    assert_eq!(parse_episode_and_title("Show - NCED1 [1080p].mkv").1, None);
    assert_eq!(parse_episode_and_title("Ep05.mkv"), ("Ep05".to_string(), Some(5.0)));
    assert_eq!(parse_episode_and_title("Opening.mkv").1, None);
}

// ---------------------------------------------------------------------------------------
// Grouping

fn scanned(path: &str) -> LocalAnimeFile {
    LocalAnimeFile {
        file_path: path.to_string(),
        file_name: Path::new(path).file_name().unwrap().to_string_lossy().to_string(),
        file_size: 100,
        parsed_title: String::new(),
        parsed_episode: None,
    }
}

#[test]
fn groups_files_into_series() {
    let root = Path::new("/lib");
    let files = vec![
        scanned("/lib/Frieren/[SubsPlease] Sousou no Frieren - 01 (1080p) [AAAA1111].mkv"),
        scanned("/lib/Frieren/[SubsPlease] Sousou no Frieren - 02 (1080p) [AAAA2222].mkv"),
        scanned("/lib/Frieren/[SubsPlease] Sousou no Frieren - 02v2 (1080p) [AAAA3333].mkv"),
        scanned("/lib/Frieren/Extras/[SubsPlease] Sousou no Frieren - NCOP (1080p).mkv"),
        scanned("/lib/Mushoku/S1/Mushoku Tensei - 01.mkv"),
        scanned("/lib/Mushoku/S2/[SubsPlease] Mushoku Tensei S2 - 01 (1080p).mkv"),
        scanned("/lib/Mushoku/S2/[SubsPlease] Mushoku Tensei S2 - 02 (1080p).mkv"),
        scanned("/lib/Movies/Kimi no Na wa (2016) [BD 1080p].mkv"),
        scanned("/lib/random/NCOP.mkv"),
        scanned("/lib/Ep05.mkv"),
    ];
    let mut linked = HashMap::new();
    linked.insert("/lib/Mushoku/S1/Mushoku Tensei - 01.mkv".to_string(), 108465);
    let series = build_library(root, files, &linked);

    let titles: Vec<(&str, Option<u32>)> = series.iter().map(|s| (s.title.as_str(), s.season)).collect();
    assert_eq!(
        titles,
        vec![
            ("Kimi no Na wa", None),
            ("Mushoku Tensei", Some(1)),
            ("Mushoku Tensei", Some(2)),
            ("random", None),
            ("Sousou no Frieren", None),
            ("", None),
        ]
    );

    let frieren = &series[4];
    assert_eq!(frieren.episodes, vec![1.0, 2.0]);
    assert_eq!(frieren.extras, 1);
    assert_eq!(frieren.files.len(), 4);
    assert_eq!(frieren.files.last().unwrap().parsed.kind, ReleaseKind::Extra);
    assert_eq!(frieren.folder.as_deref(), Some("/lib/Frieren"));
    assert!(frieren.folder_exclusive);
    assert_eq!(frieren.release_groups, vec!["SubsPlease".to_string()]);

    let s1 = &series[1];
    assert_eq!(s1.folder.as_deref(), Some("/lib/Mushoku/S1"));
    assert!(s1.folder_exclusive);
    assert_eq!(s1.linked_anilist_ids, vec![108465]);
    let s2 = &series[2];
    assert_eq!(s2.episodes, vec![1.0, 2.0]);
    assert_ne!(s1.key, s2.key);

    assert_eq!(series[0].year, Some(2016));
    // A file named only for its type takes the title of its folder; one with no title at all
    // is not matched.
    assert_eq!((series[3].extras, series[3].episodes.len()), (1, 0));
    assert!(!series[5].matchable);
    assert_eq!(series[5].episodes, vec![5.0]);
}

#[test]
fn folder_shared_with_another_series_is_not_exclusive() {
    let files = vec![
        scanned("/lib/Show A - 01.mkv"),
        scanned("/lib/Show A - 02.mkv"),
        scanned("/lib/Show B - 01.mkv"),
    ];
    let series = build_library(Path::new("/lib"), files, &HashMap::new());
    assert_eq!(series.len(), 2);
    assert!(series.iter().all(|s| s.folder.as_deref() == Some("/lib") && !s.folder_exclusive));
}

#[test]
fn series_key_separates_season_kind_and_movies() {
    let base = parse_file_name("[SubsPlease] Mushoku Tensei - 01 (1080p).mkv");
    let same = parse_file_name("Mushoku Tensei S01E02.mkv");
    let s2 = parse_file_name("Mushoku Tensei S2 - 01.mkv");
    let ova = parse_file_name("Mushoku Tensei - OVA 01.mkv");
    assert_eq!(series_key(&base), series_key(&same));
    assert_ne!(series_key(&base), series_key(&s2));
    assert_ne!(series_key(&base), series_key(&ova));
    let empty = ParsedRelease { title: String::new(), ..base.clone() };
    assert_eq!(series_key(&empty), "");
}

#[test]
fn groups_already_parsed_files() {
    let file = |path: &str, title: &str, episode: f64| LibraryFile {
        file_path: path.to_string(),
        file_name: path.rsplit('/').next().unwrap().to_string(),
        file_size: 10,
        parsed: ParsedRelease { title: title.to_string(), episode: Some(episode), ..parse_file_name("x.mkv") },
        linked_anilist_id: None,
    };
    let series = group_series(vec![
        file("/a/Show - 03.mkv", "Show", 3.0),
        file("/a/Show - 01.mkv", "Show", 1.0),
        file("/a/show - 02.mkv", "show", 2.0),
    ]);
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].episodes, vec![1.0, 2.0, 3.0]);
    assert_eq!(series[0].files[0].file_name, "Show - 01.mkv");
    assert_eq!(series[0].total_size, 30);
}

// ---------------------------------------------------------------------------------------
// Scoring

fn candidate(id: i32, romaji: &str, english: Option<&str>, format: &str, year: i32, episodes: Option<i32>) -> MatchCandidate {
    MatchCandidate {
        id,
        title: CandidateTitle {
            romaji: Some(romaji.to_string()),
            english: english.map(str::to_string),
            native: None,
            user_preferred: Some(romaji.to_string()),
        },
        format: Some(format.to_string()),
        status: Some("FINISHED".to_string()),
        season_year: Some(year),
        episodes,
        ..Default::default()
    }
}

fn request(title: &str, season: Option<u32>, kind: ReleaseKind, max_episode: Option<f64>) -> MatchRequest {
    MatchRequest {
        key: format!("{}|{:?}", title, season),
        title: title.to_string(),
        season,
        part: None,
        year: None,
        kind,
        max_episode,
    }
}

#[test]
fn title_similarity_prefers_exact_titles() {
    assert_eq!(title_similarity("Sousou no Frieren", "Sousou no Frieren"), 1.0);
    assert!(title_similarity("Re Zero", "Re:Zero") > 0.95);
    let exact = title_similarity("Oshi no Ko", "Oshi no Ko");
    let other = title_similarity("Oshi no Ko", "Oshi no Ko 2nd Season Recap");
    let unrelated = title_similarity("Oshi no Ko", "Cowboy Bebop");
    assert!(exact > other && other > unrelated, "{} {} {}", exact, other, unrelated);
    assert!(unrelated < 0.3);
}

#[test]
fn english_and_synonym_titles_count() {
    let mut aot = candidate(16498, "Shingeki no Kyojin", Some("Attack on Titan"), "TV", 2013, Some(25));
    aot.synonyms = vec!["AoT".to_string(), "SnK".to_string()];
    let req = request("Attack on Titan", None, ReleaseKind::Episode, Some(25.0));
    assert!(score_candidate(&req, &aot, 0) > 0.95);
    let req = request("SnK", None, ReleaseKind::Episode, Some(3.0));
    assert!(score_candidate(&req, &aot, 0) >= AUTO_MATCH_SCORE);
}

#[test]
fn season_hints_pick_the_right_season() {
    let s1 = candidate(108465, "Mushoku Tensei: Isekai Ittara Honki Dasu", Some("Mushoku Tensei: Jobless Reincarnation"), "TV", 2021, Some(11));
    let s1p2 = candidate(127720, "Mushoku Tensei: Isekai Ittara Honki Dasu Part 2", None, "TV", 2021, Some(12));
    let s2 = candidate(146065, "Mushoku Tensei II: Isekai Ittara Honki Dasu", Some("Mushoku Tensei: Jobless Reincarnation Season 2"), "TV", 2023, Some(12));
    let all = vec![s1.clone(), s2.clone(), s1p2.clone()];

    let first = rank_candidates(&request("Mushoku Tensei", None, ReleaseKind::Episode, Some(11.0)), all.clone());
    assert_eq!(first[0].id, 108465);
    let second = rank_candidates(&request("Mushoku Tensei", Some(2), ReleaseKind::Episode, Some(12.0)), all);
    assert_eq!(second[0].id, 146065);

    let frieren = vec![
        candidate(154587, "Sousou no Frieren", Some("Frieren: Beyond Journey's End"), "TV", 2023, Some(28)),
        candidate(182255, "Sousou no Frieren 2nd Season", None, "TV", 2026, Some(10)),
    ];
    let ranked = rank_candidates(&request("Sousou no Frieren", Some(2), ReleaseKind::Episode, Some(3.0)), frieren.clone());
    assert_eq!(ranked[0].id, 182255);
    let ranked = rank_candidates(&request("Sousou no Frieren", None, ReleaseKind::Episode, Some(28.0)), frieren);
    assert_eq!(ranked[0].id, 154587);
}

#[test]
fn format_and_episode_count_break_ties() {
    let tv = candidate(1, "Show", None, "TV", 2020, Some(12));
    let movie = candidate(2, "Show", None, "MOVIE", 2021, Some(1));
    let ova = candidate(3, "Show", None, "OVA", 2021, Some(2));
    let all = vec![movie.clone(), ova.clone(), tv.clone()];

    assert_eq!(rank_candidates(&request("Show", None, ReleaseKind::Episode, Some(12.0)), all.clone())[0].id, 1);
    assert_eq!(rank_candidates(&request("Show", None, ReleaseKind::Movie, None), all.clone())[0].id, 2);
    assert_eq!(rank_candidates(&request("Show", None, ReleaseKind::Ova, Some(2.0)), all)[0].id, 3);

    // 24 local episodes cannot be a finished 12-episode season.
    let short = candidate(4, "Show", None, "TV", 2020, Some(12));
    let long = candidate(5, "Show", None, "TV", 2020, Some(24));
    let req = request("Show", None, ReleaseKind::Episode, Some(24.0));
    assert!(score_candidate(&req, &long, 1) > score_candidate(&req, &short, 0));
}

#[test]
fn year_hint_separates_remakes() {
    let old = candidate(136, "Hunter x Hunter", None, "TV", 1999, Some(62));
    let new = candidate(11061, "Hunter x Hunter (2011)", None, "TV", 2011, Some(148));
    let mut req = request("Hunter x Hunter", None, ReleaseKind::Episode, Some(10.0));
    req.year = Some(2011);
    assert_eq!(rank_candidates(&req, vec![old, new])[0].id, 11061);
}

#[test]
fn weak_results_are_not_auto_matched() {
    let req = request("Some Obscure Home Video", None, ReleaseKind::Episode, Some(3.0));
    let m = match_from_candidates(&req, vec![candidate(9, "Cowboy Bebop", None, "TV", 1998, Some(26))], 5);
    assert_eq!(m.status, MatchStatus::None);
    assert!(m.media.is_none());
    assert_eq!(m.candidates.len(), 1);

    let req = request("Cowboy Bebop", None, ReleaseKind::Episode, Some(3.0));
    let m = match_from_candidates(&req, vec![candidate(9, "Cowboy Bebop", None, "TV", 1998, Some(26))], 5);
    assert_eq!(m.status, MatchStatus::Auto);
    assert_eq!(m.media.as_ref().map(|c| c.id), Some(9));
}

#[test]
fn search_terms_and_batched_body() {
    assert_eq!(search_terms(&request("Show", None, ReleaseKind::Episode, None)), vec!["Show"]);
    assert_eq!(search_terms(&request("Show", Some(2), ReleaseKind::Episode, None)), vec!["Show", "Show Season 2"]);
    assert_eq!(search_terms(&request("Show", None, ReleaseKind::Movie, None)), vec!["Show", "Show Movie"]);
    assert!(search_terms(&request("  ", None, ReleaseKind::Episode, None)).is_empty());

    let body = batched_search_body(&["A".to_string(), "B".to_string()], true);
    let query = body["query"].as_str().unwrap();
    assert!(query.contains("s0: Page") && query.contains("s1: Page") && !query.contains("s2:"));
    assert!(query.contains("isAdult: false") && query.contains("synonyms"));
    assert_eq!(body["variables"], json!({ "q0": "A", "q1": "B" }));
}

#[test]
fn parses_search_results() {
    let data = json!({
        "s0": { "media": [{
            "id": 154587, "idMal": 52991,
            "title": { "romaji": "Sousou no Frieren", "english": "Frieren: Beyond Journey's End", "native": "葬送のフリーレン", "userPreferred": "Sousou no Frieren" },
            "synonyms": ["Frieren at the Funeral"],
            "format": "TV", "status": "FINISHED", "episodes": 28, "season": "FALL", "seasonYear": 2023,
            "startDate": { "year": 2023 },
            "coverImage": { "large": "l.jpg", "extraLarge": "xl.jpg", "color": "#e4f1ff" },
            "bannerImage": null, "averageScore": 90, "genres": ["Adventure"], "isAdult": false
        }, { "title": {} }] }
    });
    let list = candidates_of_alias(&data, 0);
    assert_eq!(list.len(), 1);
    let c = &list[0];
    assert_eq!((c.id, c.id_mal, c.episodes), (154587, Some(52991), Some(28)));
    assert_eq!(c.cover_image.as_deref(), Some("xl.jpg"));
    assert_eq!(c.synonyms, vec!["Frieren at the Funeral".to_string()]);
    assert!(candidates_of_alias(&data, 1).is_empty());
}

// ---------------------------------------------------------------------------------------
// Cache

#[tokio::test]
async fn match_cache_round_trip() {
    let pool = db::init_db_in_memory().await.expect("database");
    let req = request("Cowboy Bebop", None, ReleaseKind::Episode, Some(3.0));
    let found = match_from_candidates(&req, vec![candidate(1, "Cowboy Bebop", None, "TV", 1998, Some(26))], 1000);
    local_library::save_match(&pool, &found).await.unwrap();

    let loaded = local_library::load_matches(&pool, &[req.key.clone(), "missing".to_string()]).await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[&req.key], found);
    assert!(loaded[&req.key].is_fresh(1000 + 1));
    assert!(!loaded[&req.key].is_fresh(1000 + local_library::AUTO_MATCH_TTL_MS + 1));

    let picked = candidate(2, "Cowboy Bebop: Tengoku no Tobira", None, "MOVIE", 2001, Some(1));
    let confirmed = local_library::confirm_match(&pool, &req.key, picked).await.unwrap();
    assert_eq!(confirmed.status, MatchStatus::Confirmed);
    assert_eq!(confirmed.candidates.len(), 2);
    assert!(confirmed.is_fresh(i64::MAX));
    let loaded = local_library::load_matches(&pool, &[req.key.clone()]).await.unwrap();
    assert_eq!(loaded[&req.key].media.as_ref().map(|c| c.id), Some(2));

    local_library::clear_match(&pool, &req.key).await.unwrap();
    assert!(local_library::load_matches(&pool, &[req.key.clone()]).await.unwrap().is_empty());
}

#[tokio::test]
async fn linked_files_are_reported() {
    let pool = db::init_db_in_memory().await.expect("database");
    let file = scanned("/videos/Show - 01.mkv");
    anix_lib::local_media::save_scan(&pool, 42, None, &[file]).await.unwrap();
    let linked = local_library::linked_files(&pool).await.unwrap();
    assert_eq!(linked.get("/videos/Show - 01.mkv"), Some(&42));
}

#[test]
fn named_seasons_and_subtitled_ovas() {
    // A season known by its subtitle: its synonym "Season 3" is not held against files that
    // name no season.
    let mut ultra = candidate(125367, "Kaguya-sama wa Kokurasetai: Ultra Romantic", None, "TV", 2022, Some(13));
    ultra.synonyms = vec!["Kaguya-sama: Love is War Season 3".to_string()];
    let req = request("Kaguya-sama wa Kokurasetai - Ultra Romantic", None, ReleaseKind::Episode, Some(5.0));
    assert!(score_candidate(&req, &ultra, 0) > 0.95);
    // ...but a title that names a later season is.
    let later = candidate(2, "Oshi no Ko 3rd Season", None, "TV", 2026, Some(11));
    let first = candidate(1, "Oshi no Ko", None, "TV", 2023, Some(11));
    let req = request("Oshi no Ko", None, ReleaseKind::Episode, Some(11.0));
    assert_eq!(rank_candidates(&req, vec![later, first])[0].id, 1);

    // "Title 2: Subtitle" is season 2.
    let ova = candidate(3, "Kono Subarashii Sekai ni Shukufuku wo! 2: Kono Subarashii Geijutsu ni Shukufuku wo!", None, "OVA", 2017, Some(1));
    let tv = candidate(4, "Kono Subarashii Sekai ni Shukufuku wo! 2", None, "TV", 2017, Some(10));
    let parsed = parse_file_name("[SallySubs] Kono Subarashii Sekai ni Shukufuku wo! 2 - OVA [BD 1080p FLAC] [1234ABCD].mkv");
    let req = MatchRequest {
        key: series_key(&parsed),
        title: parsed.title.clone(),
        season: parsed.season,
        part: parsed.part,
        year: parsed.year,
        kind: parsed.kind,
        max_episode: parsed.episode,
    };
    assert_eq!(rank_candidates(&req, vec![tv, ova])[0].id, 3);
}

// ---------------------------------------------------------------------------------------
// Matcher fallback while AniList is down

/// A local server: AniList refuses (403), MyAnimeList answers the search, the id-mapping sites
/// fail. Returns its address and a log of the paths asked.
fn catalogue_server() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use tiny_http::{Header, Response, Server, StatusCode};
    let server = Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", server.server_addr().to_ip().unwrap().port());
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = log.clone();
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let url = request.url().to_string();
            seen.lock().unwrap().push(url.clone());
            let fixtures = format!("{}/tests/fixtures/catalogue", env!("CARGO_MANIFEST_DIR"));
            let (status, body) = if url == "/anilist" {
                (403, std::fs::read_to_string(format!("{}/anilist_disabled.json", fixtures)).unwrap())
            } else if url.starts_with("/mal/anime?") {
                (200, std::fs::read_to_string(format!("{}/mal_search.json", fixtures)).unwrap())
            } else {
                (500, r#"{"error":"down"}"#.to_string())
            };
            let response = Response::from_string(body)
                .with_status_code(StatusCode(status))
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = request.respond(response);
        }
    });
    (base, log)
}

fn test_catalogue(base: &str, pool: sqlx::SqlitePool) -> anix_lib::catalogue::Catalogue {
    use anix_lib::catalogue::{Catalogue, Endpoints};
    let at = |path: &str| format!("{}/{}", base, path);
    let urls = Endpoints {
        anilist: at("anilist"),
        mal: at("mal"),
        kitsu: at("kitsu"),
        shikimori: at("shiki"),
        comick: at("comick"),
        jikan: at("jikan"),
        imdb: at("imdb"),
        arm: at("arm"),
        anizip: at("anizip"),
    };
    Catalogue::with_endpoints(
        reqwest::Client::new(),
        Some(pool),
        urls,
        Some(anix_lib::clients::app_settings::CatalogueSource::Auto),
    )
    .with_timeout(std::time::Duration::from_secs(3))
}

#[tokio::test]
async fn matcher_falls_back_and_honours_the_down_switch() {
    use anix_lib::commands::local_library::search_with_fallback;
    let (base, log) = catalogue_server();
    let client = reqwest::Client::new();
    let terms = vec!["Frieren".to_string()];

    // AniList refuses: the same answer shape comes from MyAnimeList.
    let catalogue = test_catalogue(&base, db::init_db_in_memory().await.unwrap());
    let data = search_with_fallback(&catalogue, &client, &terms).await.unwrap();
    let found = candidates_of_alias(&data, 0);
    assert!(!found.is_empty(), "{}", data);
    assert!(found.iter().any(|c| c.title.romaji.as_deref() == Some("Sousou no Frieren")), "{:?}", found);
    let anilist_hits = || log.lock().unwrap().iter().filter(|u| *u == "/anilist").count();
    assert_eq!(anilist_hits(), 1);
    assert!(catalogue.breaker().is_open("AniList"));

    // With the debug switch naming AniList, a fresh catalogue never asks it.
    std::env::set_var("ANIX_CATALOGUE_DOWN", "AniList");
    let catalogue = test_catalogue(&base, db::init_db_in_memory().await.unwrap());
    let data = search_with_fallback(&catalogue, &client, &terms).await;
    std::env::remove_var("ANIX_CATALOGUE_DOWN");
    assert!(!candidates_of_alias(&data.unwrap(), 0).is_empty());
    assert_eq!(anilist_hits(), 1, "AniList was asked although switched off");
}
