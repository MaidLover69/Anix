//! Anime release file names ("[Group] Title - 05v2 (1080p) [ABCD1234].mkv") read into title,
//! episode, season and release details. The tokenizing is done by `anitomy-ng`, a port of
//! Anitomy (the parser Taiga uses); this module turns its elements into one typed value and
//! fixes up the few things the library needs that Anitomy leaves to the caller (type words in
//! titles, extras, details that only the folder names carry).

use anitomy_ng::{Element, ElementKind, Options};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What kind of video a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseKind {
    /// A regular episode (or a file with no type at all).
    Episode,
    Movie,
    Ova,
    Ona,
    Special,
    /// Openings, endings, previews and other bonus clips; never an episode.
    Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedRelease {
    /// Series title without type words ("OVA", "Movie"); empty when none was found.
    pub title: String,
    pub episode: Option<f64>,
    /// Last episode of a range ("01-12"); `None` for a single episode.
    pub episode_end: Option<f64>,
    pub season: Option<u32>,
    pub part: Option<u32>,
    pub year: Option<u32>,
    pub kind: ReleaseKind,
    pub release_group: Option<String>,
    pub resolution: Option<String>,
    pub checksum: Option<String>,
    /// "v2" in "05v2".
    pub version: Option<u32>,
    pub episode_title: Option<String>,
}

// Type words Anitomy leaves at the end of the title: "Clannad After Story OVA",
// "Show - Special", "Violet Evergarden SP".
static TRAILING_TYPE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:^|[\s\-_.]+)[(\[]?(?:OVA|OAD|OAV|ONA|SP|Specials?|Movie|Gekijouban|TV)[)\]]?\s*$")
        .expect("trailing type regex")
});
static LEADING_TYPE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(?:Gekijouban|Movie)[\s\-_:]+").expect("leading type regex"));
static TRAILING_DASH: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\s\-_:]+$").expect("trailing dash regex"));
// "Jigen no Ririsa - 05" after Anitomy took "2.5" from "2.5 Jigen no Ririsa - 05" as the episode.
static TITLE_ENDS_WITH_EPISODE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(.*\S)\s+-\s+(\d{1,4}(?:\.\d)?)$").expect("title episode regex")
});
static PART_IN_TITLE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?:part|cour)\s*(\d{1,2})\b").expect("part regex"));
static LEADING_NUMBER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\d{1,4}(?:\.\d+)?").expect("leading number regex"));

const EXTRA_TITLES: [&str; 14] = [
    "op", "ed", "opening", "ending", "ncop", "nced", "preview", "pv", "cm", "menu", "trailer",
    "teaser", "creditless", "promo",
];
const EXTRA_FOLDERS: [&str; 12] = [
    "extras", "extra", "bonus", "nc", "ncop", "nced", "menus", "menu", "cm", "pv", "trailers",
    "featurettes",
];
const SPECIAL_FOLDERS: [&str; 4] = ["specials", "special", "sp", "sps"];
const OVA_FOLDERS: [&str; 3] = ["ova", "ovas", "oad"];

fn number(value: &str) -> Option<f64> {
    LEADING_NUMBER
        .find(value.trim())
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
}

fn small_number(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok()
}

fn kind_of_type(value: &str) -> Option<ReleaseKind> {
    match value.to_ascii_lowercase().as_str() {
        "ova" | "oad" | "oav" => Some(ReleaseKind::Ova),
        "ona" => Some(ReleaseKind::Ona),
        "sp" | "special" | "specials" => Some(ReleaseKind::Special),
        "movie" | "gekijouban" => Some(ReleaseKind::Movie),
        "op" | "ed" | "opening" | "ending" | "ncop" | "nced" | "preview" | "pv" => Some(ReleaseKind::Extra),
        _ => None,
    }
}

fn clean_title(raw: &str) -> String {
    let mut title = raw.trim().to_string();
    // Repeated so "Show - OVA (TV)" loses both words.
    for _ in 0..2 {
        title = TRAILING_TYPE.replace(&title, "").to_string();
        title = TRAILING_DASH.replace(&title, "").to_string();
    }
    title = LEADING_TYPE.replace(&title, "").to_string();
    title.trim().to_string()
}

fn first_value(elements: &[Element], kind: ElementKind) -> Option<&Element> {
    elements.iter().find(|e| e.kind == kind)
}

fn from_elements(elements: &[Element], folders: &[&str]) -> ParsedRelease {
    let value = |kind| first_value(elements, kind).map(|e| e.value.clone());

    let mut raw_title = value(ElementKind::Title).unwrap_or_default();
    let episodes: Vec<&Element> = elements.iter().filter(|e| e.kind == ElementKind::Episode).collect();
    let mut episode = episodes.first().and_then(|e| number(&e.value));
    let mut episode_end = if episodes.len() > 1 {
        episodes.last().and_then(|e| number(&e.value)).filter(|end| Some(*end) > episode)
    } else {
        None
    };

    // An episode found before the title is the start of the title when the title itself ends
    // in " - <number>".
    if let (Some(first), Some(title_el)) = (episodes.first(), first_value(elements, ElementKind::Title)) {
        if episodes.len() == 1 && first.position < title_el.position {
            if let Some(caps) = TITLE_ENDS_WITH_EPISODE.captures(&raw_title) {
                let moved = format!("{} {}", first.value, &caps[1]);
                episode = number(&caps[2]);
                episode_end = None;
                raw_title = moved;
            }
        }
    }

    let mut kind = elements
        .iter()
        .filter(|e| e.kind == ElementKind::Type)
        .filter_map(|e| kind_of_type(&e.value))
        .next()
        .unwrap_or(ReleaseKind::Episode);

    let title = clean_title(&raw_title);
    if EXTRA_TITLES.contains(&title.to_ascii_lowercase().as_str()) {
        kind = ReleaseKind::Extra;
    }
    if kind == ReleaseKind::Episode {
        let folder_names: Vec<String> = folders.iter().map(|f| f.trim().to_ascii_lowercase()).collect();
        if folder_names.iter().any(|f| EXTRA_FOLDERS.contains(&f.as_str())) {
            kind = ReleaseKind::Extra;
        } else if folder_names.last().is_some_and(|f| SPECIAL_FOLDERS.contains(&f.as_str())) {
            kind = ReleaseKind::Special;
        } else if folder_names.last().is_some_and(|f| OVA_FOLDERS.contains(&f.as_str())) {
            kind = ReleaseKind::Ova;
        }
    }

    let part = value(ElementKind::Part)
        .and_then(|p| small_number(&p))
        .or_else(|| {
            PART_IN_TITLE
                .captures(&title)
                .and_then(|c| small_number(&c[1]))
        });

    ParsedRelease {
        title,
        episode,
        episode_end,
        season: value(ElementKind::Season).and_then(|s| small_number(&s)),
        part,
        year: value(ElementKind::Year)
            .and_then(|y| small_number(&y))
            .filter(|y| (1900..=2100).contains(y)),
        kind,
        release_group: value(ElementKind::ReleaseGroup),
        resolution: value(ElementKind::VideoResolution),
        checksum: value(ElementKind::FileChecksum),
        version: value(ElementKind::ReleaseVersion).and_then(|v| small_number(&v)),
        episode_title: value(ElementKind::EpisodeTitle),
    }
}

fn folders_of(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    parts.pop();
    parts
}

/// Parses one file name (no folders).
pub fn parse_file_name(file_name: &str) -> ParsedRelease {
    from_elements(&anitomy_ng::parse(file_name, Options::default()), &[])
}

fn comparable(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn names_same_show(a: &str, b: &str) -> bool {
    let (a, b) = (comparable(a), comparable(b));
    !a.is_empty() && !b.is_empty() && (a.contains(&b) || b.contains(&a))
}

/// Parses a path relative to the scanned folder. The file name is parsed on its own (Anitomy
/// would read the folders into the title); the nearest folders then fill in what it lacks:
/// the title of "Show/Episode 03.mkv", the season of "Show/Season 2/Show - 03.mkv", the year
/// of "Show (2022)/Show - 01.mkv". A folder named after another show ends the search.
pub fn parse_relative_path(path: &str) -> ParsedRelease {
    let folders = folders_of(path);
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let mut parsed = from_elements(&anitomy_ng::parse(file_name, Options::default()), &folders);

    for folder in folders.iter().rev().take(3) {
        let lower = folder.trim().to_ascii_lowercase();
        if EXTRA_FOLDERS.contains(&lower.as_str())
            || SPECIAL_FOLDERS.contains(&lower.as_str())
            || OVA_FOLDERS.contains(&lower.as_str())
        {
            continue;
        }
        let from_folder = from_elements(&anitomy_ng::parse(folder, Options::default()), &[]);
        let same_show = from_folder.title.is_empty()
            || parsed.title.is_empty()
            || names_same_show(&from_folder.title, &parsed.title);
        if !same_show {
            break;
        }
        if parsed.title.is_empty() && !from_folder.title.is_empty() {
            parsed.title = from_folder.title.clone();
            parsed.part = parsed.part.or(from_folder.part);
        }
        parsed.season = parsed.season.or(from_folder.season);
        parsed.year = parsed.year.or(from_folder.year);
        if parsed.release_group.is_none() {
            parsed.release_group = from_folder.release_group;
        }
    }
    parsed
}

/// [`parse_relative_path`] for each of `paths`; result `i` is for `paths[i]`.
pub fn parse_relative_paths(paths: &[&str]) -> Vec<ParsedRelease> {
    paths.iter().map(|p| parse_relative_path(p)).collect()
}

/// The file name of `path` without its extension, for a title when nothing was parsed.
pub fn file_stem(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string())
}
