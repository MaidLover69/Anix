//! One playlist as a shareable `.anix-collection.json` file (Library › Playlists › Export / Import).
//!
//! The file holds only what another user needs to rebuild the list: the name, description,
//! smart-collection rule and, per item, the AniList id, series title, episode and sub/dub.
//! No covers, links, MAL ids, dates or paths go in. A file from someone else is untrusted:
//! a damaged or foreign file is refused whole, and a single bad item is skipped and reported.

use crate::models::{CustomPlaylist, CustomPlaylistItem};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FORMAT: &str = "anix-collection";
/// The only format version this build writes and reads.
pub const VERSION: u64 = 1;
/// The suggested file name ends in this.
pub const FILE_SUFFIX: &str = ".anix-collection.json";
pub const MAX_FILE_BYTES: usize = 512 * 1024;
pub const MAX_ITEMS: usize = 5000;
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_DESCRIPTION_CHARS: usize = 1000;
pub const MAX_TITLE_CHARS: usize = 300;
pub const MAX_RULE_CHARS: usize = 2000;
const MAX_FILE_STEM_CHARS: usize = 80;
const MAX_EPISODE: f64 = 100_000.0;
/// Larger ids are not AniList ids or stand-in ids (`catalogue/ids.rs`).
const MAX_ABS_ID: i64 = 1_000_000_000_000;

/// The file as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionFile {
    pub format: String,
    pub version: u64,
    pub name: String,
    pub description: Option<String>,
    pub rule: Option<String>,
    pub items: Vec<CollectionItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionItem {
    pub anilist_id: i64,
    pub title: String,
    pub episode: f64,
    pub category: String,
}

/// A file that passed the checks: cleaned fields, the good items in file order, and a label
/// (the title, or `#<1-based index>`) for each skipped item.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCollection {
    pub name: String,
    pub description: Option<String>,
    pub rule: Option<String>,
    pub items: Vec<CollectionItem>,
    pub skipped: Vec<String>,
}

/// Why a whole file was refused, as a sentence the UI can show.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn refuse<T>(message: &str) -> Result<T, ParseError> {
    Err(ParseError(message.to_string()))
}

/// Checks a smart-collection rule: at most 2000 characters of a URL query string
/// (`[A-Za-z0-9%&=+._~-]`). An empty or absent rule is `None`.
pub fn validate_rule(rule: Option<&str>) -> Result<Option<String>, String> {
    let Some(rule) = rule.filter(|r| !r.is_empty()) else { return Ok(None) };
    let allowed = |c: char| c.is_ascii_alphanumeric() || "%&=+._~-".contains(c);
    if rule.chars().count() > MAX_RULE_CHARS || !rule.chars().all(allowed) {
        return Err("This smart collection's rule is not valid.".to_string());
    }
    Ok(Some(rule.to_string()))
}

fn strip_control(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// A description keeps its line breaks and tabs; other control characters go.
fn clean_description(text: &str) -> Option<String> {
    let cleaned: String = text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
    Some(cleaned.trim().to_string()).filter(|d| !d.is_empty())
}

/// The file contents for `playlist` and its `items` (already in sort order), pretty UTF-8 JSON.
/// Name and description are cut to what `parse` accepts, so an export always imports again.
pub fn to_json(playlist: &CustomPlaylist, items: &[CustomPlaylistItem]) -> String {
    let name: String = strip_control(&playlist.name).trim().chars().take(MAX_NAME_CHARS).collect();
    let file = CollectionFile {
        format: FORMAT.to_string(),
        version: VERSION,
        name: if name.trim().is_empty() { "Collection".to_string() } else { name.trim().to_string() },
        description: playlist
            .description
            .as_deref()
            .and_then(clean_description)
            .map(|d| d.chars().take(MAX_DESCRIPTION_CHARS).collect::<String>().trim().to_string()),
        rule: validate_rule(playlist.rule.as_deref()).ok().flatten(),
        items: items
            .iter()
            .map(|i| CollectionItem {
                anilist_id: i.anilist_id,
                title: i.series_title.clone(),
                episode: i.episode_num,
                category: i.category.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&file).expect("plain structs always serialise")
}

/// Reads a collection file. See the module doc for what is refused and what is skipped.
pub fn parse(bytes: &[u8]) -> Result<ParsedCollection, ParseError> {
    let not_ours = "This file is not an Anix collection.";
    if bytes.len() > MAX_FILE_BYTES {
        return refuse("This collection file is too large (more than 512 KB).");
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ParseError(not_ours.to_string()))?;
    if value["format"].as_str() != Some(FORMAT) {
        return refuse(not_ours);
    }
    if value["version"].as_u64() != Some(VERSION) {
        return refuse("This collection was made by a different version of Anix and cannot be read.");
    }

    let name = match value["name"].as_str().map(|n| strip_control(n).trim().to_string()) {
        Some(n) if n.is_empty() => return refuse("This collection has no name."),
        Some(n) if n.chars().count() > MAX_NAME_CHARS => {
            return refuse("This collection's name is too long (more than 120 characters).")
        }
        Some(n) => n,
        None => return refuse("This collection has no name."),
    };
    let description = match &value["description"] {
        Value::Null => None,
        Value::String(d) => match clean_description(d) {
            Some(d) if d.chars().count() > MAX_DESCRIPTION_CHARS => {
                return refuse("This collection's description is too long (more than 1000 characters).")
            }
            d => d,
        },
        _ => return refuse("This collection file is damaged."),
    };
    let rule = match &value["rule"] {
        Value::Null => None,
        Value::String(r) => validate_rule(Some(r)).map_err(ParseError)?,
        _ => return refuse("This collection file is damaged."),
    };
    let Some(raw_items) = value["items"].as_array() else {
        return refuse("This collection file is damaged.");
    };
    if raw_items.len() > MAX_ITEMS {
        return refuse("This collection has too many episodes (more than 5000).");
    }

    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for (index, raw) in raw_items.iter().enumerate() {
        match parse_item(raw) {
            Ok(item) => items.push(item),
            Err(title) => skipped.push(title.unwrap_or_else(|| format!("#{}", index + 1))),
        }
    }
    Ok(ParsedCollection { name, description, rule, items, skipped })
}

/// One item, or the label to report it under (`None` when it has no usable title).
fn parse_item(raw: &Value) -> Result<CollectionItem, Option<String>> {
    let title = raw["title"]
        .as_str()
        .map(|t| strip_control(t).trim().to_string())
        .filter(|t| !t.is_empty());
    let label = || title.as_ref().map(|t| t.chars().take(MAX_FILE_STEM_CHARS).collect::<String>());
    let anilist_id = raw["anilist_id"].as_i64().filter(|id| *id != 0 && id.abs() < MAX_ABS_ID);
    let episode = raw["episode"].as_f64().filter(|e| e.is_finite() && (0.0..=MAX_EPISODE).contains(e));
    let category = raw["category"].as_str().filter(|c| *c == "sub" || *c == "dub");
    match (anilist_id, &title, episode, category) {
        (Some(anilist_id), Some(t), Some(episode), Some(category)) if t.chars().count() <= MAX_TITLE_CHARS => {
            Ok(CollectionItem { anilist_id, title: t.clone(), episode, category: category.to_string() })
        }
        _ => Err(label()),
    }
}

/// `wanted`, or `wanted (2)`, `wanted (3)`… — the first not in `existing` (ignoring case).
pub fn unique_name(wanted: &str, existing: &[String]) -> String {
    let taken: std::collections::HashSet<String> = existing.iter().map(|n| n.to_lowercase()).collect();
    if !taken.contains(&wanted.to_lowercase()) {
        return wanted.to_string();
    }
    (2..)
        .map(|n| format!("{} ({})", wanted, n))
        .find(|candidate| !taken.contains(&candidate.to_lowercase()))
        .expect("an unbounded range always finds a free name")
}

/// The suggested export file name: the playlist name without path separators or control
/// characters, at most 80 characters, then `.anix-collection.json`.
pub fn file_name(playlist_name: &str) -> String {
    let stem: String = playlist_name
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .chars()
        .take(MAX_FILE_STEM_CHARS)
        .collect();
    let stem = stem.trim();
    format!("{}{}", if stem.is_empty() { "collection" } else { stem }, FILE_SUFFIX)
}
