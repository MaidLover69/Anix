//! Seanime's `$scannerUtils` plugin binding: title normalisation, season/part/year extraction,
//! token comparison and search-query building. Pure functions with no I/O.
//!
//! Ported from Seanime's `goja_bindings/scanner_utils.go`, `scanner/title_normalization.go`,
//! `comparison/filtering.go` (`ExtractSeasonNumber`) and `util/numbers.go` (`IntegerToOrdinal`).
//!
//! Go's `regexp` treats `\d`, `\s` and `\b` as ASCII-only, while the `regex` crate makes them
//! Unicode-aware, so the patterns below spell those classes out (`[0-9]`, `[\t\n\f\r ]`,
//! `(?-u:\b)`). Lengths compared against constants are byte lengths, as in Go.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashSet;

/// Result of [`normalize_title`]. Serialised for JS with Seanime's property names: `original`,
/// `normalized`, `cleanBaseTitle`, `denoisedTitle`, `tokens`, `season`, `part`, `year`,
/// `isMain`, `isExtra`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizedTitle {
    pub original: String,
    pub normalized: String,
    /// Normalized title without roman numerals, format words and a trailing number.
    pub clean_base_title: String,
    /// `clean_base_title` without noise words.
    pub denoised_title: String,
    pub tokens: Vec<String>,
    pub season: i64,
    pub part: i64,
    pub year: i64,
    pub is_main: bool,
    pub is_extra: bool,
}

impl NormalizedTitle {
    pub fn to_json(&self) -> Value {
        json!({
            "original": self.original,
            "normalized": self.normalized,
            "cleanBaseTitle": self.clean_base_title,
            "denoisedTitle": self.denoised_title,
            "tokens": self.tokens,
            "season": self.season,
            "part": self.part,
            "year": self.year,
            "isMain": self.is_main,
            "isExtra": self.is_extra,
        })
    }
}

/// Result of [`build_smart_search_titles`]. `season` and `part` are -1 when none was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartSearchTitles {
    pub titles: Vec<String>,
    pub season: i64,
    pub part: i64,
}

impl SmartSearchTitles {
    pub fn to_json(&self) -> Value {
        json!({ "titles": self.titles, "season": self.season, "part": self.part })
    }
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("scanner_utils pattern")
}

fn string_set(words: &[&'static str]) -> HashSet<&'static str> {
    words.iter().copied().collect()
}

static NOISE_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    string_set(&[
        "the", "a", "an", "of", "to", "in", "for", "on", "with", "at", "by", "from", "as", "is",
        "it", "that", "this", "be", "are", "was", "were", "no", "wa", "wo", "ga", "ni", "de",
        "ka", "mo", "ya", "e", "he", "anime", "ova", "ona", "oad", "tv", "movie", "nc", "nced",
        "ncop", "extras", "ending", "opening", "preview", "special", "specials", "sp", "finale",
        "season", "uncensored", "censored", "bluray",
    ])
});

static FORMAT_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    string_set(&[
        "ova", "ona", "oad", "oav", "sp", "special", "specials", "movie", "film", "tv", "nc",
        "nced", "ncop", "extras", "opening", "ending", "preview", "finale",
    ])
});

/// Roman numerals recognised in titles; `i` and `x` are left out as ambiguous.
fn title_roman_to_number(roman: &str) -> Option<i64> {
    Some(match roman {
        "ii" => 2,
        "iii" => 3,
        "iv" => 4,
        "v" => 5,
        "vi" => 6,
        "vii" => 7,
        "viii" => 8,
        "ix" => 9,
        "xi" => 11,
        "xii" => 12,
        "xiii" => 13,
        _ => return None,
    })
}

/// Roman numerals recognised as season numbers.
fn season_roman_to_number(roman: &str) -> Option<i64> {
    Some(match roman {
        "ii" => 2,
        "iii" => 3,
        "iv" => 4,
        "v" => 5,
        "vi" => 6,
        "vii" => 7,
        "viii" => 8,
        "ix" => 9,
        _ => return None,
    })
}

fn ordinal_word_to_number(word: &str) -> Option<i64> {
    Some(match word {
        "first" => 1,
        "second" => 2,
        "third" => 3,
        "fourth" => 4,
        "fifth" => 5,
        "sixth" => 6,
        "seventh" => 7,
        "eighth" => 8,
        "ninth" => 9,
        "tenth" => 10,
        _ => return None,
    })
}

// title_normalization.go
static SEASON_PATTERN_EXPLICIT: Lazy<Regex> =
    Lazy::new(|| re(r"(?i)(?-u:\b)(?:season|s|series)[\t\n\f\r ]*0*([0-9]+)(?-u:\b)"));
static SEASON_PATTERN_ORDINAL: Lazy<Regex> = Lazy::new(|| {
    re(r"(?i)(?-u:\b)([0-9]+)(?:st|nd|rd|th)[\t\n\f\r ]*(?:part|season|series)(?-u:\b)")
});
static SEASON_PATTERN_SUFFIX: Lazy<Regex> =
    Lazy::new(|| re(r"(?i)(?-u:\b)([0-9]+)[\t\n\f\r ]*(?:期|シーズン)(?-u:\b)"));
static SEASON_PATTERN_WORD_ORDINAL: Lazy<Regex> = Lazy::new(|| {
    re(r"(?i)(?-u:\b)(first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth)[\t\n\f\r ]+(?:season)(?-u:\b)")
});
static PART_PATTERN_EXPLICIT: Lazy<Regex> =
    Lazy::new(|| re(r"(?i)(?-u:\b)(?:part|cour)[\t\n\f\r ]*0*([0-9]+)(?-u:\b)"));
static PART_PATTERN_ORDINAL: Lazy<Regex> =
    Lazy::new(|| re(r"(?i)(?-u:\b)([0-9]+)(?:st|nd|rd|th)[\t\n\f\r ]*(?:part|cour)(?-u:\b)"));
static PART_PATTERN_ROMAN: Lazy<Regex> =
    Lazy::new(|| re(r"(?i)(?-u:\b)(?:part|cour)[\t\n\f\r ]+(i{1,3}|iv|vi?i?i?|ix|x)(?-u:\b)"));
static YEAR_PAREN: Lazy<Regex> = Lazy::new(|| re(r"\(([0-9]{4})\)"));
static YEAR_STANDALONE: Lazy<Regex> =
    Lazy::new(|| re(r"(?-u:\b)(19[0-9]{2}|20[0-9]{2})(?-u:\b)"));
static STANDALONE_NUMBER: Lazy<Regex> = Lazy::new(|| re(r"(?-u:\b)[0-9]+(?-u:\b)$"));

// comparison/filtering.go
static SEASON_EXPLICIT: Lazy<Regex> = Lazy::new(|| re(r"season[\t\n\f\r ]*([0-9]+)"));
static SEASON_FORMAT: Lazy<Regex> =
    Lazy::new(|| re(r"(?-u:\b)s0?([0-9]{1,2})(?:e[0-9]|$|[\t\n\f\r ]|\.)"));
static SEASON_ORDINAL_NUM: Lazy<Regex> =
    Lazy::new(|| re(r"([0-9]+)(?:st|nd|rd|th)[\t\n\f\r ]+season"));
static ROMAN_PATTERN_1: Lazy<Regex> =
    Lazy::new(|| re(r"[\t\n\f\r .](i{1,3}|iv|vi?i?i?|ix|x)(?:[\t\n\f\r ]|$|[:,.]|[''])"));
static ROMAN_PATTERN_2: Lazy<Regex> =
    Lazy::new(|| re(r"[\t\n\f\r .](i{1,3}|iv|vi?i?i?|ix|x)[.\t\n\f\r ]*(?:s[0-9]|e[0-9]|part)"));
static IGNORED_ROMAN_NUM: Lazy<Regex> = Lazy::new(|| {
    re(r"(?-u:\b)((?:act|arc|chapter|saga|hen|part|cour))[\t\n\f\r ._:-]+(i{1,3}|iv|vi?i?i?|ix|x)(?-u:\b)")
});
static SEASON_TRAILING_NUM: Lazy<Regex> =
    Lazy::new(|| re(r"(?:^|[\t\n\f\r ])([0-9]{1,2})[\t\n\f\r ]*$"));
static SEASON_PART_COUR: Lazy<Regex> = Lazy::new(|| {
    re(r"(?:part|cour|specials?|sp|movie|ova|ona|oad)[\t\n\f\r ]*[0-9]{1,2}[\t\n\f\r ]*$")
});
static SEASON_JAPANESE: Lazy<Regex> = Lazy::new(|| re(r"(?:第)?([0-9]+)[\t\n\f\r ]*期"));
static SEASON_WORD_ORDINAL: Lazy<Regex> = Lazy::new(|| {
    re(r"(?i)(?-u:\b)(first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth)[\t\n\f\r ]+season(?-u:\b)")
});

// goja_bindings/scanner_utils.go
static SEARCH_SYNTAX_CHARS: Lazy<Regex> = Lazy::new(|| re(r#"[()\[\]{}|"'~*?\\^!]"#));
static MULTI_SPACE: Lazy<Regex> = Lazy::new(|| re(r"[\t\n\f\r ]{2,}"));

/// `strings.ToLower`: one lower-case character per input character, with no context rules
/// (no final sigma) and U+0130 mapped to plain `i`.
fn go_to_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\u{130}' {
            out.push('i');
        } else {
            out.push(c.to_lowercase().next().unwrap_or(c));
        }
    }
    out
}

/// `strconv.Atoi` on a captured run of ASCII digits; `None` on overflow.
fn atoi(digits: &str) -> Option<i64> {
    digits.parse::<i64>().ok()
}

fn capture<'a>(pattern: &Regex, text: &'a str) -> Option<&'a str> {
    pattern.captures(text).and_then(|c| c.get(1)).map(|m| m.as_str())
}

fn is_separator_char(c: char) -> bool {
    matches!(c, '_' | '.' | '-' | ':' | ';' | ',' | '|')
}

fn is_alphanum_or_space(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == ' '
}

fn is_title_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn collapse_whitespace(s: &str) -> String {
    tokenize(s).join(" ")
}

fn tokenize(s: &str) -> Vec<String> {
    s.split(is_title_space).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// Replaces `old` with `new` where it stands as a whole word. Boundaries are judged on the
/// neighbouring bytes, so any non-ASCII neighbour counts as a boundary.
fn replace_word(s: &str, old: &str, new: &str) -> String {
    if s.is_empty() || old.is_empty() {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut start = 0;
    loop {
        let Some(idx) = s[start..].find(old) else {
            out.push_str(&s[start..]);
            break;
        };
        let abs = start + idx;
        let end = abs + old.len();
        let start_boundary = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
        let end_boundary = end == s.len() || !bytes[end].is_ascii_alphanumeric();
        if start_boundary && end_boundary {
            out.push_str(&s[start..abs]);
            out.push_str(new);
            start = end;
        } else {
            // Every `old` used here starts with an ASCII byte, so abs + 1 is a char boundary.
            out.push_str(&s[start..abs + 1]);
            start = abs + 1;
        }
    }
    out
}

fn normalize_string(s: &str) -> String {
    let mut s = go_to_lower(s);

    s = s.replace('\u{14d}', "ou");
    s = s.replace('\u{16b}', "uu");

    s = s.replace('@', "a");
    s = s.replace('\u{d7}', " x ");
    s = s.replace('\u{a789}', ":");
    s = s.replace('\u{ff0a}', " * ");

    s = replace_word(&s, "the animation", "");
    s = replace_word(&s, "the", "");
    s = replace_word(&s, "episode", "");
    s = replace_word(&s, "oad", "ova");
    s = replace_word(&s, "oav", "ova");
    s = replace_word(&s, "specials", "sp");
    s = replace_word(&s, "special", "sp");
    s = s.replace("(tv)", "");
    s = replace_word(&s, "&", "and");

    s = s.replace("'s", " ");
    s = s.replace("\u{2019}s", " ");
    s = s.replace("`s", " ");

    s = s.replace('\'', "");
    s = s.replace('\u{2019}', "");
    s = s.replace('`', "");
    s = s.replace('"', "");
    s = s.replace('\u{201c}', "");
    s = s.replace('\u{201d}', "");

    // Separators and anything outside [a-z0-9 ] become a space; runs of them yield one space,
    // while literal spaces are kept as they are until the final collapse.
    let mut cleaned = String::with_capacity(s.len());
    let mut prev_was_space = false;
    for c in s.chars() {
        if is_separator_char(c) || !is_alphanum_or_space(c) {
            if !prev_was_space {
                cleaned.push(' ');
                prev_was_space = true;
            }
        } else {
            cleaned.push(c);
            prev_was_space = c == ' ';
        }
    }
    let mut s = cleaned;

    for pattern in [
        &SEASON_PATTERN_EXPLICIT,
        &SEASON_PATTERN_ORDINAL,
        &SEASON_PATTERN_WORD_ORDINAL,
        &SEASON_PATTERN_SUFFIX,
        &PART_PATTERN_EXPLICIT,
        &PART_PATTERN_ORDINAL,
        &PART_PATTERN_ROMAN,
    ] {
        s = pattern.replace_all(&s, " ").into_owned();
    }

    collapse_whitespace(&s)
}

/// Returns `(clean_base_title, denoised_title)` for a normalized title.
fn compute_clean_base_title(normalized: &str) -> (String, String) {
    let s = STANDALONE_NUMBER.replace_all(normalized, " ");
    let mut clean = Vec::new();
    let mut denoised = Vec::new();
    for t in s.split_whitespace() {
        if FORMAT_WORDS.contains(t) || title_roman_to_number(t).is_some() {
            continue;
        }
        if !NOISE_WORDS.contains(t) {
            denoised.push(t);
        }
        clean.push(t);
    }
    (clean.join(" "), denoised.join(" "))
}

fn extract_word_ordinal_season(val: &str) -> i64 {
    capture(&SEASON_PATTERN_WORD_ORDINAL, val)
        .and_then(|word| ordinal_word_to_number(&go_to_lower(word)))
        .unwrap_or(-1)
}

/// Normalizes a title for matching and extracts its season, part and year (-1 when absent).
/// An empty title yields the all-default value, whose season, part and year are 0.
pub fn normalize_title(title: &str) -> NormalizedTitle {
    if title.is_empty() {
        return NormalizedTitle::default();
    }

    let mut season = extract_season_number(title);
    if season == -1 {
        season = extract_word_ordinal_season(title);
    }
    let normalized = normalize_string(title);
    let (clean_base_title, denoised_title) = compute_clean_base_title(&normalized);
    let tokens = tokenize(&normalized);

    NormalizedTitle {
        original: title.to_string(),
        normalized,
        clean_base_title,
        denoised_title,
        tokens,
        season,
        part: extract_part_number(title),
        year: extract_year(title),
        is_main: false,
        is_extra: false,
    }
}

/// Part number from "Part 2", "Cour 2", "2nd Part", "Part II"; -1 when absent.
pub fn extract_part_number(val: &str) -> i64 {
    let val = go_to_lower(val);

    if let Some(num) = capture(&PART_PATTERN_EXPLICIT, &val).and_then(atoi) {
        return num;
    }
    if let Some(num) = capture(&PART_PATTERN_ORDINAL, &val).and_then(atoi) {
        return num;
    }
    if let Some(roman) = capture(&PART_PATTERN_ROMAN, &val) {
        let roman = go_to_lower(roman);
        if roman == "i" || roman == "x" {
            return -1;
        }
        if let Some(num) = title_roman_to_number(&roman) {
            return num;
        }
    }
    -1
}

/// Season number from "Season X", "SXX", "Xnd Season", roman numerals, a trailing number
/// (2-10), "X期" or a written-out ordinal; -1 when absent.
pub fn extract_season_number(val: &str) -> i64 {
    let val = go_to_lower(val);

    if let Some(season) = capture(&SEASON_EXPLICIT, &val).and_then(atoi) {
        return season;
    }

    if let Some(season) = capture(&SEASON_FORMAT, &val).and_then(atoi) {
        if season > 0 && season < 20 {
            return season;
        }
    }

    if let Some(season) = capture(&SEASON_ORDINAL_NUM, &val).and_then(atoi) {
        return season;
    }

    // Numerals that follow "act", "arc", "part", "cour" and the like are not seasons.
    let roman_val = IGNORED_ROMAN_NUM.replace_all(&val, "${1}");
    for pattern in [&ROMAN_PATTERN_1, &ROMAN_PATTERN_2] {
        if let Some(num) = capture(pattern, &roman_val).and_then(season_roman_to_number) {
            return num;
        }
    }

    if let Some(digits) = capture(&SEASON_TRAILING_NUM, &val) {
        if !SEASON_PART_COUR.is_match(&val) {
            if let Some(season) = atoi(digits) {
                if (2..=10).contains(&season) {
                    return season;
                }
            }
        }
    }

    if let Some(season) = capture(&SEASON_JAPANESE, &val).and_then(atoi) {
        return season;
    }

    if let Some(num) = capture(&SEASON_WORD_ORDINAL, &val)
        .and_then(|word| ordinal_word_to_number(&go_to_lower(word)))
    {
        return num;
    }

    -1
}

/// Year from "(2024)" (1900-2100) or a standalone 19xx/20xx; -1 when absent.
pub fn extract_year(val: &str) -> i64 {
    if let Some(year) = capture(&YEAR_PAREN, val).and_then(atoi) {
        if (1900..=2100).contains(&year) {
            return year;
        }
    }
    if let Some(year) = capture(&YEAR_STANDALONE, val).and_then(atoi) {
        return year;
    }
    -1
}

/// Tokens that are not noise words and are longer than one byte.
pub fn get_significant_tokens<S: AsRef<str>>(tokens: &[S]) -> Vec<String> {
    tokens
        .iter()
        .map(AsRef::as_ref)
        .filter(|t| !NOISE_WORDS.contains(*t) && t.len() > 1)
        .map(str::to_string)
        .collect()
}

fn is_noise_word(word: &str) -> bool {
    NOISE_WORDS.contains(go_to_lower(word).as_str())
}

fn is_year_token(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 4
        && (token.starts_with("19") || token.starts_with("20"))
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
}

/// Share of the weight of `tokens_a` found in `tokens_b`; noise words weigh 0.3 and years 0.5.
pub fn weighted_token_match_ratio<A: AsRef<str>, B: AsRef<str>>(
    tokens_a: &[A],
    tokens_b: &[B],
) -> f64 {
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }
    let set_b: HashSet<&str> = tokens_b.iter().map(AsRef::as_ref).collect();

    let mut total_weight = 0.0;
    let mut matched_weight = 0.0;
    for t in tokens_a.iter().map(AsRef::as_ref) {
        let weight = if is_noise_word(t) {
            0.3
        } else if is_year_token(t) {
            0.5
        } else {
            1.0
        };
        total_weight += weight;
        if set_b.contains(t) {
            matched_weight += weight;
        }
    }

    if total_weight == 0.0 {
        return 0.0;
    }
    matched_weight / total_weight
}

/// Weighted token match ratio of two titles after normalization.
pub fn compare_titles(a: &str, b: &str) -> f64 {
    weighted_token_match_ratio(&normalize_title(a).tokens, &normalize_title(b).tokens)
}

/// The candidate that matches `target` best; the first one wins a tie, and the result is
/// empty when there are no candidates.
pub fn find_best_match<S: AsRef<str>>(target: &str, candidates: &[S]) -> String {
    let target_tokens = normalize_title(target).tokens;
    let mut best_match = "";
    let mut best_score = -1.0;
    for cand in candidates.iter().map(AsRef::as_ref) {
        let score = weighted_token_match_ratio(&target_tokens, &normalize_title(cand).tokens);
        if score > best_score {
            best_score = score;
            best_match = cand;
        }
    }
    best_match.to_string()
}

/// Significant tokens of a title after normalization.
pub fn significant_tokens_of_title(title: &str) -> Vec<String> {
    get_significant_tokens(&normalize_title(title).tokens)
}

/// Removes characters that carry meaning in search-engine syntax.
pub fn sanitize_query(s: &str) -> String {
    let s = SEARCH_SYNTAX_CHARS.replace_all(s, " ");
    let s = MULTI_SPACE.replace_all(&s, " ");
    s.trim().to_string()
}

/// A compact query for a title: its denoised form, falling back to the clean base title and
/// then the normalized title.
pub fn build_search_query(title: &str) -> String {
    let norm = normalize_title(title);
    let query = if !norm.denoised_title.is_empty() {
        &norm.denoised_title
    } else if !norm.clean_base_title.is_empty() {
        &norm.clean_base_title
    } else {
        &norm.normalized
    };
    sanitize_query(query)
}

/// Groups the distinct queries of several titles as `(a | b | ...)`; a single one is returned
/// bare and none gives an empty string.
pub fn build_advanced_query<S: AsRef<str>>(titles: &[S]) -> String {
    let mut seen = HashSet::new();
    let mut parts = Vec::new();
    for t in titles.iter().map(AsRef::as_ref) {
        if t.is_empty() {
            continue;
        }
        let q = build_search_query(t);
        if q.is_empty() || !seen.insert(q.clone()) {
            continue;
        }
        parts.push(q);
    }
    match parts.len() {
        0 => String::new(),
        1 => parts.remove(0),
        _ => format!("({})", parts.join(" | ")),
    }
}

/// `1st`, `2nd`, `3rd`, `4th`, `11th`... The suffix is derived through a float, as in Go.
pub fn integer_to_ordinal(number: i64) -> String {
    let abs = float_to_int((number as f64).abs());
    let last_two = abs % 100;
    let suffix = if (11..=13).contains(&last_two) {
        "th"
    } else {
        match abs % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{number}{suffix}")
}

/// The base query followed by the season in several notations; season 1 or less gives the
/// base query alone.
pub fn build_season_query(title: &str, season: i64) -> String {
    let base = build_search_query(title);
    if season <= 1 {
        return base;
    }
    format!(
        "({base} S{season:02} | {base} S{season} | {base} Season {season} | {base} {} Season)",
        integer_to_ordinal(season)
    )
}

/// The base query followed by the part in several notations; part 1 or less gives the base
/// query alone.
pub fn build_part_query(title: &str, part: i64) -> String {
    let base = build_search_query(title);
    if part <= 1 {
        return base;
    }
    let mut variants = vec![format!("{base} Part {part}")];
    let roman = match part {
        2 => Some("II"),
        3 => Some("III"),
        4 => Some("IV"),
        5 => Some("V"),
        6 => Some("VI"),
        7 => Some("VII"),
        8 => Some("VIII"),
        9 => Some("IX"),
        _ => None,
    };
    if let Some(roman) = roman {
        variants.push(format!("{base} Part {roman}"));
    }
    variants.push(format!("{base} {} Cour", integer_to_ordinal(part)));
    format!("({})", variants.join(" | "))
}

/// Search-ready variants of a media's titles (denoised, clean base, and the part before a
/// colon or " - " when it is at least 5 bytes long), deduplicated case-insensitively, with
/// the first season and part found among the titles.
pub fn build_smart_search_titles<S: AsRef<str>>(raw_titles: &[S]) -> SmartSearchTitles {
    let mut season = -1;
    let mut part = -1;
    let mut seen = HashSet::new();
    let mut titles = Vec::new();

    let mut add_title = |t: &str| {
        let t = t.trim();
        if t.is_empty() || !seen.insert(go_to_lower(t)) {
            return;
        }
        titles.push(t.to_string());
    };

    for t in raw_titles.iter().map(AsRef::as_ref) {
        if t.is_empty() {
            continue;
        }

        let s = extract_season_number(t);
        if s > 0 && season <= 0 {
            season = s;
        }
        let p = extract_part_number(t);
        if p > 0 && part <= 0 {
            part = p;
        }

        add_title(&build_search_query(t));

        let norm = normalize_title(t);
        if !norm.clean_base_title.is_empty() {
            add_title(&sanitize_query(&norm.clean_base_title));
        }

        for separator in [":", " - "] {
            if let Some(idx) = t.find(separator).filter(|&idx| idx > 0) {
                let prefix = t[..idx].trim();
                if prefix.len() >= 5 {
                    add_title(&build_search_query(prefix));
                }
            }
        }
    }

    SmartSearchTitles { titles, season, part }
}

/// Go's `int(float64)` as amd64 computes it: truncation toward zero, and the minimum integer
/// for NaN and values outside the 64-bit range.
fn float_to_int(n: f64) -> i64 {
    if n.is_nan() || n >= 9_223_372_036_854_775_808.0 || n < -9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        n as i64
    }
}

/// The binding's `toInt`: numbers are truncated, strings are parsed as base-10 integers.
fn to_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => match n.as_i64() {
            Some(i) => Some(i),
            None => n.as_f64().map(float_to_int),
        },
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn string_arg<'a>(args: &'a [Value], index: usize, label: &str) -> Result<&'a str, String> {
    args.get(index)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} is not a string"))
}

/// The strings of an array argument; elements of other types are skipped.
fn string_array_arg<'a>(
    args: &'a [Value],
    index: usize,
    label: &str,
) -> Result<Vec<&'a str>, String> {
    args.get(index)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .ok_or_else(|| format!("{label} is not an array"))
}

fn int_arg(args: &[Value], index: usize, label: &str) -> Result<i64, String> {
    args.get(index)
        .and_then(to_int)
        .ok_or_else(|| format!("{label} is not a number"))
}

fn require(args: &[Value], function: &str, count: usize) -> Result<(), String> {
    if args.len() >= count {
        return Ok(());
    }
    Err(if count == 1 {
        format!("TypeError: {function} requires at least 1 argument")
    } else {
        format!("TypeError: {function} requires {count} arguments")
    })
}

/// Calls the `$scannerUtils` function named `function` with the JS call's arguments (a JSON
/// array) and returns what the JS function returns.
pub fn call(function: &str, args: &Value) -> Result<Value, String> {
    let args: &[Value] = match args {
        Value::Array(items) => items,
        _ => return Err("arguments must be an array".to_string()),
    };

    match function {
        "normalizeTitle" => {
            require(args, function, 1)?;
            Ok(normalize_title(string_arg(args, 0, "argument")?).to_json())
        }
        "extractPartNumber" => {
            require(args, function, 1)?;
            Ok(json!(extract_part_number(string_arg(args, 0, "argument")?)))
        }
        "extractSeasonNumber" => {
            require(args, function, 1)?;
            Ok(json!(extract_season_number(string_arg(args, 0, "argument")?)))
        }
        "extractYear" => {
            require(args, function, 1)?;
            Ok(json!(extract_year(string_arg(args, 0, "argument")?)))
        }
        "compareTitles" => {
            require(args, function, 2)?;
            let a = string_arg(args, 0, "argument 1")?;
            let b = string_arg(args, 1, "argument 2")?;
            Ok(json!(compare_titles(a, b)))
        }
        "findBestMatch" => {
            require(args, function, 2)?;
            let target = string_arg(args, 0, "argument 1")?;
            let candidates = string_array_arg(args, 1, "argument 2")?;
            Ok(json!(find_best_match(target, &candidates)))
        }
        "getSignificantTokens" => {
            require(args, function, 1)?;
            Ok(json!(significant_tokens_of_title(string_arg(args, 0, "argument")?)))
        }
        "buildSearchQuery" => {
            require(args, function, 1)?;
            Ok(json!(build_search_query(string_arg(args, 0, "argument")?)))
        }
        "buildAdvancedQuery" => {
            require(args, function, 1)?;
            Ok(json!(build_advanced_query(&string_array_arg(args, 0, "argument 1")?)))
        }
        "sanitizeQuery" => {
            require(args, function, 1)?;
            Ok(json!(sanitize_query(string_arg(args, 0, "argument")?)))
        }
        "buildSeasonQuery" => {
            require(args, function, 2)?;
            let title = string_arg(args, 0, "argument 1")?;
            let season = int_arg(args, 1, "argument 2")?;
            Ok(json!(build_season_query(title, season)))
        }
        "buildPartQuery" => {
            require(args, function, 2)?;
            let title = string_arg(args, 0, "argument 1")?;
            let part = int_arg(args, 1, "argument 2")?;
            Ok(json!(build_part_query(title, part)))
        }
        "buildSmartSearchTitles" => {
            require(args, function, 1)?;
            Ok(build_smart_search_titles(&string_array_arg(args, 0, "argument 1")?).to_json())
        }
        other => Err(format!("$scannerUtils.{other} is not a function")),
    }
}
