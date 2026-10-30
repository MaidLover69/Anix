//! Provider choice by language: tiers of preferred languages, the strict switch, dub-only
//! sites and episode lists, with in-process providers. No network is involved.

mod english_ui;

use english_ui::english_ui;
use anix_lib::error::AppError;
use anix_lib::models::{AnimeQuery, Category, EpisodeItem, ProviderData, SourcesResult, StreamItem, SubtitleItem};
use anix_lib::providers::languages::{plan_tiers, LanguagePrefs, Languages, Tier};
use anix_lib::providers::manager::ProviderManager;
use anix_lib::providers::AnimeProvider;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Site {
    name: &'static str,
    languages: Option<Languages>,
    dub: bool,
    delay: Duration,
    works: bool,
    subtitles: Vec<&'static str>,
    /// The category each `get_sources` call asked for.
    asked: Mutex<Vec<Category>>,
}

fn codes(list: &[&str]) -> Vec<String> {
    list.iter().map(|c| c.to_string()).collect()
}

fn site(name: &'static str, sub: &[&str], dub: &[&str]) -> Site {
    Site {
        name,
        languages: Some(Languages { sub: codes(sub), dub: codes(dub) }),
        dub: !dub.is_empty(),
        delay: Duration::ZERO,
        works: true,
        subtitles: Vec::new(),
        asked: Mutex::new(Vec::new()),
    }
}

impl Site {
    fn slow(mut self, millis: u64) -> Self {
        self.delay = Duration::from_millis(millis);
        self
    }

    fn broken(mut self) -> Self {
        self.works = false;
        self
    }

    fn without_dub(mut self) -> Self {
        self.dub = false;
        self
    }

    fn undeclared(mut self) -> Self {
        self.languages = None;
        self
    }

    fn with_subtitles(mut self, labels: &[&'static str]) -> Self {
        self.subtitles = labels.to_vec();
        self
    }

    fn calls(&self) -> Vec<Category> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait]
impl AnimeProvider for Site {
    fn name(&self) -> &'static str {
        self.name
    }

    fn supports_dub(&self) -> bool {
        self.dub
    }

    fn languages(&self) -> Option<Languages> {
        self.languages.clone()
    }

    fn verifies_streams(&self) -> bool {
        true
    }

    async fn get_episodes(&self, _query: &AnimeQuery) -> Result<ProviderData, AppError> {
        let list: Vec<EpisodeItem> = [1.0, 2.0]
            .iter()
            .map(|n| EpisodeItem::new(format!("{}|{}", self.name, n), *n, None, None, false))
            .collect();
        Ok(ProviderData {
            name: self.name.to_string(),
            dub: if self.dub { list.clone() } else { Vec::new() },
            sub: list,
        })
    }

    async fn get_sources(&self, _episode: &EpisodeItem, category: Category) -> Result<SourcesResult, AppError> {
        self.asked.lock().unwrap().push(category.clone());
        tokio::time::sleep(self.delay).await;
        if !self.works {
            return Err(AppError::provider(self.name, "broken on purpose"));
        }
        let stream = StreamItem::new(
            format!("https://{}.example/master.m3u8", self.name),
            "hls".to_string(),
            None,
            Some(category.as_str().to_string()),
            None,
            true,
        );
        let mut result = SourcesResult::new(vec![stream]);
        result.subtitles = self
            .subtitles
            .iter()
            .map(|label| SubtitleItem {
                url: format!("https://{}.example/{}.vtt", self.name, label),
                language: label.to_string(),
                format: "vtt".to_string(),
                is_default: None,
            })
            .collect();
        Ok(result)
    }
}

fn prefs(sub: &[&str], dub: &[&str], strict: bool) -> LanguagePrefs {
    LanguagePrefs { sub: codes(sub), dub: codes(dub), strict, pinned: None }
}

fn manager(sites: &[Arc<Site>], prefs: LanguagePrefs) -> ProviderManager {
    ProviderManager::with_providers_and_languages(
        sites.iter().map(|s| s.clone() as Arc<dyn AnimeProvider>).collect(),
        prefs,
    )
}

async fn play(manager: &ProviderManager, category: &str, provider: Option<&str>) -> Result<SourcesResult, AppError> {
    manager
        .get_sources_for_episode(30, None, 1.0, category, Some("Frieren".to_string()), provider)
        .await
}

fn message(result: Result<SourcesResult, AppError>) -> String {
    match result {
        Err(AppError::SourcesUnavailable(message)) => message,
        other => panic!("expected a failure message, got {:?}", other),
    }
}

#[tokio::test]
async fn faster_site_in_another_language_is_not_asked() {
    let italian = Arc::new(site("animeworld", &["it"], &[]));
    let english = Arc::new(site("hianime", &["en"], &["en"]).slow(150));
    let found = play(&manager(&[italian.clone(), english.clone()], prefs(&["en"], &["en"], true)), "sub", None)
        .await
        .unwrap();
    assert_eq!(found.provider.as_deref(), Some("hianime"));
    assert_eq!(found.language.as_deref(), Some("en"));
    assert!(italian.calls().is_empty(), "a site without English subtitles must not race");
}

#[tokio::test]
async fn languages_are_tried_in_preference_order() {
    // The Hindi site is slower, but Hindi is wanted first: English is not asked at all.
    let hindi = Arc::new(site("hindisite", &["hi"], &[]).slow(150));
    let english = Arc::new(site("hianime", &["en"], &[]));
    let both = manager(&[english.clone(), hindi.clone()], prefs(&["hi", "en"], &["en"], true));
    let found = play(&both, "sub", None).await.unwrap();
    assert_eq!((found.provider.as_deref(), found.language.as_deref()), (Some("hindisite"), Some("hi")));
    assert!(english.calls().is_empty());

    // When the Hindi tier has nothing, the English tier is raced next.
    let hindi = Arc::new(site("hindisite", &["hi"], &[]).broken());
    let english = Arc::new(site("hianime", &["en"], &[]));
    let found = play(&manager(&[hindi.clone(), english.clone()], prefs(&["hi", "en"], &["en"], true)), "sub", None)
        .await
        .unwrap();
    assert_eq!((found.provider.as_deref(), found.language.as_deref()), (Some("hianime"), Some("en")));
    assert_eq!(hindi.calls().len(), 1);
}

#[tokio::test]
async fn strict_preferences_refuse_other_languages() {
    english_ui();
    let italian = Arc::new(site("animeworld", &["it"], &[]));
    let strict = manager(&[italian.clone()], prefs(&["hi"], &["en"], true));
    let refused = message(play(&strict, "sub", None).await);
    assert!(refused.contains("Hindi subtitles"), "{}", refused);
    assert!(refused.contains("Settings › Sources"), "{}", refused);
    assert!(italian.calls().is_empty());

    // Matching plugins that all fail: the reasons, and a note that others were left out.
    let english = Arc::new(site("hianime", &["en"], &[]).broken());
    let failed = message(play(&manager(&[italian.clone(), english], prefs(&["en"], &["en"], true)), "sub", None).await);
    assert!(failed.contains("hianime: broken on purpose"), "{}", failed);
    assert!(failed.contains("Only plugins with English subtitles were asked"), "{}", failed);
    assert!(italian.calls().is_empty());
}

#[tokio::test]
async fn loose_preferences_fall_back_to_other_languages() {
    let italian = Arc::new(site("animeworld", &["it"], &[]));
    let english = Arc::new(site("hianime", &["en"], &[]).broken());
    let loose = manager(&[italian.clone(), english.clone()], prefs(&["en"], &["en"], false));
    let found = play(&loose, "sub", None).await.unwrap();
    assert_eq!((found.provider.as_deref(), found.language.as_deref()), (Some("animeworld"), Some("it")));
    assert_eq!(english.calls().len(), 1, "the English site goes first");

    // No match at all, but other languages are allowed.
    let only_italian = manager(&[italian.clone()], prefs(&["hi"], &["hi"], false));
    assert_eq!(play(&only_italian, "sub", None).await.unwrap().provider.as_deref(), Some("animeworld"));
}

#[tokio::test]
async fn a_provider_asked_for_by_name_is_always_asked() {
    let italian = Arc::new(site("animeworld", &["it"], &[]));
    let english = Arc::new(site("hianime", &["en"], &[]));
    let found = play(&manager(&[english.clone(), italian], prefs(&["en"], &["en"], true)), "sub", Some("animeworld"))
        .await
        .unwrap();
    assert_eq!((found.provider.as_deref(), found.language.as_deref()), (Some("animeworld"), Some("it")));
    assert!(english.calls().is_empty());
}

#[tokio::test]
async fn dub_only_sites_serve_dubs_in_their_language() {
    english_ui();
    let russian = Arc::new(site("aniliberty", &[], &["ru"]).without_dub());
    let english = Arc::new(site("hianime", &["en"], &["en"]));
    let sites = [english.clone(), russian.clone()];

    let dub = play(&manager(&sites, prefs(&["en"], &["ru", "en"], true)), "dub", None).await.unwrap();
    assert_eq!((dub.provider.as_deref(), dub.language.as_deref()), (Some("aniliberty"), Some("ru")));
    assert_eq!(russian.calls(), vec![Category::Sub], "the site's only version is asked for");
    assert_eq!(dub.streams[0].audio.as_deref(), Some("dub"));
    assert!(english.calls().is_empty());

    // A sub is never answered with the Russian audio while the preferences are strict.
    let sub = play(&manager(&sites, prefs(&["en"], &["ru"], true)), "sub", None).await.unwrap();
    assert_eq!(sub.provider.as_deref(), Some("hianime"));
    assert_eq!(russian.calls().len(), 1);

    // A site without a dub of its own and with subtitles is not a dub site.
    let italian = Arc::new(site("animeworld", &["it"], &["it"]).without_dub());
    let none = message(play(&manager(&[italian.clone()], prefs(&["en"], &["it"], true)), "dub", None).await);
    assert!(none.contains("Italian audio"), "{}", none);
    assert!(italian.calls().is_empty());
}

#[tokio::test]
async fn undeclared_providers_count_as_any_language() {
    let mystery = Arc::new(site("mystery", &[], &[]).undeclared());
    let found = play(&manager(&[mystery], prefs(&["hi"], &["hi"], true)), "sub", None).await.unwrap();
    assert_eq!(found.provider.as_deref(), Some("mystery"));
    assert_eq!(found.language, None);
}

#[test]
fn empty_or_unknown_language_declarations_count_as_undeclared() {
    // What a manifest with `lang: "multi"`, or with no `lang` and no `languages`, comes to.
    assert_eq!(Languages { sub: codes(&["multi"]), dub: codes(&["multi"]) }.declared(), None);
    assert_eq!(Languages { sub: codes(&["multi"]), dub: Vec::new() }.declared(), None);
    assert_eq!(Languages::default().declared(), None);
    // A dub-only site still says what it serves.
    let russian = Languages { sub: Vec::new(), dub: codes(&["ru"]) };
    assert_eq!(russian.clone().declared(), Some(russian));
    let mixed = Languages { sub: codes(&["multi", "en"]), dub: Vec::new() };
    assert_eq!(mixed.clone().declared(), Some(mixed));
}

#[test]
fn undeclared_providers_are_asked_after_the_matching_tiers_even_when_strict() {
    let english = Some(Languages { sub: codes(&["en"]), dub: codes(&["en"]) });
    let italian = Some(Languages { sub: codes(&["it"]), dub: Vec::new() });
    let lineup = vec![
        (None, true),           // 0 undeclared
        (english, true),        // 1 English
        (italian, true),        // 2 Italian
        (None, false),          // 3 undeclared, cannot answer
    ];
    let tier = |language: Option<&str>, members: Vec<usize>| Tier { language: language.map(String::from), members };

    assert_eq!(
        plan_tiers(&lineup, &Category::Sub, &prefs(&["en"], &["en"], true)),
        vec![tier(Some("en"), vec![1]), tier(None, vec![0])]
    );
    // Nobody declares Hindi: the undeclared one is still asked.
    assert_eq!(plan_tiers(&lineup, &Category::Sub, &prefs(&["hi"], &[], true)), vec![tier(None, vec![0])]);
    // Without the strict switch, declared other languages come after it.
    assert_eq!(
        plan_tiers(&lineup, &Category::Sub, &prefs(&["en"], &["en"], false)),
        vec![tier(Some("en"), vec![1]), tier(None, vec![0]), tier(None, vec![2])]
    );
}

#[tokio::test]
async fn subtitle_tracks_in_preferred_languages_come_first() {
    let anizone = Arc::new(site("anizone", &["en", "es"], &[]).with_subtitles(&["English", "Español", "Deutsch"]));
    let found = play(&manager(&[anizone], prefs(&["es", "en"], &["en"], true)), "sub", None).await.unwrap();
    let order: Vec<&str> = found.subtitles.iter().map(|t| t.language.as_str()).collect();
    assert_eq!(order, vec!["Español", "English", "Deutsch"]);
    assert_eq!(found.subtitles[0].is_default, Some(true));
    assert_eq!(found.language.as_deref(), Some("es"));
}

#[tokio::test]
async fn episode_lists_follow_the_language_filter() {
    let italian = Arc::new(site("animeworld", &["it"], &[]));
    let english = Arc::new(site("hianime", &["en"], &[]).without_dub());
    let russian = Arc::new(site("aniliberty", &[], &["ru"]).without_dub());
    let sites = [italian.clone(), english.clone(), russian.clone()];

    let listed = manager(&sites, prefs(&["en"], &["ru"], true))
        .get_all_episodes(30, None, Some("Frieren".to_string()))
        .await
        .unwrap();
    let names: Vec<&str> = listed.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["hianime", "aniliberty"]);
    assert_eq!((listed[0].sub.len(), listed[0].dub.len()), (2, 0));
    assert_eq!((listed[1].sub.len(), listed[1].dub.len()), (0, 2), "the Russian site lists dubs");

    let mut pinned = prefs(&["en"], &["en"], true);
    pinned.pinned = Some("animeworld".to_string());
    let listed = manager(&sites, pinned).get_all_episodes(31, None, Some("Frieren".to_string())).await.unwrap();
    let names: Vec<&str> = listed.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["animeworld", "hianime"], "the provider picked in settings stays listed");

    let everything = manager(&sites, prefs(&["en"], &["en"], false))
        .get_all_episodes(32, None, Some("Frieren".to_string()))
        .await
        .unwrap();
    assert_eq!(everything.len(), 3);
}
