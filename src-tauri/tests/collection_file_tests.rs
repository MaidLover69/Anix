//! Tests of the shareable playlist file (`collection_file.rs`) and of importing one into the
//! database. Files from other users are untrusted, so every refusal path is covered.

use anix_lib::collection_file::{self, CollectionItem};
use anix_lib::db;
use anix_lib::models::{CustomPlaylist, CustomPlaylistItem};
use serde_json::{json, Value};

fn playlist(name: &str, description: Option<&str>, rule: Option<&str>) -> CustomPlaylist {
    CustomPlaylist {
        id: "pl-1".into(),
        name: name.into(),
        description: description.map(String::from),
        created_at: 1,
        updated_at: 2,
        item_count: 2,
        covers: vec!["https://example.com/cover.jpg".into()],
        rule: rule.map(String::from),
    }
}

fn item(anilist_id: i64, title: &str, episode: f64, category: &str) -> CustomPlaylistItem {
    CustomPlaylistItem {
        id: format!("item-{}", episode),
        playlist_id: "pl-1".into(),
        anilist_id,
        mal_id: Some(52991),
        episode_num: episode,
        episode_title: Some("The Journey's End".into()),
        series_title: title.into(),
        series_cover: Some("https://example.com/cover.jpg".into()),
        category: category.into(),
        sort_order: episode as i64,
        added_at: 1_700_000_000,
    }
}

fn file(items: Value) -> Value {
    json!({"format": "anix-collection", "version": 1, "name": "Weekend", "description": null, "rule": null, "items": items})
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn good_item() -> Value {
    json!({"anilist_id": 154587, "title": "Frieren", "episode": 1.0, "category": "sub"})
}

fn refusal(value: &Value) -> String {
    collection_file::parse(&bytes(value)).expect_err("must be refused").0
}

#[test]
fn export_holds_only_the_shareable_fields_and_imports_again() {
    let items = [item(154587, "Frieren", 1.0, "sub"), item(-100000021, "Stand-in", 2.5, "dub")];
    let text = collection_file::to_json(&playlist("Weekend", Some("Two shows\nfor Sunday"), Some("genre=Drama&sort=score")), &items);

    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["format"], "anix-collection");
    assert_eq!(value["version"], 1);
    assert_eq!(value["items"][0], json!({"anilist_id": 154587, "title": "Frieren", "episode": 1.0, "category": "sub"}));
    for private in ["cover", "52991", "Journey", "1700000000", "pl-1", "item-"] {
        assert!(!text.contains(private), "{} must not be exported", private);
    }

    let parsed = collection_file::parse(text.as_bytes()).unwrap();
    assert_eq!(parsed.name, "Weekend");
    assert_eq!(parsed.description.as_deref(), Some("Two shows\nfor Sunday"));
    assert_eq!(parsed.rule.as_deref(), Some("genre=Drama&sort=score"));
    assert!(parsed.skipped.is_empty());
    assert_eq!(
        parsed.items,
        vec![
            CollectionItem { anilist_id: 154587, title: "Frieren".into(), episode: 1.0, category: "sub".into() },
            CollectionItem { anilist_id: -100000021, title: "Stand-in".into(), episode: 2.5, category: "dub".into() },
        ]
    );
}

#[test]
fn export_of_an_overlong_or_empty_name_still_imports() {
    let long = "x".repeat(300);
    let parsed = collection_file::parse(collection_file::to_json(&playlist(&long, None, None), &[]).as_bytes()).unwrap();
    assert_eq!(parsed.name.chars().count(), collection_file::MAX_NAME_CHARS);
    let parsed = collection_file::parse(collection_file::to_json(&playlist(" \u{7} ", None, Some("bad rule!")), &[]).as_bytes()).unwrap();
    assert_eq!(parsed.name, "Collection");
    assert_eq!(parsed.rule, None, "an invalid stored rule is left out");
}

#[test]
fn refuses_files_that_are_not_collections() {
    assert_eq!(collection_file::parse(b"not json {").unwrap_err().0, "This file is not an Anix collection.");
    let mut wrong_format = file(json!([]));
    wrong_format["format"] = json!("anix-backup");
    assert_eq!(refusal(&wrong_format), "This file is not an Anix collection.");
    for version in [json!(2), json!(0), json!("1"), Value::Null] {
        let mut wrong_version = file(json!([]));
        wrong_version["version"] = version.clone();
        assert!(refusal(&wrong_version).contains("different version"), "{}", version);
    }
    let mut no_items = file(json!([]));
    no_items["items"] = json!({"0": good_item()});
    assert_eq!(refusal(&no_items), "This collection file is damaged.");
}

#[test]
fn refuses_files_over_the_size_and_item_limits() {
    let mut big = bytes(&file(json!([])));
    big.resize(collection_file::MAX_FILE_BYTES + 1, b' ');
    assert!(collection_file::parse(&big).unwrap_err().0.contains("too large"));

    let items: Vec<Value> = (0..=collection_file::MAX_ITEMS).map(|_| json!({})).collect();
    assert!(refusal(&file(Value::Array(items))).contains("too many episodes"));
    let items: Vec<Value> = (0..collection_file::MAX_ITEMS).map(|_| json!({})).collect();
    assert_eq!(collection_file::parse(&bytes(&file(Value::Array(items)))).unwrap().skipped.len(), 5000);
}

#[test]
fn refuses_a_bad_name_description_or_rule() {
    for name in [json!(""), json!("  \u{0}\u{1b} "), Value::Null, json!(5)] {
        let mut value = file(json!([]));
        value["name"] = name.clone();
        assert_eq!(refusal(&value), "This collection has no name.", "{}", name);
    }
    let mut value = file(json!([]));
    value["name"] = json!("n".repeat(121));
    assert!(refusal(&value).contains("name is too long"));
    value["name"] = json!(format!("  {}\u{7}  ", "n".repeat(120)));
    assert_eq!(collection_file::parse(&bytes(&value)).unwrap().name, "n".repeat(120));

    let mut value = file(json!([]));
    value["description"] = json!("d".repeat(1001));
    assert!(refusal(&value).contains("description is too long"));
    value["description"] = json!(["not", "text"]);
    assert_eq!(refusal(&value), "This collection file is damaged.");
    value["description"] = json!("   ");
    assert_eq!(collection_file::parse(&bytes(&value)).unwrap().description, None);

    for rule in ["genre=Drama;DROP", "a b", "<script>", "x/y", &"a".repeat(2001)] {
        let mut value = file(json!([]));
        value["rule"] = json!(rule);
        assert_eq!(refusal(&value), "This smart collection's rule is not valid.", "{}", rule);
    }
    let mut value = file(json!([]));
    value["rule"] = json!(7);
    assert_eq!(refusal(&value), "This collection file is damaged.");
    value["rule"] = json!("");
    assert_eq!(collection_file::parse(&bytes(&value)).unwrap().rule, None);
}

#[test]
fn rule_validator_accepts_only_query_string_characters() {
    assert_eq!(collection_file::validate_rule(None), Ok(None));
    assert_eq!(collection_file::validate_rule(Some("")), Ok(None));
    let ok = "status=CURRENT&genre=Slice+of+Life&q=frieren%20s2&min=7.5&x=a_b~c-d";
    assert_eq!(collection_file::validate_rule(Some(ok)), Ok(Some(ok.to_string())));
    assert!(collection_file::validate_rule(Some(&"a".repeat(2000))).is_ok());
    for bad in ["a\nb", "é=1", "a#b", "a?b", "\"", &"a".repeat(2001)] {
        assert!(collection_file::validate_rule(Some(bad)).is_err(), "{:?}", bad);
    }
}

#[test]
fn bad_items_are_skipped_and_reported_while_good_ones_stay() {
    let items = json!([
        good_item(),
        {"anilist_id": 0, "title": "Zero id", "episode": 1, "category": "sub"},
        {"anilist_id": 1_000_000_000_000i64, "title": "Huge id", "episode": 1, "category": "sub"},
        {"anilist_id": 1.5, "title": "Fraction id", "episode": 1, "category": "sub"},
        {"anilist_id": 5, "title": "NaN episode", "episode": "NaN", "category": "sub"},
        {"anilist_id": 5, "title": "Negative episode", "episode": -1, "category": "sub"},
        {"anilist_id": 5, "title": "Huge episode", "episode": 100_001, "category": "sub"},
        {"anilist_id": 5, "title": "Raw", "episode": 1, "category": "raw"},
        {"anilist_id": 5, "title": "  \u{0} ", "episode": 1, "category": "sub"},
        {"anilist_id": 5, "title": "t".repeat(301), "episode": 1, "category": "sub"},
        "not an object",
        {"anilist_id": -100000021, "title": " Stand-in\u{7} ", "episode": 100_000, "category": "dub"},
        {"anilist_id": 999_999_999_999i64, "title": "Largest id", "episode": 0, "category": "sub"}
    ]);
    let parsed = collection_file::parse(&bytes(&file(items))).unwrap();

    let kept: Vec<(i64, &str)> = parsed.items.iter().map(|i| (i.anilist_id, i.title.as_str())).collect();
    assert_eq!(kept, vec![(154587, "Frieren"), (-100000021, "Stand-in"), (999_999_999_999, "Largest id")]);
    assert_eq!(
        parsed.skipped,
        vec![
            "Zero id".to_string(),
            "Huge id".into(),
            "Fraction id".into(),
            "NaN episode".into(),
            "Negative episode".into(),
            "Huge episode".into(),
            "Raw".into(),
            "#9".into(),
            "t".repeat(80),
            "#11".into(),
        ]
    );
}

#[test]
fn unique_name_adds_a_number_ignoring_case() {
    let existing = vec!["Weekend".to_string(), "weekend (2)".to_string(), "Other".to_string()];
    assert_eq!(collection_file::unique_name("Fresh", &existing), "Fresh");
    assert_eq!(collection_file::unique_name("Weekend", &existing), "Weekend (3)");
    assert_eq!(collection_file::unique_name("WEEKEND", &existing), "WEEKEND (3)");
    assert_eq!(collection_file::unique_name("other", &existing), "other (2)");
    assert_eq!(collection_file::unique_name("Weekend", &[]), "Weekend");
}

#[test]
fn file_name_has_no_path_parts() {
    assert_eq!(collection_file::file_name("Weekend"), "Weekend.anix-collection.json");
    assert_eq!(collection_file::file_name("../../etc/passwd"), "etcpasswd.anix-collection.json");
    assert_eq!(collection_file::file_name(" a\\b\u{0}c\n "), "abc.anix-collection.json");
    assert_eq!(collection_file::file_name("  "), "collection.anix-collection.json");
    assert_eq!(collection_file::file_name("..."), "collection.anix-collection.json");
    assert_eq!(collection_file::file_name(&"é".repeat(200)), format!("{}.anix-collection.json", "é".repeat(80)));
}

#[tokio::test]
async fn importing_makes_a_new_playlist_and_leaves_a_same_named_one_alone() {
    let pool = db::init_db_in_memory().await.unwrap();
    let existing = db::create_custom_playlist(&pool, "weekend", Some("mine"), None).await.unwrap();
    db::add_items_to_custom_playlist(&pool, &[CustomPlaylistItem { playlist_id: existing.id.clone(), ..item(1, "Mine", 1.0, "sub") }])
        .await
        .unwrap();

    let mut value = file(json!([good_item(), {"anilist_id": 0, "title": "Bad", "episode": 1, "category": "sub"},
        {"anilist_id": -100000021, "title": "Stand-in", "episode": 3, "category": "dub"}]));
    value["rule"] = json!("genre=Drama");
    let parsed = collection_file::parse(&bytes(&value)).unwrap();
    let (imported, added) = db::import_custom_playlist(&pool, &parsed).await.unwrap();
    assert_eq!(added, 2);
    assert_eq!(imported.name, "Weekend (2)");
    assert_eq!(imported.rule.as_deref(), Some("genre=Drama"));
    assert_eq!(imported.item_count, 2);
    assert_ne!(imported.id, existing.id);

    let mine = db::get_custom_playlist_items(&pool, &existing.id).await.unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].series_title, "Mine");
    let new_items = db::get_custom_playlist_items(&pool, &imported.id).await.unwrap();
    let shape: Vec<_> = new_items
        .iter()
        .map(|i| (i.anilist_id, i.series_title.as_str(), i.episode_num, i.category.as_str(), i.sort_order))
        .collect();
    assert_eq!(shape, vec![(154587, "Frieren", 1.0, "sub", 0), (-100000021, "Stand-in", 3.0, "dub", 1)]);
    assert!(new_items.iter().all(|i| i.series_cover.is_none() && i.mal_id.is_none() && i.episode_title.is_none()));

    let listed = db::list_custom_playlists(&pool).await.unwrap();
    assert_eq!(listed.len(), 2);
    let old = listed.iter().find(|p| p.id == existing.id).unwrap();
    assert_eq!((old.name.as_str(), old.description.as_deref(), old.rule.as_deref()), ("weekend", Some("mine"), None));

    // The same file again makes a third playlist rather than merging.
    let (again, _) = db::import_custom_playlist(&pool, &parsed).await.unwrap();
    assert_eq!(again.name, "Weekend (3)");
    assert_eq!(db::get_custom_playlist_items(&pool, &imported.id).await.unwrap().len(), 2);
}

#[test]
fn refusal_messages_map_to_the_generic_app_error_code() {
    let mut messages = vec![
        collection_file::parse(b"[").unwrap_err().0,
        collection_file::validate_rule(Some("a b")).unwrap_err(),
        "This playlist no longer exists.".to_string(),
        "Could not save the collection: Permission denied (os error 13)".to_string(),
        "Could not read the collection file: No such file or directory (os error 2)".to_string(),
    ];
    let mut big = vec![b' '; collection_file::MAX_FILE_BYTES + 1];
    big[0] = b'{';
    messages.push(collection_file::parse(&big).unwrap_err().0);
    for (key, bad) in [("version", json!(2)), ("name", json!("")), ("description", json!("d".repeat(1001))), ("items", json!(1))] {
        let mut value = file(json!([]));
        value[key] = bad;
        messages.push(refusal(&value));
    }
    messages.push(refusal(&file(Value::Array(vec![json!({}); collection_file::MAX_ITEMS + 1]))));
    let mut long_name = file(json!([]));
    long_name["name"] = json!("n".repeat(121));
    messages.push(refusal(&long_name));
    for message in messages {
        assert_eq!(anix_lib::error::code_for_message(&message), "APP-ERR", "{}", message);
    }
}
