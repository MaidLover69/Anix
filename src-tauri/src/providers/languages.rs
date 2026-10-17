//! Which languages a provider serves and which ones the user wants.
//!
//! A sub is Japanese audio with subtitles in some language; a dub is audio in that language.
//! Languages are ISO 639-1 codes ("en", "pt"). Plugin manifests may declare
//! `languages: { sub?: string[], dub?: string[] }`; without it the manifest's `lang` stands for
//! the subtitles, and for the dub when the plugin has one.

use crate::models::{Category, SubtitleItem};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Code, English name, then other spellings a site may label a track with (native names,
/// ISO 639-2 codes, common variants). Matched without regard to case.
const LANGUAGES: &[(&str, &str, &[&str])] = &[
    ("en", "English", &["eng", "inglés", "ingles", "anglais", "englisch", "inglese", "inglês"]),
    ("ja", "Japanese", &["jpn", "jap", "日本語", "japonés", "japonais", "japanisch", "giapponese"]),
    ("es", "Spanish", &["spa", "español", "espanol", "castellano", "latino", "espagnol", "spanisch", "spagnolo", "espanhol"]),
    ("pt", "Portuguese", &["por", "português", "portugues", "portugués", "brazilian", "portuguais", "portugiesisch", "portoghese"]),
    ("fr", "French", &["fre", "fra", "français", "francais", "francés", "französisch", "francese", "francês"]),
    ("de", "German", &["ger", "deu", "deutsch", "alemán", "aleman", "allemand", "tedesco", "alemão"]),
    ("it", "Italian", &["ita", "italiano", "italien", "italienisch"]),
    ("ru", "Russian", &["rus", "русский", "ruso", "russe", "russisch", "russo"]),
    ("ar", "Arabic", &["ara", "العربية", "عربي", "árabe", "arabe", "arabisch", "arabo"]),
    ("hi", "Hindi", &["hin", "हिन्दी", "हिंदी"]),
    ("id", "Indonesian", &["ind", "bahasa indonesia", "indonesia"]),
    ("ms", "Malay", &["may", "msa", "bahasa melayu", "melayu"]),
    ("th", "Thai", &["tha", "ไทย"]),
    ("vi", "Vietnamese", &["vie", "tiếng việt", "tieng viet"]),
    ("tr", "Turkish", &["tur", "türkçe", "turkce"]),
    ("pl", "Polish", &["pol", "polski"]),
    ("nl", "Dutch", &["dut", "nld", "nederlands"]),
    ("sv", "Swedish", &["swe", "svenska"]),
    ("no", "Norwegian", &["nor", "nob", "nb", "norsk", "bokmål"]),
    ("da", "Danish", &["dan", "dansk"]),
    ("fi", "Finnish", &["fin", "suomi"]),
    ("cs", "Czech", &["cze", "ces", "čeština", "cestina"]),
    ("sk", "Slovak", &["slo", "slk", "slovenčina"]),
    ("hu", "Hungarian", &["hun", "magyar"]),
    ("ro", "Romanian", &["rum", "ron", "română", "romana"]),
    ("bg", "Bulgarian", &["bul", "български"]),
    ("uk", "Ukrainian", &["ukr", "українська"]),
    ("el", "Greek", &["gre", "ell", "ελληνικά"]),
    ("he", "Hebrew", &["heb", "iw", "עברית"]),
    ("fa", "Persian", &["per", "fas", "farsi", "فارسی"]),
    ("ur", "Urdu", &["urd", "اردو"]),
    ("bn", "Bengali", &["ben", "bangla", "বাংলা"]),
    ("ta", "Tamil", &["tam", "தமிழ்"]),
    ("te", "Telugu", &["tel", "తెలుగు"]),
    ("ml", "Malayalam", &["mal", "മലയാളം"]),
    ("kn", "Kannada", &["kan", "ಕನ್ನಡ"]),
    ("mr", "Marathi", &["mar", "मराठी"]),
    ("zh", "Chinese", &["chi", "zho", "中文", "简体中文", "繁體中文", "mandarin", "chinese (simplified)", "chinese (traditional)"]),
    ("ko", "Korean", &["kor", "한국어"]),
    ("tl", "Filipino", &["fil", "tgl", "tagalog"]),
    ("hr", "Croatian", &["hrv", "hrvatski"]),
    ("sr", "Serbian", &["srp", "српски", "srpski"]),
    ("ca", "Catalan", &["cat", "català"]),
    ("lt", "Lithuanian", &["lit", "lietuvių"]),
    ("lv", "Latvian", &["lav", "latviešu"]),
    ("et", "Estonian", &["est", "eesti"]),
];

/// The English name of a language code, or the code itself when it is not known.
pub fn language_name(code: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(known, _, _)| known.eq_ignore_ascii_case(code))
        .map(|(_, name, _)| name.to_string())
        .unwrap_or_else(|| code.to_string())
}

/// The ISO 639-1 code for a language label: a code ("en", "pt-BR", "eng"), a name in English
/// or in the language itself ("Español"), possibly with a note ("English [CC]",
/// "Portuguese (Brazil)"). `None` when nothing in it names a known language.
pub fn normalize_language(label: &str) -> Option<String> {
    let text = label.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    let lookup = |candidate: &str| -> Option<String> {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            return None;
        }
        LANGUAGES
            .iter()
            .find(|(code, name, aliases)| {
                *code == candidate || name.eq_ignore_ascii_case(candidate) || aliases.contains(&candidate)
            })
            .map(|(code, _, _)| code.to_string())
    };
    if let Some(code) = lookup(&text) {
        return Some(code);
    }
    // A tag with a region or script: "pt-BR", "zh_Hans", "en-US".
    if let Some(primary) = text.split(['-', '_']).next().filter(|p| (2..=3).contains(&p.len())) {
        if primary.len() < text.len() {
            if let Some(code) = lookup(primary) {
                return Some(code);
            }
        }
    }
    // A name followed by a note: "english (cc)", "spanish - latin america", "español [latino]".
    let head = text
        .split(|c: char| matches!(c, '(' | '[' | '-' | ',' | '/' | '|'))
        .next()
        .unwrap_or_default();
    if head.len() < text.len() {
        if let Some(code) = lookup(head) {
            return Some(code);
        }
    }
    text.split_whitespace().find_map(|word| {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric());
        (word.chars().count() > 3).then(|| lookup(word)).flatten()
    })
}

/// Codes from a list of labels, normalised, without repeats, in the given order.
pub fn normalize_list<'a>(labels: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for label in labels {
        let code = normalize_language(label).unwrap_or_else(|| label.trim().to_ascii_lowercase());
        if !code.is_empty() && !codes.contains(&code) {
            codes.push(code);
        }
    }
    codes
}

/// The languages a provider serves: subtitle languages (with Japanese audio) and dub audio
/// languages.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Languages {
    #[serde(default)]
    pub sub: Vec<String>,
    #[serde(default)]
    pub dub: Vec<String>,
}

impl Languages {
    /// The manifest's `languages` object, normalised. `None` when the manifest has none (or
    /// it is not an object), so the `lang` fallback applies.
    pub fn from_manifest(manifest: &Value) -> Option<Self> {
        let declared = manifest.get("languages")?.as_object()?;
        let list = |key: &str| -> Vec<String> {
            let values = declared.get(key).and_then(Value::as_array);
            normalize_list(values.into_iter().flatten().filter_map(Value::as_str))
        };
        Some(Self {
            sub: list("sub"),
            dub: list("dub"),
        })
    }

    /// What a plugin serves: its declared languages, or else `lang` for the subtitles and,
    /// when the plugin has a dub, for the dub.
    pub fn effective(declared: Option<&Languages>, lang: &str, supports_dub: bool) -> Self {
        if let Some(declared) = declared {
            return declared.clone();
        }
        let lang = normalize_list([lang]);
        Self {
            dub: if supports_dub { lang.clone() } else { Vec::new() },
            sub: lang,
        }
    }

    pub fn for_category(&self, category: &Category) -> &[String] {
        match category {
            Category::Sub => &self.sub,
            Category::Dub => &self.dub,
        }
    }

    /// A site whose only version is dubbed audio, which it serves as its plain ("sub")
    /// version because it has no separate dub.
    pub fn dub_only(&self, supports_dub: bool) -> bool {
        !supports_dub && self.sub.is_empty() && !self.dub.is_empty()
    }
}

/// The user's language choices, from the settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePrefs {
    /// Subtitle languages for subs, most wanted first.
    pub sub: Vec<String>,
    /// Audio languages for dubs, most wanted first.
    pub dub: Vec<String>,
    /// Only plugins in these languages are used when no provider is named.
    pub strict: bool,
    /// The provider picked in settings; its episode lists are kept whatever its languages.
    pub pinned: Option<String>,
}

pub const SUB_LANGUAGES_KEY: &str = "preferred_sub_languages";
pub const DUB_LANGUAGES_KEY: &str = "preferred_dub_languages";
pub const STRICT_LANGUAGES_KEY: &str = "only_preferred_languages";

impl Default for LanguagePrefs {
    fn default() -> Self {
        Self {
            sub: vec!["en".to_string()],
            dub: vec!["en".to_string()],
            strict: true,
            pinned: None,
        }
    }
}

impl LanguagePrefs {
    /// Reads the preferences from the settings object; missing or malformed keys keep their
    /// defaults, and an empty list counts as missing.
    pub fn from_settings(settings: &Value) -> Self {
        let defaults = Self::default();
        let list = |key: &str, fallback: Vec<String>| -> Vec<String> {
            let values = settings.get(key).and_then(Value::as_array);
            let codes = normalize_list(values.into_iter().flatten().filter_map(Value::as_str));
            if codes.is_empty() {
                fallback
            } else {
                codes
            }
        };
        Self {
            sub: list(SUB_LANGUAGES_KEY, defaults.sub),
            dub: list(DUB_LANGUAGES_KEY, defaults.dub),
            strict: settings
                .get(STRICT_LANGUAGES_KEY)
                .and_then(Value::as_bool)
                .unwrap_or(defaults.strict),
            pinned: settings
                .get("preferred_provider")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("auto"))
                .map(String::from),
        }
    }

    pub fn for_category(&self, category: &Category) -> &[String] {
        match category {
            Category::Sub => &self.sub,
            Category::Dub => &self.dub,
        }
    }

    /// How a request for these languages reads to the user: "English subtitles",
    /// "Hindi or English audio".
    pub fn describe(&self, category: &Category) -> String {
        let names: Vec<String> = self.for_category(category).iter().map(|c| language_name(c)).collect();
        let joined = match names.len() {
            0 => "your languages".to_string(),
            1 => names[0].clone(),
            n => format!("{} or {}", names[..n - 1].join(", "), names[n - 1]),
        };
        match category {
            Category::Sub => format!("{} subtitles", joined),
            Category::Dub => format!("{} audio", joined),
        }
    }
}

/// One step of an automatic lookup: the providers that are raced together, and the language
/// they were picked for (`None` for providers outside the preferred languages, and for
/// providers that do not say which languages they serve).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tier {
    pub language: Option<String>,
    pub members: Vec<usize>,
}

/// The order in which an automatic lookup tries providers, given each provider's languages
/// (`None`: not declared, taken as serving every language) and whether it can answer the
/// category at all.
///
/// Each preferred language, most wanted first, gets a tier of the providers serving it that
/// no earlier tier took; providers without declared languages join the first tier. Providers
/// in none of the preferred languages form a last tier only when `strict` is off. An empty
/// result means nothing may be asked.
pub fn plan_tiers(
    languages: &[(Option<Languages>, bool)],
    category: &Category,
    prefs: &LanguagePrefs,
) -> Vec<Tier> {
    let mut taken = vec![false; languages.len()];
    let mut tiers: Vec<Tier> = Vec::new();
    for (rank, wanted) in prefs.for_category(category).iter().enumerate() {
        let mut members = Vec::new();
        for (idx, (langs, usable)) in languages.iter().enumerate() {
            if taken[idx] || !usable {
                continue;
            }
            let matches = match langs {
                None => rank == 0,
                Some(langs) => langs.for_category(category).contains(wanted),
            };
            if matches {
                taken[idx] = true;
                members.push(idx);
            }
        }
        if !members.is_empty() {
            tiers.push(Tier {
                language: Some(wanted.clone()),
                members,
            });
        }
    }
    if prefs.for_category(category).is_empty() {
        // No preference at all: everything usable, in one race.
        let members: Vec<usize> = (0..languages.len()).filter(|&i| languages[i].1).collect();
        return if members.is_empty() { Vec::new() } else { vec![Tier { language: None, members }] };
    }
    if !prefs.strict {
        let others: Vec<usize> = (0..languages.len()).filter(|&i| !taken[i] && languages[i].1).collect();
        if !others.is_empty() {
            tiers.push(Tier {
                language: None,
                members: others,
            });
        }
    }
    tiers
}

/// The language a provider's answer is in: the most wanted one it serves for the category,
/// else the first one it serves.
pub fn served_language(langs: Option<&Languages>, category: &Category, wanted: &[String]) -> Option<String> {
    let served = langs?.for_category(category);
    wanted
        .iter()
        .find(|code| served.contains(code))
        .or_else(|| served.first())
        .cloned()
}

/// Puts subtitle tracks in the wanted languages first (most wanted first, the rest in their
/// order) and makes the first of them the default. Tracks are left as they are when none is
/// in a wanted language.
pub fn prefer_subtitles(tracks: &mut Vec<SubtitleItem>, wanted: &[String]) {
    let rank = |track: &SubtitleItem| -> usize {
        normalize_language(&track.language)
            .and_then(|code| wanted.iter().position(|w| *w == code))
            .unwrap_or(wanted.len())
    };
    if tracks.iter().all(|track| rank(track) == wanted.len()) {
        return;
    }
    tracks.sort_by_key(|track| rank(track));
    for (position, track) in tracks.iter_mut().enumerate() {
        track.is_default = Some(position == 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn langs(sub: &[&str], dub: &[&str]) -> Option<Languages> {
        Some(Languages {
            sub: sub.iter().map(|s| s.to_string()).collect(),
            dub: dub.iter().map(|s| s.to_string()).collect(),
        })
    }

    fn prefs(sub: &[&str], dub: &[&str], strict: bool) -> LanguagePrefs {
        LanguagePrefs {
            sub: sub.iter().map(|s| s.to_string()).collect(),
            dub: dub.iter().map(|s| s.to_string()).collect(),
            strict,
            pinned: None,
        }
    }

    fn track(language: &str) -> SubtitleItem {
        SubtitleItem {
            url: format!("https://example.com/{}.vtt", language),
            language: language.to_string(),
            format: "vtt".to_string(),
            is_default: None,
        }
    }

    #[test]
    fn labels_normalise_to_codes() {
        for (label, code) in [
            ("English", "en"),
            ("en", "en"),
            ("ENG", "en"),
            ("en-US", "en"),
            ("English [CC]", "en"),
            ("Español", "es"),
            ("Spanish - Latin America", "es"),
            ("pt-BR", "pt"),
            ("Portuguese (Brazil)", "pt"),
            ("Português", "pt"),
            ("Bahasa Indonesia", "id"),
            ("العربية", "ar"),
            ("zh_Hans", "zh"),
            ("Deutsch", "de"),
            ("Full English Subs", "en"),
        ] {
            assert_eq!(normalize_language(label).as_deref(), Some(code), "{}", label);
        }
        for label in ["", "Unknown", "Track 1", "default"] {
            assert_eq!(normalize_language(label), None, "{}", label);
        }
        assert_eq!(language_name("hi"), "Hindi");
        assert_eq!(language_name("xx"), "xx");
    }

    #[test]
    fn manifest_languages_and_fallback() {
        let declared = Languages::from_manifest(&json!({ "lang": "ru", "languages": { "dub": ["ru"] } }));
        assert_eq!(declared, langs(&[], &["ru"]));
        let mixed = Languages::from_manifest(&json!({ "languages": { "sub": ["EN", "pt-BR", "en"], "dub": "en" } }));
        assert_eq!(mixed, langs(&["en", "pt"], &[]));
        assert_eq!(Languages::from_manifest(&json!({ "lang": "en" })), None);
        assert_eq!(Languages::from_manifest(&json!({ "languages": ["en"] })), None);

        // Without a declaration the manifest's lang covers the sub, and the dub only when there is one.
        assert_eq!(Some(Languages::effective(None, "it", false)), langs(&["it"], &[]));
        assert_eq!(Some(Languages::effective(None, "English", true)), langs(&["en"], &["en"]));
        assert_eq!(Languages::effective(None, "", true), Languages::default());
        let ru = declared.unwrap();
        assert_eq!(Languages::effective(Some(&ru), "ru", false), ru);
        assert!(ru.dub_only(false));
        assert!(!langs(&["it"], &["it"]).unwrap().dub_only(false));
        assert!(!ru.dub_only(true));
    }

    #[test]
    fn preferences_from_settings() {
        assert_eq!(LanguagePrefs::from_settings(&json!({})), LanguagePrefs::default());
        let read = LanguagePrefs::from_settings(&json!({
            "preferred_sub_languages": ["English", "es"],
            "preferred_dub_languages": [],
            "only_preferred_languages": false,
            "preferred_provider": "hianime",
        }));
        assert_eq!(read.sub, vec!["en", "es"]);
        assert_eq!(read.dub, vec!["en"]);
        assert!(!read.strict);
        assert_eq!(read.pinned.as_deref(), Some("hianime"));
        let auto = LanguagePrefs::from_settings(&json!({ "preferred_provider": "auto" }));
        assert_eq!(auto.pinned, None);
        assert_eq!(prefs(&["hi", "en"], &[], true).describe(&Category::Sub), "Hindi or English subtitles");
        assert_eq!(prefs(&[], &["ja", "en", "hi"], true).describe(&Category::Dub), "Japanese, English or Hindi audio");
    }

    #[test]
    fn tiers_follow_preference_order() {
        let lineup = vec![
            (langs(&["it"], &["it"]), true),      // 0 Italian
            (langs(&["en"], &["en"]), true),      // 1 English
            (langs(&["en", "es"], &[]), true),    // 2 English + Spanish subs
            (langs(&["es"], &[]), true),          // 3 Spanish
            (langs(&[], &["ru"]), true),          // 4 Russian dub only
            (None, true),                         // 5 undeclared
            (langs(&["en"], &["en"]), false),     // 6 cannot answer
        ];
        let sub = plan_tiers(&lineup, &Category::Sub, &prefs(&["es", "en"], &["en"], true));
        assert_eq!(
            sub,
            vec![
                Tier { language: Some("es".into()), members: vec![2, 3, 5] },
                Tier { language: Some("en".into()), members: vec![1] },
            ]
        );

        let loose = plan_tiers(&lineup, &Category::Sub, &prefs(&["en"], &["en"], false));
        assert_eq!(loose.len(), 2);
        assert_eq!(loose[0], Tier { language: Some("en".into()), members: vec![1, 2, 5] });
        assert_eq!(loose[1], Tier { language: None, members: vec![0, 3, 4] });

        let dub = plan_tiers(&lineup, &Category::Dub, &prefs(&["en"], &["ru", "en"], true));
        assert_eq!(
            dub,
            vec![
                Tier { language: Some("ru".into()), members: vec![4, 5] },
                Tier { language: Some("en".into()), members: vec![1] },
            ]
        );

        // Nobody serves Hindi: strict leaves nothing to ask, otherwise everything usable.
        let only = vec![(langs(&["it"], &[]), true), (langs(&["en"], &[]), true)];
        assert!(plan_tiers(&only, &Category::Sub, &prefs(&["hi"], &[], true)).is_empty());
        assert_eq!(
            plan_tiers(&only, &Category::Sub, &prefs(&["hi"], &[], false)),
            vec![Tier { language: None, members: vec![0, 1] }]
        );
    }

    #[test]
    fn served_language_and_subtitle_order() {
        let anizone = langs(&["en", "es", "pt"], &[]);
        let wanted = vec!["pt".to_string(), "en".to_string()];
        assert_eq!(served_language(anizone.as_ref(), &Category::Sub, &wanted).as_deref(), Some("pt"));
        assert_eq!(served_language(anizone.as_ref(), &Category::Dub, &wanted), None);
        assert_eq!(served_language(langs(&["it"], &[]).as_ref(), &Category::Sub, &wanted).as_deref(), Some("it"));

        let mut tracks = vec![track("English"), track("Español"), track("Português (Brasil)"), track("Deutsch")];
        tracks[0].is_default = Some(true);
        prefer_subtitles(&mut tracks, &wanted);
        let order: Vec<&str> = tracks.iter().map(|t| t.language.as_str()).collect();
        assert_eq!(order, vec!["Português (Brasil)", "English", "Español", "Deutsch"]);
        assert_eq!(tracks[0].is_default, Some(true));
        assert!(tracks[1..].iter().all(|t| t.is_default == Some(false)));

        let mut untouched = vec![track("Deutsch"), track("Unknown")];
        untouched[1].is_default = Some(true);
        prefer_subtitles(&mut untouched, &wanted);
        assert_eq!(untouched[0].language, "Deutsch");
        assert_eq!(untouched[1].is_default, Some(true));
    }
}
