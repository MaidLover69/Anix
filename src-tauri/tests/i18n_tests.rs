//! Backend text in the interface language: notifications, release-check lines, reminders and
//! the language a setting resolves to.

use anix_lib::i18n;
use anix_lib::models::Category;
use anix_lib::notifier::{Airing, ReleaseNotification};
use anix_lib::providers::languages::LanguagePrefs;
use anix_lib::release_check;
use anix_lib::reminders::Reminder;

fn notification(episodes: &[i32]) -> ReleaseNotification {
    ReleaseNotification {
        media_id: 7,
        title: "Frieren".to_string(),
        cover_image: None,
        episodes: episodes.iter().map(|&episode| Airing { media_id: 7, episode, airing_at: 0 }).collect(),
        note: None,
    }
}

#[test]
fn episode_out_in_french_and_english() {
    assert_eq!(notification(&[5]).body_in("fr"), "L’épisode 5 est disponible");
    assert_eq!(notification(&[5]).body_in("en"), "Episode 5 is out");
    assert_eq!(notification(&[5]).body(), "Episode 5 is out", "log lines stay English");
    assert_eq!(notification(&[3, 4]).body_in("de"), "Folgen 3 und 4 sind da");
    assert_eq!(notification(&[3, 4, 5]).body_in("fr"), "Les épisodes 3 à 5 sont disponibles");
}

#[test]
fn unknown_language_falls_back_to_english() {
    assert_eq!(notification(&[5]).body_in("xx"), "Episode 5 is out");
    assert_eq!(i18n::text("fr", "no_such_key"), "no_such_key", "a missing key shows as itself");
}

#[test]
fn release_check_lines() {
    assert_eq!(
        release_check::found_body_in("fr", 5, &Category::Sub, Some("en")),
        "L’épisode 5 est disponible — Audio japonais · sous-titres en anglais"
    );
    assert_eq!(
        release_check::found_body(5, &Category::Sub, Some("en")),
        "Episode 5 is out — Japanese audio · English subtitles"
    );
    let prefs = LanguagePrefs { sub: vec!["en".into(), "es".into()], dub: vec!["en".into()], ..LanguagePrefs::default() };
    assert_eq!(
        release_check::missing_body_in("es", 2, &Category::Sub, &prefs),
        "El episodio 2 ya se emitió, pero ninguna fuente lo tiene aún — subtítulos en inglés o español"
    );
    assert_eq!(
        release_check::missing_body(2, &Category::Sub, &prefs),
        "Episode 2 has aired, but no source has it yet — English or Spanish subtitles"
    );
}

#[test]
fn reminder_plurals() {
    let reminder = Reminder { media_id: 7, episode: 5, airing_at: 600, lead_minutes: 10 };
    assert_eq!(reminder.body_in("en", 540), "Episode 5 airs in 1 minute");
    assert_eq!(reminder.body_in("en", 0), "Episode 5 airs in 10 minutes");
    assert_eq!(reminder.body_in("de", 0), "Folge 5 läuft in 10 Minuten");
}

#[test]
fn language_setting_resolves() {
    let system = |v: &str| vec![None, None, Some(v.to_string())];
    assert_eq!(i18n::resolve(Some("ar"), &system("en_US.UTF-8")), "ar");
    assert_eq!(i18n::resolve(Some("system"), &system("pt_BR.UTF-8")), "pt");
    assert_eq!(i18n::resolve(None, &system("ja_JP.UTF-8")), "ja");
    assert_eq!(i18n::resolve(None, &system("nl_NL.UTF-8")), "en");
    assert_eq!(i18n::language_name("de", "ja"), "Japanisch");
    assert_eq!(i18n::language_name("fr", "xx"), "xx", "unknown codes stay as written");
}

#[test]
fn every_language_has_every_text() {
    for lang in i18n::LANGUAGES {
        for key in ["episode_out", "found", "missing", "reminder_other", "tray_open", "tray_quit", "today"] {
            assert_ne!(i18n::text(lang, key), key, "{lang} {key}");
        }
    }
}

#[test]
fn system_language_follows_gettext_order() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
    };
    let pick = |pairs| i18n::resolve(Some("system"), &i18n::system_languages(env(pairs)));
    assert_eq!(pick(&[("LANGUAGE", "fr:en"), ("LANG", "en_US.UTF-8")]), "fr");
    assert_eq!(pick(&[("LANGUAGE", "nl:de"), ("LANG", "en_US.UTF-8")]), "de", "unsupported entries are skipped");
    assert_eq!(pick(&[("LANGUAGE", "fr"), ("LC_ALL", "C"), ("LANG", "de_DE.UTF-8")]), "en", "C ignores LANGUAGE");
    assert_eq!(pick(&[("LANGUAGE", "fr")]), "en", "LANGUAGE needs a locale to apply");
    assert_eq!(pick(&[("LC_MESSAGES", "ru_RU.UTF-8"), ("LANG", "de_DE.UTF-8")]), "ru");
}
