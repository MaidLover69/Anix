//! Tests for the `$scannerUtils` port: Seanime's own test cases for title normalisation and
//! season extraction, the JS-facing dispatcher, and outputs recorded from the Go
//! implementation running under goja.

use anilili_lib::plugins::scanner_utils::{
    build_advanced_query, build_part_query, build_search_query, build_season_query,
    build_smart_search_titles, call, compare_titles, extract_part_number, extract_season_number,
    extract_year, find_best_match, get_significant_tokens, integer_to_ordinal, normalize_title,
    sanitize_query, significant_tokens_of_title, weighted_token_match_ratio,
};
use serde_json::{json, Value};

struct NormalizeCase {
    input: &'static str,
    want: &'static str,
    want_base: &'static str,
    want_denoised: &'static str,
    /// `None` where the Go test leaves the field unchecked.
    season: Option<i64>,
    part: Option<i64>,
}

const fn case(
    input: &'static str,
    want: &'static str,
    want_base: &'static str,
    want_denoised: &'static str,
    season: Option<i64>,
    part: Option<i64>,
) -> NormalizeCase {
    NormalizeCase { input, want, want_base, want_denoised, season, part }
}

/// `TestNormalizeTitle` from `internal/library/scanner/title_normalization_test.go`.
#[test]
fn normalize_title_seanime_cases() {
    let cases = [
        case("Attack on Titan", "attack on titan", "attack on titan", "attack titan", Some(-1), Some(-1)),
        case("Attack on Titan Season 2", "attack on titan", "attack on titan", "attack titan", Some(2), Some(-1)),
        case("Attack on Titan Season 3 Part 2", "attack on titan", "attack on titan", "attack titan", Some(3), Some(2)),
        case("Overlord III", "overlord iii", "overlord", "overlord", Some(3), None),
        case("Attack on Titan Part II", "attack on titan", "attack on titan", "attack titan", Some(-1), Some(2)),
        case("Mushoku Tensei II Part 2", "mushoku tensei ii", "mushoku tensei", "mushoku tensei", Some(2), Some(2)),
        case("Steins;Gate", "steins gate", "steins gate", "steins gate", None, None),
        case("Kino's Journey", "kino journey", "kino journey", "kino journey", None, None),
        case("Persona 4 The Animation", "persona 4", "persona", "persona", None, None),
        case("86 - Eighty Six", "86 eighty six", "86 eighty six", "86 eighty six", None, None),
        case("ATTACK ON TITAN", "attack on titan", "attack on titan", "attack titan", None, None),
        case(
            "The Melancholy of Haruhi Suzumiya",
            "melancholy of haruhi suzumiya",
            "melancholy of haruhi suzumiya",
            "melancholy haruhi suzumiya",
            None,
            None,
        ),
        case("One Piece Episode 1000", "one piece 1000", "one piece", "one piece", None, None),
        case("Attack on Titan OAD", "attack on titan ova", "attack on titan", "attack titan", None, None),
    ];
    for c in cases {
        let got = normalize_title(c.input);
        assert_eq!(got.normalized, c.want, "normalized of {:?}", c.input);
        assert_eq!(got.clean_base_title, c.want_base, "clean base title of {:?}", c.input);
        assert_eq!(got.denoised_title, c.want_denoised, "denoised title of {:?}", c.input);
        if let Some(season) = c.season {
            assert_eq!(got.season, season, "season of {:?}", c.input);
        }
        if let Some(part) = c.part {
            assert_eq!(got.part, part, "part of {:?}", c.input);
        }
        assert_eq!(got.original, c.input);
    }
}

/// `TestExtractSeasonNumber` from `internal/util/comparison/filtering_test.go`.
#[test]
fn extract_season_number_seanime_cases() {
    let cases = [
        ("JJK season 2", 2),
        ("Spy x Family 2nd Season", 2),
        ("Spy X Family", -1),
        ("Overlord III", 3),
        ("Diamond no Ace Act II", -1),
        ("Diamond no Ace Act II Second Season", 2),
        ("Attack on Titan Part II", -1),
        ("Attack on Titan Cour II", -1),
        ("This is a test", -1),
    ];
    for (input, expected) in cases {
        assert_eq!(extract_season_number(input), expected, "season of {input:?}");
    }
}

#[test]
fn extract_season_number_other_notations() {
    assert_eq!(extract_season_number("Fate/Zero S2"), 2);
    assert_eq!(extract_season_number("Dr. Stone S03E05"), 3);
    assert_eq!(extract_season_number("Title S25"), -1);
    assert_eq!(extract_season_number("Konosuba 2"), 2);
    assert_eq!(extract_season_number("Title 11"), -1);
    assert_eq!(extract_season_number("Title Movie 2"), -1);
    assert_eq!(extract_season_number("進撃の巨人 第2期"), 2);
    assert_eq!(extract_season_number("Haikyuu!! Third Season"), 3);
    assert_eq!(extract_season_number("Title X"), -1);
}

#[test]
fn extract_part_number_notations() {
    assert_eq!(extract_part_number("Attack on Titan Season 3 Part 2"), 2);
    assert_eq!(extract_part_number("Title Cour 02"), 2);
    assert_eq!(extract_part_number("Title 3rd Part"), 3);
    assert_eq!(extract_part_number("Sword Art Online 2nd Cour"), 2);
    assert_eq!(extract_part_number("Title Part IV"), 4);
    assert_eq!(extract_part_number("Title Part I"), -1);
    assert_eq!(extract_part_number("Title Part X"), -1);
    assert_eq!(extract_part_number("Title Part 99999999999999999999"), -1);
    assert_eq!(extract_part_number("Overlord"), -1);
}

#[test]
fn extract_year_notations() {
    assert_eq!(extract_year("Bakemonogatari (2009)"), 2009);
    assert_eq!(extract_year("Cowboy Bebop 1998"), 1998);
    assert_eq!(extract_year("Blade Runner 2049"), 2049);
    assert_eq!(extract_year("Space Dandy (3000)"), -1);
    assert_eq!(extract_year("Show (1899) 2015"), 2015);
    assert_eq!(extract_year("Mob Psycho 100"), -1);
}

#[test]
fn integer_to_ordinal_suffixes() {
    let cases = [
        (1, "1st"),
        (2, "2nd"),
        (3, "3rd"),
        (4, "4th"),
        (10, "10th"),
        (11, "11th"),
        (12, "12th"),
        (13, "13th"),
        (21, "21st"),
        (22, "22nd"),
        (23, "23rd"),
        (101, "101st"),
        (111, "111th"),
        (112, "112th"),
        (0, "0th"),
        (-1, "-1st"),
        (-12, "-12th"),
    ];
    for (number, expected) in cases {
        assert_eq!(integer_to_ordinal(number), expected);
    }
}

#[test]
fn normalize_title_of_an_empty_title_is_all_defaults() {
    let got = normalize_title("");
    assert_eq!((got.season, got.part, got.year), (0, 0, 0));
    assert!(got.original.is_empty() && got.normalized.is_empty() && got.tokens.is_empty());
}

#[test]
fn lower_casing_follows_go() {
    // Go maps U+0130 to a plain "i" and has no final-sigma rule.
    assert_eq!(normalize_title("İstanbul").normalized, "istanbul");
    assert_eq!(normalize_title("Bōkoku no Akito").normalized, "boukoku no akito");
    assert_eq!(normalize_title("Yūsha no Kuzu").normalized, "yuusha no kuzu");
}

#[test]
fn token_helpers() {
    let tokens = normalize_title("The Melancholy of Haruhi Suzumiya 2 a").tokens;
    assert_eq!(tokens, ["melancholy", "of", "haruhi", "suzumiya", "2", "a"]);
    assert_eq!(get_significant_tokens(&tokens), ["melancholy", "haruhi", "suzumiya"]);
    assert_eq!(
        significant_tokens_of_title("Attack on Titan OVA"),
        ["attack", "titan"]
    );
    assert_eq!(weighted_token_match_ratio(&["cowboy", "bebop", "1998"], &["cowboy", "bebop"]), 0.8);
    assert_eq!(weighted_token_match_ratio(&[] as &[&str], &["a"]), 0.0);
    assert_eq!(weighted_token_match_ratio(&["a"], &[] as &[&str]), 0.0);
}

#[test]
fn typed_query_builders() {
    assert_eq!(compare_titles("Overlord IV", "Overlord"), 0.5);
    assert_eq!(find_best_match("Overlord II", &["Overlord", "Overlord II"]), "Overlord II");
    assert_eq!(find_best_match("Overlord", &[] as &[&str]), "");
    assert_eq!(sanitize_query("  a (b) [c]  {d} | \"e\"  "), "a b c d e");
    assert_eq!(build_search_query("Attack on Titan Season 2"), "attack titan");
    assert_eq!(build_search_query("The"), "");
    assert_eq!(build_advanced_query(&["Re:Zero", "Re Zero", ""]), "re zero");
    assert_eq!(build_season_query("Overlord", 1), "overlord");
    assert_eq!(build_part_query("Overlord", 10), "(overlord Part 10 | overlord 10th Cour)");
    let smart = build_smart_search_titles(&["Overlord", "OVERLORD", "Overlord IV"]);
    assert_eq!(smart.titles, ["overlord"]);
    assert_eq!((smart.season, smart.part), (4, -1));
}

fn ok(function: &str, args: Value) -> Value {
    call(function, &args).unwrap_or_else(|e| panic!("{function}{args} failed: {e}"))
}

fn err(function: &str, args: Value) -> String {
    match call(function, &args) {
        Ok(value) => panic!("{function}{args} returned {value}"),
        Err(e) => e,
    }
}

#[test]
fn call_normalize_title_uses_seanime_property_names() {
    let got = ok("normalizeTitle", json!(["Attack on Titan Season 3 Part 2 (2019)"]));
    assert_eq!(
        got,
        json!({
            "original": "Attack on Titan Season 3 Part 2 (2019)",
            "normalized": "attack on titan 2019",
            "cleanBaseTitle": "attack on titan",
            "denoisedTitle": "attack titan",
            "tokens": ["attack", "on", "titan", "2019"],
            "season": 3,
            "part": 2,
            "year": 2019,
            "isMain": false,
            "isExtra": false,
        })
    );
    assert_eq!(got.as_object().map(|o| o.len()), Some(10));
}

#[test]
fn call_extractors() {
    assert_eq!(ok("extractPartNumber", json!(["Title Part II"])), json!(2));
    assert_eq!(ok("extractPartNumber", json!(["Title"])), json!(-1));
    assert_eq!(ok("extractSeasonNumber", json!(["Konosuba 2"])), json!(2));
    assert_eq!(ok("extractSeasonNumber", json!(["進撃の巨人 2期"])), json!(2));
    assert_eq!(ok("extractSeasonNumber", json!(["Spy X Family"])), json!(-1));
    assert_eq!(ok("extractYear", json!(["Hunter × Hunter (2011)"])), json!(2011));
    assert_eq!(ok("extractYear", json!(["Hunter × Hunter"])), json!(-1));
}

#[test]
fn call_compare_titles_and_find_best_match() {
    assert_eq!(ok("compareTitles", json!(["Re:Zero", "Re Zero"])).as_f64(), Some(1.0));
    assert_eq!(
        ok("compareTitles", json!(["Kino no Tabi", "Kino's Journey"])).as_f64(),
        Some(0.4347826086956522)
    );
    assert_eq!(ok("compareTitles", json!(["", "Title"])).as_f64(), Some(0.0));
    assert_eq!(
        ok("findBestMatch", json!(["Overlord II", ["Overlord", "Overlord II", "Overlord III"]])),
        json!("Overlord II")
    );
    // Elements that are not strings are skipped; the first candidate wins a tie.
    assert_eq!(ok("findBestMatch", json!(["Bleach", [1, null, "zzz", "yyy"]])), json!("zzz"));
    assert_eq!(ok("findBestMatch", json!(["Bleach", []])), json!(""));
}

#[test]
fn call_get_significant_tokens() {
    assert_eq!(
        ok("getSignificantTokens", json!(["The Melancholy of Haruhi Suzumiya"])),
        json!(["melancholy", "haruhi", "suzumiya"])
    );
    assert_eq!(ok("getSignificantTokens", json!([""])), json!([]));
}

#[test]
fn call_query_builders() {
    assert_eq!(ok("buildSearchQuery", json!(["Attack on Titan Season 2"])), json!("attack titan"));
    assert_eq!(ok("buildSearchQuery", json!(["The"])), json!(""));
    assert_eq!(
        ok(
            "buildAdvancedQuery",
            json!([["Attack on Titan", "Shingeki no Kyojin", "ATTACK ON TITAN", "", 5, null]])
        ),
        json!("(attack titan | shingeki kyojin)")
    );
    assert_eq!(ok("buildAdvancedQuery", json!([["Re:Zero", "Re Zero"]])), json!("re zero"));
    assert_eq!(ok("buildAdvancedQuery", json!([[]])), json!(""));
    assert_eq!(
        ok("sanitizeQuery", json!(["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"])),
        json!("What s this that caret tilde back pipe")
    );
}

/// The Go doc comments show the title's original casing ("Overlord S02", "Re Zero Part 2"),
/// but the code builds the variants from the normalized, lower-cased title.
#[test]
fn call_season_and_part_queries_from_the_go_doc_comments() {
    assert_eq!(
        ok("buildSeasonQuery", json!(["Overlord", 2])),
        json!("(overlord S02 | overlord S2 | overlord Season 2 | overlord 2nd Season)")
    );
    assert_eq!(
        ok("buildPartQuery", json!(["Re:Zero", 2])),
        json!("(re zero Part 2 | re zero Part II | re zero 2nd Cour)")
    );
}

#[test]
fn call_season_and_part_number_conversion() {
    assert_eq!(ok("buildSeasonQuery", json!(["Overlord", 1])), json!("overlord"));
    assert_eq!(ok("buildSeasonQuery", json!(["Overlord", 0])), json!("overlord"));
    assert_eq!(ok("buildSeasonQuery", json!(["Overlord", -3])), json!("overlord"));
    assert_eq!(
        ok("buildSeasonQuery", json!(["Overlord", 2.7])),
        json!("(overlord S02 | overlord S2 | overlord Season 2 | overlord 2nd Season)")
    );
    assert_eq!(
        ok("buildSeasonQuery", json!(["Overlord", "03"])),
        json!("(overlord S03 | overlord S3 | overlord Season 3 | overlord 3rd Season)")
    );
    assert_eq!(
        ok("buildSeasonQuery", json!(["Overlord", 12])),
        json!("(overlord S12 | overlord S12 | overlord Season 12 | overlord 12th Season)")
    );
    assert_eq!(ok("buildSeasonQuery", json!(["Overlord", 1e19])), json!("overlord"));
    assert_eq!(
        ok("buildPartQuery", json!(["Overlord", 10])),
        json!("(overlord Part 10 | overlord 10th Cour)")
    );
    assert_eq!(ok("buildPartQuery", json!(["Overlord", "1"])), json!("overlord"));
}

#[test]
fn call_build_smart_search_titles() {
    assert_eq!(
        ok(
            "buildSmartSearchTitles",
            json!([[
                "Shingeki no Kyojin: The Final Season Part 2",
                "Attack on Titan: Final Season Part 2",
                3,
                ""
            ]])
        ),
        json!({
            "titles": [
                "shingeki kyojin final",
                "shingeki no kyojin final season",
                "shingeki kyojin",
                "attack titan final",
                "attack on titan final season",
                "attack titan"
            ],
            "season": -1,
            "part": 2,
        })
    );
    // An empty list of titles is an empty array in JS, not null.
    assert_eq!(
        ok("buildSmartSearchTitles", json!([[]])),
        json!({ "titles": [], "season": -1, "part": -1 })
    );
    // The prefix before a colon is used only when it is at least 5 bytes long.
    assert_eq!(
        ok("buildSmartSearchTitles", json!([["Show: A", "Shows: A"]]))["titles"],
        json!(["show", "show a", "shows", "shows a"])
    );
}

#[test]
fn call_argument_errors() {
    let one_string = [
        "normalizeTitle",
        "extractPartNumber",
        "extractSeasonNumber",
        "extractYear",
        "getSignificantTokens",
        "buildSearchQuery",
        "sanitizeQuery",
    ];
    for function in one_string {
        assert_eq!(
            err(function, json!([])),
            format!("TypeError: {function} requires at least 1 argument")
        );
        assert_eq!(err(function, json!([5])), "argument is not a string");
        assert_eq!(err(function, json!([null])), "argument is not a string");
    }
    for function in ["buildAdvancedQuery", "buildSmartSearchTitles"] {
        assert_eq!(
            err(function, json!([])),
            format!("TypeError: {function} requires at least 1 argument")
        );
        assert_eq!(err(function, json!(["x"])), "argument 1 is not an array");
        assert_eq!(err(function, json!([{ "a": 1 }])), "argument 1 is not an array");
    }
    for function in ["compareTitles", "findBestMatch", "buildSeasonQuery", "buildPartQuery"] {
        assert_eq!(err(function, json!([])), format!("TypeError: {function} requires 2 arguments"));
        assert_eq!(
            err(function, json!(["x"])),
            format!("TypeError: {function} requires 2 arguments")
        );
        assert_eq!(err(function, json!([5, "y"])), "argument 1 is not a string");
    }
    assert_eq!(err("compareTitles", json!(["x", 5])), "argument 2 is not a string");
    assert_eq!(err("findBestMatch", json!(["x", "y"])), "argument 2 is not an array");
    for function in ["buildSeasonQuery", "buildPartQuery"] {
        for bad in [json!(null), json!(true), json!("abc"), json!("2.5"), json!(" 3"), json!([2])] {
            assert_eq!(err(function, json!(["x", bad])), "argument 2 is not a number");
        }
    }
}

#[test]
fn call_rejects_unknown_functions_and_non_array_arguments() {
    assert!(call("noSuchFunction", &json!([])).is_err());
    assert!(call("normalizeTitle", &json!("Overlord")).is_err());
}

/// JSON equality with numbers compared by value, so `1` and `1.0` are the same.
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same_json(p, q)))
        }
        _ => a == b,
    }
}

fn check_against_go(recorded: &str) -> usize {
    let mut checked = 0;
    let mut failures = Vec::new();
    for line in recorded.lines().filter(|l| !l.trim().is_empty()) {
        let record: Value = serde_json::from_str(line).expect("recorded line is JSON");
        let function = record["fn"].as_str().expect("fn");
        let got = call(function, &record["args"]);
        let matches = match (&got, record.get("ok"), record.get("err")) {
            (Ok(value), Some(expected), None) => same_json(value, expected),
            // Every failure reaches JS from Seanime as the same thrown string, so only the
            // fact of failing is compared.
            (Err(_), None, Some(_)) => true,
            _ => false,
        };
        if !matches {
            failures.push(format!("{line}\n    got {got:?}"));
        }
        checked += 1;
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
    checked
}

#[test]
fn matches_outputs_recorded_from_go() {
    assert_eq!(check_against_go(GO_RECORDED), GO_RECORDED_CASES);
}

const GO_RECORDED_CASES: usize = 655;

/// One call per line as `{fn, args, ok | err}`: Seanime's Go binding run under goja with its
/// `FieldMapper`, each result passed through `JSON.stringify`.
const GO_RECORDED: &str = r#"
{"fn":"normalizeTitle","args":["Attack on Titan"],"ok":{"original":"Attack on Titan","normalized":"attack on titan","cleanBaseTitle":"attack on titan","denoisedTitle":"attack titan","tokens":["attack","on","titan"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Attack on Titan Season 2"],"ok":{"original":"Attack on Titan Season 2","normalized":"attack on titan","cleanBaseTitle":"attack on titan","denoisedTitle":"attack titan","tokens":["attack","on","titan"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Attack on Titan Season 3 Part 2"],"ok":{"original":"Attack on Titan Season 3 Part 2","normalized":"attack on titan","cleanBaseTitle":"attack on titan","denoisedTitle":"attack titan","tokens":["attack","on","titan"],"season":3,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Attack on Titan Season 3 Part 2"],"ok":2}
{"fn":"extractSeasonNumber","args":["Attack on Titan Season 3 Part 2"],"ok":3}
{"fn":"extractYear","args":["Attack on Titan Season 3 Part 2"],"ok":-1}
{"fn":"getSignificantTokens","args":["Attack on Titan Season 3 Part 2"],"ok":["attack","titan"]}
{"fn":"buildSearchQuery","args":["Attack on Titan Season 3 Part 2"],"ok":"attack titan"}
{"fn":"sanitizeQuery","args":["Attack on Titan Season 3 Part 2"],"ok":"Attack on Titan Season 3 Part 2"}
{"fn":"buildSmartSearchTitles","args":[["Attack on Titan Season 3 Part 2"]],"ok":{"titles":["attack titan","attack on titan"],"season":3,"part":2}}
{"fn":"buildAdvancedQuery","args":[["Attack on Titan Season 3 Part 2"]],"ok":"attack titan"}
{"fn":"normalizeTitle","args":["Overlord III"],"ok":{"original":"Overlord III","normalized":"overlord iii","cleanBaseTitle":"overlord","denoisedTitle":"overlord","tokens":["overlord","iii"],"season":3,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Overlord III"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Overlord III"],"ok":3}
{"fn":"extractYear","args":["Overlord III"],"ok":-1}
{"fn":"getSignificantTokens","args":["Overlord III"],"ok":["overlord","iii"]}
{"fn":"buildSearchQuery","args":["Overlord III"],"ok":"overlord"}
{"fn":"sanitizeQuery","args":["Overlord III"],"ok":"Overlord III"}
{"fn":"buildSmartSearchTitles","args":[["Overlord III"]],"ok":{"titles":["overlord"],"season":3,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Overlord III"]],"ok":"overlord"}
{"fn":"normalizeTitle","args":["Attack on Titan Part II"],"ok":{"original":"Attack on Titan Part II","normalized":"attack on titan","cleanBaseTitle":"attack on titan","denoisedTitle":"attack titan","tokens":["attack","on","titan"],"season":-1,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Mushoku Tensei II Part 2"],"ok":{"original":"Mushoku Tensei II Part 2","normalized":"mushoku tensei ii","cleanBaseTitle":"mushoku tensei","denoisedTitle":"mushoku tensei","tokens":["mushoku","tensei","ii"],"season":2,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Mushoku Tensei II Part 2"],"ok":2}
{"fn":"extractSeasonNumber","args":["Mushoku Tensei II Part 2"],"ok":2}
{"fn":"extractYear","args":["Mushoku Tensei II Part 2"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Mushoku Tensei II Part 2"]],"ok":{"titles":["mushoku tensei"],"season":2,"part":2}}
{"fn":"buildAdvancedQuery","args":[["Mushoku Tensei II Part 2"]],"ok":"mushoku tensei"}
{"fn":"normalizeTitle","args":["Steins;Gate"],"ok":{"original":"Steins;Gate","normalized":"steins gate","cleanBaseTitle":"steins gate","denoisedTitle":"steins gate","tokens":["steins","gate"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Kino's Journey"],"ok":{"original":"Kino's Journey","normalized":"kino journey","cleanBaseTitle":"kino journey","denoisedTitle":"kino journey","tokens":["kino","journey"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Persona 4 The Animation"],"ok":{"original":"Persona 4 The Animation","normalized":"persona 4","cleanBaseTitle":"persona","denoisedTitle":"persona","tokens":["persona","4"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["86 - Eighty Six"],"ok":{"original":"86 - Eighty Six","normalized":"86 eighty six","cleanBaseTitle":"86 eighty six","denoisedTitle":"86 eighty six","tokens":["86","eighty","six"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["ATTACK ON TITAN"],"ok":{"original":"ATTACK ON TITAN","normalized":"attack on titan","cleanBaseTitle":"attack on titan","denoisedTitle":"attack titan","tokens":["attack","on","titan"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["The Melancholy of Haruhi Suzumiya"],"ok":{"original":"The Melancholy of Haruhi Suzumiya","normalized":"melancholy of haruhi suzumiya","cleanBaseTitle":"melancholy of haruhi suzumiya","denoisedTitle":"melancholy haruhi suzumiya","tokens":["melancholy","of","haruhi","suzumiya"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["One Piece Episode 1000"],"ok":{"original":"One Piece Episode 1000","normalized":"one piece 1000","cleanBaseTitle":"one piece","denoisedTitle":"one piece","tokens":["one","piece","1000"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Attack on Titan OAD"],"ok":{"original":"Attack on Titan OAD","normalized":"attack on titan ova","cleanBaseTitle":"attack on titan","denoisedTitle":"attack titan","tokens":["attack","on","titan","ova"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Re:Zero"],"ok":{"original":"Re:Zero","normalized":"re zero","cleanBaseTitle":"re zero","denoisedTitle":"re zero","tokens":["re","zero"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Overlord"],"ok":{"original":"Overlord","normalized":"overlord","cleanBaseTitle":"overlord","denoisedTitle":"overlord","tokens":["overlord"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season Part 2"],"ok":{"original":"Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season Part 2","normalized":"re zero kara hajimeru isekai seikatsu","cleanBaseTitle":"re zero kara hajimeru isekai seikatsu","denoisedTitle":"re zero kara hajimeru isekai seikatsu","tokens":["re","zero","kara","hajimeru","isekai","seikatsu"],"season":2,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Shingeki no Kyojin - The Final Season"],"ok":{"original":"Shingeki no Kyojin - The Final Season","normalized":"shingeki no kyojin final season","cleanBaseTitle":"shingeki no kyojin final season","denoisedTitle":"shingeki kyojin final","tokens":["shingeki","no","kyojin","final","season"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Shingeki no Kyojin - The Final Season"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Shingeki no Kyojin - The Final Season"],"ok":-1}
{"fn":"extractYear","args":["Shingeki no Kyojin - The Final Season"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Shingeki no Kyojin - The Final Season"]],"ok":{"titles":["shingeki kyojin final","shingeki no kyojin final season","shingeki kyojin"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Shingeki no Kyojin - The Final Season"]],"ok":"shingeki kyojin final"}
{"fn":"normalizeTitle","args":["Shingeki no Kyojin: The Final Season Part 2"],"ok":{"original":"Shingeki no Kyojin: The Final Season Part 2","normalized":"shingeki no kyojin final season","cleanBaseTitle":"shingeki no kyojin final season","denoisedTitle":"shingeki kyojin final","tokens":["shingeki","no","kyojin","final","season"],"season":-1,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Shingeki no Kyojin: The Final Season Part 2"],"ok":2}
{"fn":"extractSeasonNumber","args":["Shingeki no Kyojin: The Final Season Part 2"],"ok":-1}
{"fn":"extractYear","args":["Shingeki no Kyojin: The Final Season Part 2"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Shingeki no Kyojin: The Final Season Part 2"]],"ok":{"titles":["shingeki kyojin final","shingeki no kyojin final season","shingeki kyojin"],"season":-1,"part":2}}
{"fn":"buildAdvancedQuery","args":[["Shingeki no Kyojin: The Final Season Part 2"]],"ok":"shingeki kyojin final"}
{"fn":"normalizeTitle","args":["JJK season 2"],"ok":{"original":"JJK season 2","normalized":"jjk","cleanBaseTitle":"jjk","denoisedTitle":"jjk","tokens":["jjk"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Spy x Family 2nd Season"],"ok":{"original":"Spy x Family 2nd Season","normalized":"spy x family","cleanBaseTitle":"spy x family","denoisedTitle":"spy x family","tokens":["spy","x","family"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Spy X Family"],"ok":{"original":"Spy X Family","normalized":"spy x family","cleanBaseTitle":"spy x family","denoisedTitle":"spy x family","tokens":["spy","x","family"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Diamond no Ace Act II"],"ok":{"original":"Diamond no Ace Act II","normalized":"diamond no ace act ii","cleanBaseTitle":"diamond no ace act","denoisedTitle":"diamond ace act","tokens":["diamond","no","ace","act","ii"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Diamond no Ace Act II Second Season"],"ok":{"original":"Diamond no Ace Act II Second Season","normalized":"diamond no ace act ii","cleanBaseTitle":"diamond no ace act","denoisedTitle":"diamond ace act","tokens":["diamond","no","ace","act","ii"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Diamond no Ace Act II Second Season"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Diamond no Ace Act II Second Season"],"ok":2}
{"fn":"extractYear","args":["Diamond no Ace Act II Second Season"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Diamond no Ace Act II Second Season"]],"ok":{"titles":["diamond ace act","diamond no ace act"],"season":2,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Diamond no Ace Act II Second Season"]],"ok":"diamond ace act"}
{"fn":"normalizeTitle","args":["Attack on Titan Cour II"],"ok":{"original":"Attack on Titan Cour II","normalized":"attack on titan","cleanBaseTitle":"attack on titan","denoisedTitle":"attack titan","tokens":["attack","on","titan"],"season":-1,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["This is a test"],"ok":{"original":"This is a test","normalized":"this is a test","cleanBaseTitle":"this is a test","denoisedTitle":"test","tokens":["this","is","a","test"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Konosuba 2"],"ok":{"original":"Konosuba 2","normalized":"konosuba 2","cleanBaseTitle":"konosuba","denoisedTitle":"konosuba","tokens":["konosuba","2"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Konosuba 2"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Konosuba 2"],"ok":2}
{"fn":"extractYear","args":["Konosuba 2"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Konosuba 2"]],"ok":{"titles":["konosuba"],"season":2,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Konosuba 2"]],"ok":"konosuba"}
{"fn":"normalizeTitle","args":["KonoSuba: God's Blessing on This Wonderful World! 3"],"ok":{"original":"KonoSuba: God's Blessing on This Wonderful World! 3","normalized":"konosuba god blessing on this wonderful world 3","cleanBaseTitle":"konosuba god blessing on this wonderful world","denoisedTitle":"konosuba god blessing wonderful world","tokens":["konosuba","god","blessing","on","this","wonderful","world","3"],"season":3,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Mob Psycho 100 III"],"ok":{"original":"Mob Psycho 100 III","normalized":"mob psycho 100 iii","cleanBaseTitle":"mob psycho 100","denoisedTitle":"mob psycho 100","tokens":["mob","psycho","100","iii"],"season":3,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Mob Psycho 100"],"ok":{"original":"Mob Psycho 100","normalized":"mob psycho 100","cleanBaseTitle":"mob psycho","denoisedTitle":"mob psycho","tokens":["mob","psycho","100"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Bakemonogatari (2009)"],"ok":{"original":"Bakemonogatari (2009)","normalized":"bakemonogatari 2009","cleanBaseTitle":"bakemonogatari","denoisedTitle":"bakemonogatari","tokens":["bakemonogatari","2009"],"season":-1,"part":-1,"year":2009,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Fullmetal Alchemist: Brotherhood"],"ok":{"original":"Fullmetal Alchemist: Brotherhood","normalized":"fullmetal alchemist brotherhood","cleanBaseTitle":"fullmetal alchemist brotherhood","denoisedTitle":"fullmetal alchemist brotherhood","tokens":["fullmetal","alchemist","brotherhood"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Hunter × Hunter (2011)"],"ok":{"original":"Hunter × Hunter (2011)","normalized":"hunter x hunter 2011","cleanBaseTitle":"hunter x hunter","denoisedTitle":"hunter x hunter","tokens":["hunter","x","hunter","2011"],"season":-1,"part":-1,"year":2011,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Hunter × Hunter (2011)"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Hunter × Hunter (2011)"],"ok":-1}
{"fn":"extractYear","args":["Hunter × Hunter (2011)"],"ok":2011}
{"fn":"getSignificantTokens","args":["Hunter × Hunter (2011)"],"ok":["hunter","hunter","2011"]}
{"fn":"buildSearchQuery","args":["Hunter × Hunter (2011)"],"ok":"hunter x hunter"}
{"fn":"sanitizeQuery","args":["Hunter × Hunter (2011)"],"ok":"Hunter × Hunter 2011"}
{"fn":"buildSmartSearchTitles","args":[["Hunter × Hunter (2011)"]],"ok":{"titles":["hunter x hunter"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Hunter × Hunter (2011)"]],"ok":"hunter x hunter"}
{"fn":"normalizeTitle","args":["Sword Art Online: Alicization - War of Underworld 2nd Cour"],"ok":{"original":"Sword Art Online: Alicization - War of Underworld 2nd Cour","normalized":"sword art online alicization war of underworld","cleanBaseTitle":"sword art online alicization war of underworld","denoisedTitle":"sword art online alicization war underworld","tokens":["sword","art","online","alicization","war","of","underworld"],"season":-1,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Kaguya-sama wa Kokurasetai: Tensai-tachi no Ren'ai Zunousen"],"ok":{"original":"Kaguya-sama wa Kokurasetai: Tensai-tachi no Ren'ai Zunousen","normalized":"kaguya sama wa kokurasetai tensai tachi no renai zunousen","cleanBaseTitle":"kaguya sama wa kokurasetai tensai tachi no renai zunousen","denoisedTitle":"kaguya sama kokurasetai tensai tachi renai zunousen","tokens":["kaguya","sama","wa","kokurasetai","tensai","tachi","no","renai","zunousen"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Bōkoku no Akito"],"ok":{"original":"Bōkoku no Akito","normalized":"boukoku no akito","cleanBaseTitle":"boukoku no akito","denoisedTitle":"boukoku akito","tokens":["boukoku","no","akito"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Yūsha no Kuzu"],"ok":{"original":"Yūsha no Kuzu","normalized":"yuusha no kuzu","cleanBaseTitle":"yuusha no kuzu","denoisedTitle":"yuusha kuzu","tokens":["yuusha","no","kuzu"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["D.Gray-man"],"ok":{"original":"D.Gray-man","normalized":"d gray man","cleanBaseTitle":"d gray man","denoisedTitle":"d gray man","tokens":["d","gray","man"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Fate/stay night [Unlimited Blade Works] 2nd Season"],"ok":{"original":"Fate/stay night [Unlimited Blade Works] 2nd Season","normalized":"fate stay night unlimited blade works","cleanBaseTitle":"fate stay night unlimited blade works","denoisedTitle":"fate stay night unlimited blade works","tokens":["fate","stay","night","unlimited","blade","works"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Fate/Zero S2"],"ok":{"original":"Fate/Zero S2","normalized":"fate zero","cleanBaseTitle":"fate zero","denoisedTitle":"fate zero","tokens":["fate","zero"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Fate/Zero S2"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Fate/Zero S2"],"ok":2}
{"fn":"extractYear","args":["Fate/Zero S2"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Fate/Zero S2"]],"ok":{"titles":["fate zero"],"season":2,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Fate/Zero S2"]],"ok":"fate zero"}
{"fn":"normalizeTitle","args":["[SubsPlease] Sousou no Frieren - 28 (1080p) [ABCD1234].mkv"],"ok":{"original":"[SubsPlease] Sousou no Frieren - 28 (1080p) [ABCD1234].mkv","normalized":"subsplease sousou no frieren 28 1080p abcd1234 mkv","cleanBaseTitle":"subsplease sousou no frieren 28 1080p abcd1234 mkv","denoisedTitle":"subsplease sousou frieren 28 1080p abcd1234 mkv","tokens":["subsplease","sousou","no","frieren","28","1080p","abcd1234","mkv"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Dr. Stone S03E05"],"ok":{"original":"Dr. Stone S03E05","normalized":"dr stone s03e05","cleanBaseTitle":"dr stone s03e05","denoisedTitle":"dr stone s03e05","tokens":["dr","stone","s03e05"],"season":3,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Dr. Stone S03E05"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Dr. Stone S03E05"],"ok":3}
{"fn":"extractYear","args":["Dr. Stone S03E05"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Dr. Stone S03E05"]],"ok":{"titles":["dr stone s03e05"],"season":3,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Dr. Stone S03E05"]],"ok":"dr stone s03e05"}
{"fn":"normalizeTitle","args":["Dr. Stone: New World Part 2"],"ok":{"original":"Dr. Stone: New World Part 2","normalized":"dr stone new world","cleanBaseTitle":"dr stone new world","denoisedTitle":"dr stone new world","tokens":["dr","stone","new","world"],"season":-1,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Tate no Yuusha no Nariagari Season 3"],"ok":{"original":"Tate no Yuusha no Nariagari Season 3","normalized":"tate no yuusha no nariagari","cleanBaseTitle":"tate no yuusha no nariagari","denoisedTitle":"tate yuusha nariagari","tokens":["tate","no","yuusha","no","nariagari"],"season":3,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["進撃の巨人 第2期"],"ok":{"original":"進撃の巨人 第2期","normalized":"2","cleanBaseTitle":"","denoisedTitle":"","tokens":["2"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["進撃の巨人 第2期"],"ok":-1}
{"fn":"extractSeasonNumber","args":["進撃の巨人 第2期"],"ok":2}
{"fn":"extractYear","args":["進撃の巨人 第2期"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["進撃の巨人 第2期"]],"ok":{"titles":["2"],"season":2,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["進撃の巨人 第2期"]],"ok":"2"}
{"fn":"normalizeTitle","args":["進撃の巨人 2期"],"ok":{"original":"進撃の巨人 2期","normalized":"2","cleanBaseTitle":"","denoisedTitle":"","tokens":["2"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["僕のヒーローアカデミア 3"],"ok":{"original":"僕のヒーローアカデミア 3","normalized":"3","cleanBaseTitle":"","denoisedTitle":"","tokens":["3"],"season":3,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Boku no Hero Academia 7"],"ok":{"original":"Boku no Hero Academia 7","normalized":"boku no hero academia 7","cleanBaseTitle":"boku no hero academia","denoisedTitle":"boku hero academia","tokens":["boku","no","hero","academia","7"],"season":7,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["K-ON!!"],"ok":{"original":"K-ON!!","normalized":"k on","cleanBaseTitle":"k on","denoisedTitle":"k","tokens":["k","on"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["K-ON! Movie"],"ok":{"original":"K-ON! Movie","normalized":"k on movie","cleanBaseTitle":"k on","denoisedTitle":"k","tokens":["k","on","movie"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Clannad ~After Story~"],"ok":{"original":"Clannad ~After Story~","normalized":"clannad after story","cleanBaseTitle":"clannad after story","denoisedTitle":"clannad after story","tokens":["clannad","after","story"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["The iDOLM@STER"],"ok":{"original":"The iDOLM@STER","normalized":"idolmaster","cleanBaseTitle":"idolmaster","denoisedTitle":"idolmaster","tokens":["idolmaster"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Black★Rock Shooter"],"ok":{"original":"Black★Rock Shooter","normalized":"black rock shooter","cleanBaseTitle":"black rock shooter","denoisedTitle":"black rock shooter","tokens":["black","rock","shooter"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Code Geass: Hangyaku no Lelouch R2"],"ok":{"original":"Code Geass: Hangyaku no Lelouch R2","normalized":"code geass hangyaku no lelouch r2","cleanBaseTitle":"code geass hangyaku no lelouch r2","denoisedTitle":"code geass hangyaku lelouch r2","tokens":["code","geass","hangyaku","no","lelouch","r2"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Gintama'"],"ok":{"original":"Gintama'","normalized":"gintama","cleanBaseTitle":"gintama","denoisedTitle":"gintama","tokens":["gintama"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Gintama°"],"ok":{"original":"Gintama°","normalized":"gintama","cleanBaseTitle":"gintama","denoisedTitle":"gintama","tokens":["gintama"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Gintama."],"ok":{"original":"Gintama.","normalized":"gintama","cleanBaseTitle":"gintama","denoisedTitle":"gintama","tokens":["gintama"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Haikyuu!! Second Season"],"ok":{"original":"Haikyuu!! Second Season","normalized":"haikyuu","cleanBaseTitle":"haikyuu","denoisedTitle":"haikyuu","tokens":["haikyuu"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Haikyuu!! Third Season"],"ok":{"original":"Haikyuu!! Third Season","normalized":"haikyuu","cleanBaseTitle":"haikyuu","denoisedTitle":"haikyuu","tokens":["haikyuu"],"season":3,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Haikyuu!! Third Season"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Haikyuu!! Third Season"],"ok":3}
{"fn":"extractYear","args":["Haikyuu!! Third Season"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Haikyuu!! Third Season"]],"ok":{"titles":["haikyuu"],"season":3,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Haikyuu!! Third Season"]],"ok":"haikyuu"}
{"fn":"normalizeTitle","args":["Vinland Saga Season 02"],"ok":{"original":"Vinland Saga Season 02","normalized":"vinland saga","cleanBaseTitle":"vinland saga","denoisedTitle":"vinland saga","tokens":["vinland","saga"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Bleach: Sennen Kessen-hen - Ketsubetsu-tan"],"ok":{"original":"Bleach: Sennen Kessen-hen - Ketsubetsu-tan","normalized":"bleach sennen kessen hen ketsubetsu tan","cleanBaseTitle":"bleach sennen kessen hen ketsubetsu tan","denoisedTitle":"bleach sennen kessen hen ketsubetsu tan","tokens":["bleach","sennen","kessen","hen","ketsubetsu","tan"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Bleach: Sennen Kessen-hen - Ketsubetsu-tan"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Bleach: Sennen Kessen-hen - Ketsubetsu-tan"],"ok":-1}
{"fn":"extractYear","args":["Bleach: Sennen Kessen-hen - Ketsubetsu-tan"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Bleach: Sennen Kessen-hen - Ketsubetsu-tan"]],"ok":{"titles":["bleach sennen kessen hen ketsubetsu tan","bleach","bleach sennen kessen hen"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Bleach: Sennen Kessen-hen - Ketsubetsu-tan"]],"ok":"bleach sennen kessen hen ketsubetsu tan"}
{"fn":"normalizeTitle","args":["Bleach: Thousand-Year Blood War Part 3"],"ok":{"original":"Bleach: Thousand-Year Blood War Part 3","normalized":"bleach thousand year blood war","cleanBaseTitle":"bleach thousand year blood war","denoisedTitle":"bleach thousand year blood war","tokens":["bleach","thousand","year","blood","war"],"season":-1,"part":3,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["JoJo's Bizarre Adventure Part 5: Golden Wind"],"ok":{"original":"JoJo's Bizarre Adventure Part 5: Golden Wind","normalized":"jojo bizarre adventure golden wind","cleanBaseTitle":"jojo bizarre adventure golden wind","denoisedTitle":"jojo bizarre adventure golden wind","tokens":["jojo","bizarre","adventure","golden","wind"],"season":-1,"part":5,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Monogatari Series: Second Season"],"ok":{"original":"Monogatari Series: Second Season","normalized":"monogatari series","cleanBaseTitle":"monogatari series","denoisedTitle":"monogatari series","tokens":["monogatari","series"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Tokyo Ghoul √A"],"ok":{"original":"Tokyo Ghoul √A","normalized":"tokyo ghoul a","cleanBaseTitle":"tokyo ghoul a","denoisedTitle":"tokyo ghoul","tokens":["tokyo","ghoul","a"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Tokyo Ghoul:re 2nd Season"],"ok":{"original":"Tokyo Ghoul:re 2nd Season","normalized":"tokyo ghoul re","cleanBaseTitle":"tokyo ghoul re","denoisedTitle":"tokyo ghoul re","tokens":["tokyo","ghoul","re"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["  Spaced   Out   Title  "],"ok":{"original":"  Spaced   Out   Title  ","normalized":"spaced out title","cleanBaseTitle":"spaced out title","denoisedTitle":"spaced out title","tokens":["spaced","out","title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["The"],"ok":{"original":"The","normalized":"","cleanBaseTitle":"","denoisedTitle":"","tokens":[],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["The"],"ok":-1}
{"fn":"extractSeasonNumber","args":["The"],"ok":-1}
{"fn":"extractYear","args":["The"],"ok":-1}
{"fn":"getSignificantTokens","args":["The"],"ok":[]}
{"fn":"buildSearchQuery","args":["The"],"ok":""}
{"fn":"sanitizeQuery","args":["The"],"ok":"The"}
{"fn":"buildSmartSearchTitles","args":[["The"]],"ok":{"titles":[],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["The"]],"ok":""}
{"fn":"normalizeTitle","args":["the animation"],"ok":{"original":"the animation","normalized":"","cleanBaseTitle":"","denoisedTitle":"","tokens":[],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["!!!"],"ok":{"original":"!!!","normalized":"","cleanBaseTitle":"","denoisedTitle":"","tokens":[],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["!!!"],"ok":-1}
{"fn":"extractSeasonNumber","args":["!!!"],"ok":-1}
{"fn":"extractYear","args":["!!!"],"ok":-1}
{"fn":"getSignificantTokens","args":["!!!"],"ok":[]}
{"fn":"buildSearchQuery","args":["!!!"],"ok":""}
{"fn":"sanitizeQuery","args":["!!!"],"ok":""}
{"fn":"buildSmartSearchTitles","args":[["!!!"]],"ok":{"titles":[],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["!!!"]],"ok":""}
{"fn":"normalizeTitle","args":["2"],"ok":{"original":"2","normalized":"2","cleanBaseTitle":"","denoisedTitle":"","tokens":["2"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["1999"],"ok":{"original":"1999","normalized":"1999","cleanBaseTitle":"","denoisedTitle":"","tokens":["1999"],"season":-1,"part":-1,"year":1999,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Blade Runner 2049"],"ok":{"original":"Blade Runner 2049","normalized":"blade runner 2049","cleanBaseTitle":"blade runner","denoisedTitle":"blade runner","tokens":["blade","runner","2049"],"season":-1,"part":-1,"year":2049,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Blade Runner 2049"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Blade Runner 2049"],"ok":-1}
{"fn":"extractYear","args":["Blade Runner 2049"],"ok":2049}
{"fn":"buildSmartSearchTitles","args":[["Blade Runner 2049"]],"ok":{"titles":["blade runner"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Blade Runner 2049"]],"ok":"blade runner"}
{"fn":"normalizeTitle","args":["Cowboy Bebop 1998"],"ok":{"original":"Cowboy Bebop 1998","normalized":"cowboy bebop 1998","cleanBaseTitle":"cowboy bebop","denoisedTitle":"cowboy bebop","tokens":["cowboy","bebop","1998"],"season":-1,"part":-1,"year":1998,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Space Dandy (3000)"],"ok":{"original":"Space Dandy (3000)","normalized":"space dandy 3000","cleanBaseTitle":"space dandy","denoisedTitle":"space dandy","tokens":["space","dandy","3000"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Space Dandy (3000)"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Space Dandy (3000)"],"ok":-1}
{"fn":"extractYear","args":["Space Dandy (3000)"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Space Dandy (3000)"]],"ok":{"titles":["space dandy"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Space Dandy (3000)"]],"ok":"space dandy"}
{"fn":"normalizeTitle","args":["Show (1899) 2015"],"ok":{"original":"Show (1899) 2015","normalized":"show 1899 2015","cleanBaseTitle":"show 1899","denoisedTitle":"show 1899","tokens":["show","1899","2015"],"season":-1,"part":-1,"year":2015,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Show (1899) 2015"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Show (1899) 2015"],"ok":-1}
{"fn":"extractYear","args":["Show (1899) 2015"],"ok":2015}
{"fn":"buildSmartSearchTitles","args":[["Show (1899) 2015"]],"ok":{"titles":["show 1899"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Show (1899) 2015"]],"ok":"show 1899"}
{"fn":"normalizeTitle","args":["Macross 7"],"ok":{"original":"Macross 7","normalized":"macross 7","cleanBaseTitle":"macross","denoisedTitle":"macross","tokens":["macross","7"],"season":7,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Gundam 00 S2"],"ok":{"original":"Gundam 00 S2","normalized":"gundam 00","cleanBaseTitle":"gundam","denoisedTitle":"gundam","tokens":["gundam","00"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Rock & Roll"],"ok":{"original":"Rock & Roll","normalized":"rock and roll","cleanBaseTitle":"rock and roll","denoisedTitle":"rock and roll","tokens":["rock","and","roll"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Rock & Roll"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Rock & Roll"],"ok":-1}
{"fn":"extractYear","args":["Rock & Roll"],"ok":-1}
{"fn":"getSignificantTokens","args":["Rock & Roll"],"ok":["rock","and","roll"]}
{"fn":"buildSearchQuery","args":["Rock & Roll"],"ok":"rock and roll"}
{"fn":"sanitizeQuery","args":["Rock & Roll"],"ok":"Rock & Roll"}
{"fn":"buildSmartSearchTitles","args":[["Rock & Roll"]],"ok":{"titles":["rock and roll"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Rock & Roll"]],"ok":"rock and roll"}
{"fn":"normalizeTitle","args":["Rock&Roll"],"ok":{"original":"Rock&Roll","normalized":"rock roll","cleanBaseTitle":"rock roll","denoisedTitle":"rock roll","tokens":["rock","roll"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Mother’s Day “Special”"],"ok":{"original":"Mother’s Day “Special”","normalized":"mother day sp","cleanBaseTitle":"mother day","denoisedTitle":"mother day","tokens":["mother","day","sp"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["`quoted` title"],"ok":{"original":"`quoted` title","normalized":"quoted title","cleanBaseTitle":"quoted title","denoisedTitle":"quoted title","tokens":["quoted","title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title＊Star"],"ok":{"original":"Title＊Star","normalized":"title star","cleanBaseTitle":"title star","denoisedTitle":"title star","tokens":["title","star"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title꞉Colon"],"ok":{"original":"Title꞉Colon","normalized":"title colon","cleanBaseTitle":"title colon","denoisedTitle":"title colon","tokens":["title","colon"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Specials"],"ok":{"original":"Specials","normalized":"sp","cleanBaseTitle":"","denoisedTitle":"","tokens":["sp"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Special 2"],"ok":{"original":"Special 2","normalized":"sp 2","cleanBaseTitle":"","denoisedTitle":"","tokens":["sp","2"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Show SP 1"],"ok":{"original":"Show SP 1","normalized":"show sp 1","cleanBaseTitle":"show","denoisedTitle":"show","tokens":["show","sp","1"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Show OAV"],"ok":{"original":"Show OAV","normalized":"show ova","cleanBaseTitle":"show","denoisedTitle":"show","tokens":["show","ova"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Theatre of Darkness"],"ok":{"original":"Theatre of Darkness","normalized":"theatre of darkness","cleanBaseTitle":"theatre of darkness","denoisedTitle":"theatre darkness","tokens":["theatre","of","darkness"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Another"],"ok":{"original":"Another","normalized":"another","cleanBaseTitle":"another","denoisedTitle":"another","tokens":["another"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Bathe"],"ok":{"original":"Bathe","normalized":"bathe","cleanBaseTitle":"bathe","denoisedTitle":"bathe","tokens":["bathe"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Episode"],"ok":{"original":"Episode","normalized":"","cleanBaseTitle":"","denoisedTitle":"","tokens":[],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Seasons"],"ok":{"original":"Seasons","normalized":"seasons","cleanBaseTitle":"seasons","denoisedTitle":"seasons","tokens":["seasons"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["İstanbul Σ ΟΔΟΣ"],"ok":{"original":"İstanbul Σ ΟΔΟΣ","normalized":"istanbul","cleanBaseTitle":"istanbul","denoisedTitle":"istanbul","tokens":["istanbul"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["ẞtraße Ünïcödé"],"ok":{"original":"ẞtraße Ünïcödé","normalized":"tra e n c d","cleanBaseTitle":"tra e n c d","denoisedTitle":"tra n c d","tokens":["tra","e","n","c","d"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title\tWith\tTabs"],"ok":{"original":"Title\tWith\tTabs","normalized":"title with tabs","cleanBaseTitle":"title with tabs","denoisedTitle":"title tabs","tokens":["title","with","tabs"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title NBSP: Sub"],"ok":{"original":"Title NBSP: Sub","normalized":"title nbsp sub","cleanBaseTitle":"title nbsp sub","denoisedTitle":"title nbsp sub","tokens":["title","nbsp","sub"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title NBSP: Sub"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Title NBSP: Sub"],"ok":-1}
{"fn":"extractYear","args":["Title NBSP: Sub"],"ok":-1}
{"fn":"getSignificantTokens","args":["Title NBSP: Sub"],"ok":["title","nbsp","sub"]}
{"fn":"buildSearchQuery","args":["Title NBSP: Sub"],"ok":"title nbsp sub"}
{"fn":"sanitizeQuery","args":["Title NBSP: Sub"],"ok":"Title NBSP: Sub"}
{"fn":"buildSmartSearchTitles","args":[["Title NBSP: Sub"]],"ok":{"titles":["title nbsp sub","title nbsp"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Title NBSP: Sub"]],"ok":"title nbsp sub"}
{"fn":"normalizeTitle","args":["Title Part I"],"ok":{"original":"Title Part I","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title Part X"],"ok":{"original":"Title Part X","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title Part IV"],"ok":{"original":"Title Part IV","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":4,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title Part IV"],"ok":4}
{"fn":"extractSeasonNumber","args":["Title Part IV"],"ok":-1}
{"fn":"extractYear","args":["Title Part IV"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Title Part IV"]],"ok":{"titles":["title"],"season":-1,"part":4}}
{"fn":"buildAdvancedQuery","args":[["Title Part IV"]],"ok":"title"}
{"fn":"normalizeTitle","args":["Title Cour 02"],"ok":{"original":"Title Cour 02","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title 3rd Part"],"ok":{"original":"Title 3rd Part","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":3,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title 3rd Part"],"ok":3}
{"fn":"extractSeasonNumber","args":["Title 3rd Part"],"ok":-1}
{"fn":"extractYear","args":["Title 3rd Part"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Title 3rd Part"]],"ok":{"titles":["title"],"season":-1,"part":3}}
{"fn":"buildAdvancedQuery","args":[["Title 3rd Part"]],"ok":"title"}
{"fn":"normalizeTitle","args":["Title 4th Season"],"ok":{"original":"Title 4th Season","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":4,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title Series 2"],"ok":{"original":"Title Series 2","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title 2nd Series"],"ok":{"original":"Title 2nd Series","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title X"],"ok":{"original":"Title X","normalized":"title x","cleanBaseTitle":"title x","denoisedTitle":"title x","tokens":["title","x"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title X"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Title X"],"ok":-1}
{"fn":"extractYear","args":["Title X"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Title X"]],"ok":{"titles":["title x"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Title X"]],"ok":"title x"}
{"fn":"normalizeTitle","args":["Title IX"],"ok":{"original":"Title IX","normalized":"title ix","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","ix"],"season":9,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title XI"],"ok":{"original":"Title XI","normalized":"title xi","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","xi"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title II: Subtitle"],"ok":{"original":"Title II: Subtitle","normalized":"title ii subtitle","cleanBaseTitle":"title subtitle","denoisedTitle":"title subtitle","tokens":["title","ii","subtitle"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title.III.S01E02"],"ok":{"original":"Title.III.S01E02","normalized":"title iii s01e02","cleanBaseTitle":"title s01e02","denoisedTitle":"title s01e02","tokens":["title","iii","s01e02"],"season":1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title.III.S01E02"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Title.III.S01E02"],"ok":1}
{"fn":"extractYear","args":["Title.III.S01E02"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Title.III.S01E02"]],"ok":{"titles":["title s01e02"],"season":1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Title.III.S01E02"]],"ok":"title s01e02"}
{"fn":"normalizeTitle","args":["Title vi part 2"],"ok":{"original":"Title vi part 2","normalized":"title vi","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","vi"],"season":6,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title Season 21"],"ok":{"original":"Title Season 21","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":21,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title S25"],"ok":{"original":"Title S25","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title S25"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Title S25"],"ok":-1}
{"fn":"extractYear","args":["Title S25"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Title S25"]],"ok":{"titles":["title"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Title S25"]],"ok":"title"}
{"fn":"normalizeTitle","args":["Title S19"],"ok":{"original":"Title S19","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":19,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title Part 99999999999999999999"],"ok":{"original":"Title Part 99999999999999999999","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title Part 99999999999999999999"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Title Part 99999999999999999999"],"ok":-1}
{"fn":"extractYear","args":["Title Part 99999999999999999999"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Title Part 99999999999999999999"]],"ok":{"titles":["title"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Title Part 99999999999999999999"]],"ok":"title"}
{"fn":"normalizeTitle","args":["Title Season 99999999999999999999"],"ok":{"original":"Title Season 99999999999999999999","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title Season 99999999999999999999"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Title Season 99999999999999999999"],"ok":-1}
{"fn":"extractYear","args":["Title Season 99999999999999999999"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Title Season 99999999999999999999"]],"ok":{"titles":["title"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Title Season 99999999999999999999"]],"ok":"title"}
{"fn":"normalizeTitle","args":["Title 11"],"ok":{"original":"Title 11","normalized":"title 11","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","11"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title 11"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Title 11"],"ok":-1}
{"fn":"extractYear","args":["Title 11"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["Title 11"]],"ok":{"titles":["title"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Title 11"]],"ok":"title"}
{"fn":"normalizeTitle","args":["Title 10"],"ok":{"original":"Title 10","normalized":"title 10","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","10"],"season":10,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title 1"],"ok":{"original":"Title 1","normalized":"title 1","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","1"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title Movie 2"],"ok":{"original":"Title Movie 2","normalized":"title movie 2","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","movie","2"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Title Movie 2"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Title Movie 2"],"ok":-1}
{"fn":"extractYear","args":["Title Movie 2"],"ok":-1}
{"fn":"getSignificantTokens","args":["Title Movie 2"],"ok":["title"]}
{"fn":"buildSearchQuery","args":["Title Movie 2"],"ok":"title"}
{"fn":"sanitizeQuery","args":["Title Movie 2"],"ok":"Title Movie 2"}
{"fn":"buildSmartSearchTitles","args":[["Title Movie 2"]],"ok":{"titles":["title"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Title Movie 2"]],"ok":"title"}
{"fn":"normalizeTitle","args":["Title OVA 3"],"ok":{"original":"Title OVA 3","normalized":"title ova 3","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","ova","3"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Show: A"],"ok":{"original":"Show: A","normalized":"show a","cleanBaseTitle":"show a","denoisedTitle":"show","tokens":["show","a"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Shows: A Long Subtitle"],"ok":{"original":"Shows: A Long Subtitle","normalized":"shows a long subtitle","cleanBaseTitle":"shows a long subtitle","denoisedTitle":"shows long subtitle","tokens":["shows","a","long","subtitle"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Ab - Cd"],"ok":{"original":"Ab - Cd","normalized":"ab cd","cleanBaseTitle":"ab cd","denoisedTitle":"ab cd","tokens":["ab","cd"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Abcde - Fghij"],"ok":{"original":"Abcde - Fghij","normalized":"abcde fghij","cleanBaseTitle":"abcde fghij","denoisedTitle":"abcde fghij","tokens":["abcde","fghij"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["日本語: タイトル"],"ok":{"original":"日本語: タイトル","normalized":"","cleanBaseTitle":"","denoisedTitle":"","tokens":[],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["日本語: タイトル"],"ok":-1}
{"fn":"extractSeasonNumber","args":["日本語: タイトル"],"ok":-1}
{"fn":"extractYear","args":["日本語: タイトル"],"ok":-1}
{"fn":"buildSmartSearchTitles","args":[["日本語: タイトル"]],"ok":{"titles":[],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["日本語: タイトル"]],"ok":""}
{"fn":"normalizeTitle","args":["Äb: cd"],"ok":{"original":"Äb: cd","normalized":"b cd","cleanBaseTitle":"b cd","denoisedTitle":"b cd","tokens":["b","cd"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["Äb: cd"],"ok":-1}
{"fn":"extractSeasonNumber","args":["Äb: cd"],"ok":-1}
{"fn":"extractYear","args":["Äb: cd"],"ok":-1}
{"fn":"getSignificantTokens","args":["Äb: cd"],"ok":["cd"]}
{"fn":"buildSearchQuery","args":["Äb: cd"],"ok":"b cd"}
{"fn":"sanitizeQuery","args":["Äb: cd"],"ok":"Äb: cd"}
{"fn":"buildSmartSearchTitles","args":[["Äb: cd"]],"ok":{"titles":["b cd"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["Äb: cd"]],"ok":"b cd"}
{"fn":"normalizeTitle","args":["Fate/Grand Order: Zettai Majuu Sensen Babylonia"],"ok":{"original":"Fate/Grand Order: Zettai Majuu Sensen Babylonia","normalized":"fate grand order zettai majuu sensen babylonia","cleanBaseTitle":"fate grand order zettai majuu sensen babylonia","denoisedTitle":"fate grand order zettai majuu sensen babylonia","tokens":["fate","grand","order","zettai","majuu","sensen","babylonia"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["a"],"ok":{"original":"a","normalized":"a","cleanBaseTitle":"a","denoisedTitle":"","tokens":["a"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["e"],"ok":{"original":"e","normalized":"e","cleanBaseTitle":"e","denoisedTitle":"","tokens":["e"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["no"],"ok":{"original":"no","normalized":"no","cleanBaseTitle":"no","denoisedTitle":"","tokens":["no"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Uncensored BluRay Finale"],"ok":{"original":"Uncensored BluRay Finale","normalized":"uncensored bluray finale","cleanBaseTitle":"uncensored bluray","denoisedTitle":"","tokens":["uncensored","bluray","finale"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["A Certain Scientific Railgun T"],"ok":{"original":"A Certain Scientific Railgun T","normalized":"a certain scientific railgun t","cleanBaseTitle":"a certain scientific railgun t","denoisedTitle":"certain scientific railgun t","tokens":["a","certain","scientific","railgun","t"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["To LOVE-Ru Darkness 2nd"],"ok":{"original":"To LOVE-Ru Darkness 2nd","normalized":"to love ru darkness 2nd","cleanBaseTitle":"to love ru darkness 2nd","denoisedTitle":"love ru darkness 2nd","tokens":["to","love","ru","darkness","2nd"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title (TV)"],"ok":{"original":"Title (TV)","normalized":"title","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title (tv) 2"],"ok":{"original":"Title (tv) 2","normalized":"title 2","cleanBaseTitle":"title","denoisedTitle":"title","tokens":["title","2"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title [1080p] {x265} (BD)"],"ok":{"original":"Title [1080p] {x265} (BD)","normalized":"title 1080p x265 bd","cleanBaseTitle":"title 1080p x265 bd","denoisedTitle":"title 1080p x265 bd","tokens":["title","1080p","x265","bd"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"],"ok":{"original":"What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe","normalized":"what this that caret tilde back pipe","cleanBaseTitle":"what this that caret tilde back pipe","denoisedTitle":"what caret tilde back pipe","tokens":["what","this","that","caret","tilde","back","pipe"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"extractPartNumber","args":["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"],"ok":-1}
{"fn":"extractSeasonNumber","args":["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"],"ok":-1}
{"fn":"extractYear","args":["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"],"ok":-1}
{"fn":"getSignificantTokens","args":["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"],"ok":["what","caret","tilde","back","pipe"]}
{"fn":"buildSearchQuery","args":["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"],"ok":"what caret tilde back pipe"}
{"fn":"sanitizeQuery","args":["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"],"ok":"What s this that caret tilde back pipe"}
{"fn":"buildSmartSearchTitles","args":[["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"]],"ok":{"titles":["what caret tilde back pipe","what this that caret tilde back pipe"],"season":-1,"part":-1}}
{"fn":"buildAdvancedQuery","args":[["What's \"this\"? *that* ^caret! ~tilde~ \\back|pipe"]],"ok":"what caret tilde back pipe"}
{"fn":"normalizeTitle","args":["Season 2"],"ok":{"original":"Season 2","normalized":"","cleanBaseTitle":"","denoisedTitle":"","tokens":[],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["S2"],"ok":{"original":"S2","normalized":"","cleanBaseTitle":"","denoisedTitle":"","tokens":[],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Part 2"],"ok":{"original":"Part 2","normalized":"","cleanBaseTitle":"","denoisedTitle":"","tokens":[],"season":-1,"part":2,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["II"],"ok":{"original":"II","normalized":"ii","cleanBaseTitle":"","denoisedTitle":"","tokens":["ii"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["Title ſeason 2"],"ok":{"original":"Title ſeason 2","normalized":"title eason 2","cleanBaseTitle":"title eason","denoisedTitle":"title eason","tokens":["title","eason","2"],"season":2,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":["KING’s game"],"ok":{"original":"KING’s game","normalized":"king game","cleanBaseTitle":"king game","denoisedTitle":"king game","tokens":["king","game"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"compareTitles","args":["Attack on Titan","Attack on Titan Season 2"],"ok":1}
{"fn":"compareTitles","args":["Attack on Titan","Shingeki no Kyojin"],"ok":0}
{"fn":"compareTitles","args":["The Melancholy of Haruhi Suzumiya","Haruhi Suzumiya"],"ok":0.6060606060606061}
{"fn":"compareTitles","args":["Cowboy Bebop 1998","Cowboy Bebop"],"ok":0.8}
{"fn":"compareTitles","args":["Cowboy Bebop","Cowboy Bebop 1998"],"ok":1}
{"fn":"compareTitles","args":["","Title"],"ok":0}
{"fn":"compareTitles","args":["Title",""],"ok":0}
{"fn":"compareTitles","args":["One Piece","One Piece Film Red"],"ok":1}
{"fn":"compareTitles","args":["Kino no Tabi","Kino's Journey"],"ok":0.4347826086956522}
{"fn":"compareTitles","args":["Blade Runner 2049","Blade Runner"],"ok":0.8}
{"fn":"compareTitles","args":["no wa ga","no"],"ok":0.33333333333333337}
{"fn":"compareTitles","args":["Sword Art Online Alicization","Sword Art Online II"],"ok":0.75}
{"fn":"compareTitles","args":["!!!","???"],"ok":0}
{"fn":"compareTitles","args":["Re:Zero","Re Zero"],"ok":1}
{"fn":"compareTitles","args":["Overlord IV","Overlord"],"ok":0.5}
{"fn":"findBestMatch","args":["Overlord II",["Overlord","Overlord II","Overlord III"]],"ok":"Overlord II"}
{"fn":"findBestMatch","args":["Overlord II",["Shingeki no Kyojin","Attack on Titan Final Season","Attack on Titan"]],"ok":"Shingeki no Kyojin"}
{"fn":"findBestMatch","args":["Overlord II",[]],"ok":""}
{"fn":"findBestMatch","args":["Overlord II",[1,null,"Bleach"]],"ok":"Bleach"}
{"fn":"findBestMatch","args":["Overlord II",["zzz","yyy"]],"ok":"zzz"}
{"fn":"findBestMatch","args":["Overlord II",["","Naruto Shippuden","Naruto"]],"ok":""}
{"fn":"findBestMatch","args":["Overlord II",[5,6]],"ok":""}
{"fn":"findBestMatch","args":["Attack on Titan",["Overlord","Overlord II","Overlord III"]],"ok":"Overlord"}
{"fn":"findBestMatch","args":["Attack on Titan",["Shingeki no Kyojin","Attack on Titan Final Season","Attack on Titan"]],"ok":"Attack on Titan Final Season"}
{"fn":"findBestMatch","args":["Attack on Titan",[]],"ok":""}
{"fn":"findBestMatch","args":["Attack on Titan",[1,null,"Bleach"]],"ok":"Bleach"}
{"fn":"findBestMatch","args":["Attack on Titan",["zzz","yyy"]],"ok":"zzz"}
{"fn":"findBestMatch","args":["Attack on Titan",["","Naruto Shippuden","Naruto"]],"ok":""}
{"fn":"findBestMatch","args":["Attack on Titan",[5,6]],"ok":""}
{"fn":"findBestMatch","args":["Naruto",["Overlord","Overlord II","Overlord III"]],"ok":"Overlord"}
{"fn":"findBestMatch","args":["Naruto",["Shingeki no Kyojin","Attack on Titan Final Season","Attack on Titan"]],"ok":"Shingeki no Kyojin"}
{"fn":"findBestMatch","args":["Naruto",[]],"ok":""}
{"fn":"findBestMatch","args":["Naruto",[1,null,"Bleach"]],"ok":"Bleach"}
{"fn":"findBestMatch","args":["Naruto",["zzz","yyy"]],"ok":"zzz"}
{"fn":"findBestMatch","args":["Naruto",["","Naruto Shippuden","Naruto"]],"ok":"Naruto Shippuden"}
{"fn":"findBestMatch","args":["Naruto",[5,6]],"ok":""}
{"fn":"findBestMatch","args":["Bleach",["Overlord","Overlord II","Overlord III"]],"ok":"Overlord"}
{"fn":"findBestMatch","args":["Bleach",["Shingeki no Kyojin","Attack on Titan Final Season","Attack on Titan"]],"ok":"Shingeki no Kyojin"}
{"fn":"findBestMatch","args":["Bleach",[]],"ok":""}
{"fn":"findBestMatch","args":["Bleach",[1,null,"Bleach"]],"ok":"Bleach"}
{"fn":"findBestMatch","args":["Bleach",["zzz","yyy"]],"ok":"zzz"}
{"fn":"findBestMatch","args":["Bleach",["","Naruto Shippuden","Naruto"]],"ok":""}
{"fn":"findBestMatch","args":["Bleach",[5,6]],"ok":""}
{"fn":"findBestMatch","args":["",["Overlord","Overlord II","Overlord III"]],"ok":"Overlord"}
{"fn":"findBestMatch","args":["",["Shingeki no Kyojin","Attack on Titan Final Season","Attack on Titan"]],"ok":"Shingeki no Kyojin"}
{"fn":"findBestMatch","args":["",[]],"ok":""}
{"fn":"findBestMatch","args":["",[1,null,"Bleach"]],"ok":"Bleach"}
{"fn":"findBestMatch","args":["",["zzz","yyy"]],"ok":"zzz"}
{"fn":"findBestMatch","args":["",["","Naruto Shippuden","Naruto"]],"ok":""}
{"fn":"findBestMatch","args":["",[5,6]],"ok":""}
{"fn":"buildAdvancedQuery","args":[["Attack on Titan","Shingeki no Kyojin","ATTACK ON TITAN","",5,null,"The Attack of Titan"]],"ok":"(attack titan | shingeki kyojin)"}
{"fn":"buildAdvancedQuery","args":[[]],"ok":""}
{"fn":"buildAdvancedQuery","args":[["!!!",""]],"ok":""}
{"fn":"buildAdvancedQuery","args":[["Re:Zero","Re Zero","Re:Zero kara Hajimeru Isekai Seikatsu"]],"ok":"(re zero | re zero kara hajimeru isekai seikatsu)"}
{"fn":"buildSeasonQuery","args":["Overlord",-1],"ok":"overlord"}
{"fn":"buildPartQuery","args":["Overlord",-1],"ok":"overlord"}
{"fn":"buildSeasonQuery","args":["Overlord",0],"ok":"overlord"}
{"fn":"buildPartQuery","args":["Overlord",0],"ok":"overlord"}
{"fn":"buildSeasonQuery","args":["Overlord",1],"ok":"overlord"}
{"fn":"buildPartQuery","args":["Overlord",1],"ok":"overlord"}
{"fn":"buildSeasonQuery","args":["Overlord",2],"ok":"(overlord S02 | overlord S2 | overlord Season 2 | overlord 2nd Season)"}
{"fn":"buildPartQuery","args":["Overlord",2],"ok":"(overlord Part 2 | overlord Part II | overlord 2nd Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",3],"ok":"(overlord S03 | overlord S3 | overlord Season 3 | overlord 3rd Season)"}
{"fn":"buildPartQuery","args":["Overlord",3],"ok":"(overlord Part 3 | overlord Part III | overlord 3rd Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",4],"ok":"(overlord S04 | overlord S4 | overlord Season 4 | overlord 4th Season)"}
{"fn":"buildPartQuery","args":["Overlord",4],"ok":"(overlord Part 4 | overlord Part IV | overlord 4th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",9],"ok":"(overlord S09 | overlord S9 | overlord Season 9 | overlord 9th Season)"}
{"fn":"buildPartQuery","args":["Overlord",9],"ok":"(overlord Part 9 | overlord Part IX | overlord 9th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",10],"ok":"(overlord S10 | overlord S10 | overlord Season 10 | overlord 10th Season)"}
{"fn":"buildPartQuery","args":["Overlord",10],"ok":"(overlord Part 10 | overlord 10th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",11],"ok":"(overlord S11 | overlord S11 | overlord Season 11 | overlord 11th Season)"}
{"fn":"buildPartQuery","args":["Overlord",11],"ok":"(overlord Part 11 | overlord 11th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",12],"ok":"(overlord S12 | overlord S12 | overlord Season 12 | overlord 12th Season)"}
{"fn":"buildPartQuery","args":["Overlord",12],"ok":"(overlord Part 12 | overlord 12th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",13],"ok":"(overlord S13 | overlord S13 | overlord Season 13 | overlord 13th Season)"}
{"fn":"buildPartQuery","args":["Overlord",13],"ok":"(overlord Part 13 | overlord 13th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",21],"ok":"(overlord S21 | overlord S21 | overlord Season 21 | overlord 21st Season)"}
{"fn":"buildPartQuery","args":["Overlord",21],"ok":"(overlord Part 21 | overlord 21st Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",22],"ok":"(overlord S22 | overlord S22 | overlord Season 22 | overlord 22nd Season)"}
{"fn":"buildPartQuery","args":["Overlord",22],"ok":"(overlord Part 22 | overlord 22nd Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",23],"ok":"(overlord S23 | overlord S23 | overlord Season 23 | overlord 23rd Season)"}
{"fn":"buildPartQuery","args":["Overlord",23],"ok":"(overlord Part 23 | overlord 23rd Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",100],"ok":"(overlord S100 | overlord S100 | overlord Season 100 | overlord 100th Season)"}
{"fn":"buildPartQuery","args":["Overlord",100],"ok":"(overlord Part 100 | overlord 100th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",101],"ok":"(overlord S101 | overlord S101 | overlord Season 101 | overlord 101st Season)"}
{"fn":"buildPartQuery","args":["Overlord",101],"ok":"(overlord Part 101 | overlord 101st Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",111],"ok":"(overlord S111 | overlord S111 | overlord Season 111 | overlord 111th Season)"}
{"fn":"buildPartQuery","args":["Overlord",111],"ok":"(overlord Part 111 | overlord 111th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",112],"ok":"(overlord S112 | overlord S112 | overlord Season 112 | overlord 112th Season)"}
{"fn":"buildPartQuery","args":["Overlord",112],"ok":"(overlord Part 112 | overlord 112th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",1.9],"ok":"overlord"}
{"fn":"buildPartQuery","args":["Overlord",1.9],"ok":"overlord"}
{"fn":"buildSeasonQuery","args":["Overlord",2.7],"ok":"(overlord S02 | overlord S2 | overlord Season 2 | overlord 2nd Season)"}
{"fn":"buildPartQuery","args":["Overlord",2.7],"ok":"(overlord Part 2 | overlord Part II | overlord 2nd Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",-2.5],"ok":"overlord"}
{"fn":"buildPartQuery","args":["Overlord",-2.5],"ok":"overlord"}
{"fn":"buildSeasonQuery","args":["Overlord","3"],"ok":"(overlord S03 | overlord S3 | overlord Season 3 | overlord 3rd Season)"}
{"fn":"buildPartQuery","args":["Overlord","3"],"ok":"(overlord Part 3 | overlord Part III | overlord 3rd Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord","03"],"ok":"(overlord S03 | overlord S3 | overlord Season 3 | overlord 3rd Season)"}
{"fn":"buildPartQuery","args":["Overlord","03"],"ok":"(overlord Part 3 | overlord Part III | overlord 3rd Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord","+4"],"ok":"(overlord S04 | overlord S4 | overlord Season 4 | overlord 4th Season)"}
{"fn":"buildPartQuery","args":["Overlord","+4"],"ok":"(overlord Part 4 | overlord Part IV | overlord 4th Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord","-2"],"ok":"overlord"}
{"fn":"buildPartQuery","args":["Overlord","-2"],"ok":"overlord"}
{"fn":"buildSeasonQuery","args":["Overlord","abc"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Overlord","abc"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Overlord",""],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Overlord",""],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Overlord","2.5"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Overlord","2.5"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Overlord"," 3"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Overlord"," 3"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Overlord",true],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Overlord",true],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Overlord",null],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Overlord",null],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Overlord",[2]],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Overlord",[2]],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Overlord",10000000000000000000],"ok":"overlord"}
{"fn":"buildPartQuery","args":["Overlord",10000000000000000000],"ok":"overlord"}
{"fn":"buildSeasonQuery","args":["Overlord",-10000000000000000000],"ok":"overlord"}
{"fn":"buildPartQuery","args":["Overlord",-10000000000000000000],"ok":"overlord"}
{"fn":"buildSeasonQuery","args":["Overlord",9007199254740992],"ok":"(overlord S9007199254740992 | overlord S9007199254740992 | overlord Season 9007199254740992 | overlord 9007199254740992nd Season)"}
{"fn":"buildPartQuery","args":["Overlord",9007199254740992],"ok":"(overlord Part 9007199254740992 | overlord 9007199254740992nd Cour)"}
{"fn":"buildSeasonQuery","args":["Overlord",100000000000000000],"ok":"(overlord S100000000000000000 | overlord S100000000000000000 | overlord Season 100000000000000000 | overlord 100000000000000000th Season)"}
{"fn":"buildPartQuery","args":["Overlord",100000000000000000],"ok":"(overlord Part 100000000000000000 | overlord 100000000000000000th Cour)"}
{"fn":"buildSeasonQuery","args":["Re:Zero",1],"ok":"re zero"}
{"fn":"buildPartQuery","args":["Re:Zero",1],"ok":"re zero"}
{"fn":"buildSeasonQuery","args":["Re:Zero",2],"ok":"(re zero S02 | re zero S2 | re zero Season 2 | re zero 2nd Season)"}
{"fn":"buildPartQuery","args":["Re:Zero",2],"ok":"(re zero Part 2 | re zero Part II | re zero 2nd Cour)"}
{"fn":"buildSeasonQuery","args":["Re:Zero",3],"ok":"(re zero S03 | re zero S3 | re zero Season 3 | re zero 3rd Season)"}
{"fn":"buildPartQuery","args":["Re:Zero",3],"ok":"(re zero Part 3 | re zero Part III | re zero 3rd Cour)"}
{"fn":"buildSeasonQuery","args":["Re:Zero",10],"ok":"(re zero S10 | re zero S10 | re zero Season 10 | re zero 10th Season)"}
{"fn":"buildPartQuery","args":["Re:Zero",10],"ok":"(re zero Part 10 | re zero 10th Cour)"}
{"fn":"buildSeasonQuery","args":["Re:Zero",2.7],"ok":"(re zero S02 | re zero S2 | re zero Season 2 | re zero 2nd Season)"}
{"fn":"buildPartQuery","args":["Re:Zero",2.7],"ok":"(re zero Part 2 | re zero Part II | re zero 2nd Cour)"}
{"fn":"buildSeasonQuery","args":["Re:Zero","3"],"ok":"(re zero S03 | re zero S3 | re zero Season 3 | re zero 3rd Season)"}
{"fn":"buildPartQuery","args":["Re:Zero","3"],"ok":"(re zero Part 3 | re zero Part III | re zero 3rd Cour)"}
{"fn":"buildSeasonQuery","args":["Re:Zero","abc"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Re:Zero","abc"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Re:Zero",""],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Re:Zero",""],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Re:Zero","2.5"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Re:Zero","2.5"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Re:Zero"," 3"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Re:Zero"," 3"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Re:Zero",true],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Re:Zero",true],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Re:Zero",null],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Re:Zero",null],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Re:Zero",[2]],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Re:Zero",[2]],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya",1],"ok":"melancholy haruhi suzumiya"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya",1],"ok":"melancholy haruhi suzumiya"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya",2],"ok":"(melancholy haruhi suzumiya S02 | melancholy haruhi suzumiya S2 | melancholy haruhi suzumiya Season 2 | melancholy haruhi suzumiya 2nd Season)"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya",2],"ok":"(melancholy haruhi suzumiya Part 2 | melancholy haruhi suzumiya Part II | melancholy haruhi suzumiya 2nd Cour)"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya","3"],"ok":"(melancholy haruhi suzumiya S03 | melancholy haruhi suzumiya S3 | melancholy haruhi suzumiya Season 3 | melancholy haruhi suzumiya 3rd Season)"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya","3"],"ok":"(melancholy haruhi suzumiya Part 3 | melancholy haruhi suzumiya Part III | melancholy haruhi suzumiya 3rd Cour)"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya","abc"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya","abc"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya",""],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya",""],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya","2.5"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya","2.5"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya"," 3"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya"," 3"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya",true],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya",true],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya",null],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya",null],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["The Melancholy of Haruhi Suzumiya",[2]],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["The Melancholy of Haruhi Suzumiya",[2]],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["!!!",1],"ok":""}
{"fn":"buildPartQuery","args":["!!!",1],"ok":""}
{"fn":"buildSeasonQuery","args":["!!!",2],"ok":"( S02 |  S2 |  Season 2 |  2nd Season)"}
{"fn":"buildPartQuery","args":["!!!",2],"ok":"( Part 2 |  Part II |  2nd Cour)"}
{"fn":"buildSeasonQuery","args":["!!!","3"],"ok":"( S03 |  S3 |  Season 3 |  3rd Season)"}
{"fn":"buildPartQuery","args":["!!!","3"],"ok":"( Part 3 |  Part III |  3rd Cour)"}
{"fn":"buildSeasonQuery","args":["!!!","abc"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["!!!","abc"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["!!!",""],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["!!!",""],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["!!!","2.5"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["!!!","2.5"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["!!!"," 3"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["!!!"," 3"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["!!!",true],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["!!!",true],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["!!!",null],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["!!!",null],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["!!!",[2]],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["!!!",[2]],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["",1],"ok":""}
{"fn":"buildPartQuery","args":["",1],"ok":""}
{"fn":"buildSeasonQuery","args":["",2],"ok":"( S02 |  S2 |  Season 2 |  2nd Season)"}
{"fn":"buildPartQuery","args":["",2],"ok":"( Part 2 |  Part II |  2nd Cour)"}
{"fn":"buildSeasonQuery","args":["","3"],"ok":"( S03 |  S3 |  Season 3 |  3rd Season)"}
{"fn":"buildPartQuery","args":["","3"],"ok":"( Part 3 |  Part III |  3rd Cour)"}
{"fn":"buildSeasonQuery","args":["","abc"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["","abc"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["",""],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["",""],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["","2.5"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["","2.5"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":[""," 3"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":[""," 3"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["",true],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["",true],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["",null],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["",null],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["",[2]],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["",[2]],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Attack on Titan Season 2","abc"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Attack on Titan Season 2","abc"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Attack on Titan Season 2",""],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Attack on Titan Season 2",""],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Attack on Titan Season 2","2.5"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Attack on Titan Season 2","2.5"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Attack on Titan Season 2"," 3"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Attack on Titan Season 2"," 3"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Attack on Titan Season 2",true],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Attack on Titan Season 2",true],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Attack on Titan Season 2",null],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Attack on Titan Season 2",null],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Attack on Titan Season 2",[2]],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Attack on Titan Season 2",[2]],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Mob Psycho 100","abc"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Mob Psycho 100","abc"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Mob Psycho 100",""],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Mob Psycho 100",""],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Mob Psycho 100","2.5"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Mob Psycho 100","2.5"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Mob Psycho 100"," 3"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Mob Psycho 100"," 3"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Mob Psycho 100",true],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Mob Psycho 100",true],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Mob Psycho 100",null],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Mob Psycho 100",null],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["Mob Psycho 100",[2]],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["Mob Psycho 100",[2]],"err":"selection is nil"}
{"fn":"buildSmartSearchTitles","args":[["Shingeki no Kyojin: The Final Season Part 2","Attack on Titan: Final Season Part 2","AoT Final Season","進撃の巨人 The Final Season Part.2"]],"ok":{"titles":["shingeki kyojin final","shingeki no kyojin final season","shingeki kyojin","attack titan final","attack on titan final season","attack titan","aot final","aot final season","final","final season"],"season":-1,"part":2}}
{"fn":"buildSmartSearchTitles","args":[["Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season","Re:ZERO -Starting Life in Another World- Season 2","Re:Zero 2","Re Zero 2"]],"ok":{"titles":["re zero kara hajimeru isekai seikatsu","re zero starting life another world","re zero starting life in another world","re zero"],"season":2,"part":-1}}
{"fn":"buildSmartSearchTitles","args":[["Mushoku Tensei II: Isekai Ittara Honki Dasu Part 2","Mushoku Tensei: Jobless Reincarnation Season 2 Part 2",3,null,""]],"ok":{"titles":["mushoku tensei isekai ittara honki dasu","mushoku tensei","mushoku tensei jobless reincarnation"],"season":2,"part":2}}
{"fn":"buildSmartSearchTitles","args":[["Overlord","OVERLORD","overlord","Overlord IV"]],"ok":{"titles":["overlord"],"season":4,"part":-1}}
{"fn":"buildSmartSearchTitles","args":[["Bleach: Sennen Kessen-hen - Ketsubetsu-tan","Show: A","Ab - Cd","Äb: cd","日本: x",": lead"," - lead","Abcd : x"]],"ok":{"titles":["bleach sennen kessen hen ketsubetsu tan","bleach","bleach sennen kessen hen","show","show a","ab cd","b cd","x","lead","abcd x"],"season":-1,"part":-1}}
{"fn":"buildSmartSearchTitles","args":[["Title Part 3","Title Part 2 Season 4","Title 5"]],"ok":{"titles":["title"],"season":4,"part":3}}
{"fn":"buildSmartSearchTitles","args":[["!!!","The","2"]],"ok":{"titles":["2"],"season":2,"part":-1}}
{"fn":"normalizeTitle","args":[],"err":"selection is nil"}
{"fn":"normalizeTitle","args":[5],"err":"selection is nil"}
{"fn":"normalizeTitle","args":[null],"err":"selection is nil"}
{"fn":"normalizeTitle","args":["x","extra"],"ok":{"original":"x","normalized":"x","cleanBaseTitle":"x","denoisedTitle":"x","tokens":["x"],"season":-1,"part":-1,"year":-1,"isMain":false,"isExtra":false}}
{"fn":"normalizeTitle","args":[["x"]],"err":"selection is nil"}
{"fn":"normalizeTitle","args":[{"a":1}],"err":"selection is nil"}
{"fn":"extractPartNumber","args":[],"err":"selection is nil"}
{"fn":"extractPartNumber","args":[5],"err":"selection is nil"}
{"fn":"extractPartNumber","args":[null],"err":"selection is nil"}
{"fn":"extractPartNumber","args":[["x"]],"err":"selection is nil"}
{"fn":"extractPartNumber","args":[{"a":1}],"err":"selection is nil"}
{"fn":"extractSeasonNumber","args":[],"err":"selection is nil"}
{"fn":"extractSeasonNumber","args":[5],"err":"selection is nil"}
{"fn":"extractSeasonNumber","args":[null],"err":"selection is nil"}
{"fn":"extractSeasonNumber","args":[["x"]],"err":"selection is nil"}
{"fn":"extractSeasonNumber","args":[{"a":1}],"err":"selection is nil"}
{"fn":"extractYear","args":[],"err":"selection is nil"}
{"fn":"extractYear","args":[5],"err":"selection is nil"}
{"fn":"extractYear","args":[null],"err":"selection is nil"}
{"fn":"extractYear","args":[["x"]],"err":"selection is nil"}
{"fn":"extractYear","args":[{"a":1}],"err":"selection is nil"}
{"fn":"getSignificantTokens","args":[],"err":"selection is nil"}
{"fn":"getSignificantTokens","args":[5],"err":"selection is nil"}
{"fn":"getSignificantTokens","args":[null],"err":"selection is nil"}
{"fn":"getSignificantTokens","args":[["x"]],"err":"selection is nil"}
{"fn":"getSignificantTokens","args":[{"a":1}],"err":"selection is nil"}
{"fn":"buildSearchQuery","args":[],"err":"selection is nil"}
{"fn":"buildSearchQuery","args":[5],"err":"selection is nil"}
{"fn":"buildSearchQuery","args":[null],"err":"selection is nil"}
{"fn":"buildSearchQuery","args":[["x"]],"err":"selection is nil"}
{"fn":"buildSearchQuery","args":[{"a":1}],"err":"selection is nil"}
{"fn":"sanitizeQuery","args":[],"err":"selection is nil"}
{"fn":"sanitizeQuery","args":[5],"err":"selection is nil"}
{"fn":"sanitizeQuery","args":[null],"err":"selection is nil"}
{"fn":"sanitizeQuery","args":[["x"]],"err":"selection is nil"}
{"fn":"sanitizeQuery","args":[{"a":1}],"err":"selection is nil"}
{"fn":"buildAdvancedQuery","args":[],"err":"selection is nil"}
{"fn":"buildAdvancedQuery","args":[5],"err":"selection is nil"}
{"fn":"buildAdvancedQuery","args":[null],"err":"selection is nil"}
{"fn":"buildAdvancedQuery","args":["x","extra"],"err":"selection is nil"}
{"fn":"buildAdvancedQuery","args":[{"a":1}],"err":"selection is nil"}
{"fn":"buildSmartSearchTitles","args":[],"err":"selection is nil"}
{"fn":"buildSmartSearchTitles","args":[5],"err":"selection is nil"}
{"fn":"buildSmartSearchTitles","args":[null],"err":"selection is nil"}
{"fn":"buildSmartSearchTitles","args":["x","extra"],"err":"selection is nil"}
{"fn":"buildSmartSearchTitles","args":[{"a":1}],"err":"selection is nil"}
{"fn":"compareTitles","args":[],"err":"selection is nil"}
{"fn":"compareTitles","args":["x"],"err":"selection is nil"}
{"fn":"compareTitles","args":[5,"y"],"err":"selection is nil"}
{"fn":"compareTitles","args":["x",null],"err":"selection is nil"}
{"fn":"compareTitles","args":["x",{"a":1}],"err":"selection is nil"}
{"fn":"compareTitles","args":["x","y"],"ok":0}
{"fn":"compareTitles","args":["x",["y"]],"err":"selection is nil"}
{"fn":"findBestMatch","args":[],"err":"selection is nil"}
{"fn":"findBestMatch","args":["x"],"err":"selection is nil"}
{"fn":"findBestMatch","args":[5,"y"],"err":"selection is nil"}
{"fn":"findBestMatch","args":["x",null],"err":"selection is nil"}
{"fn":"findBestMatch","args":["x",{"a":1}],"err":"selection is nil"}
{"fn":"findBestMatch","args":["x","y"],"err":"selection is nil"}
{"fn":"findBestMatch","args":["x",["y"]],"ok":"y"}
{"fn":"buildSeasonQuery","args":[],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["x"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":[5,"y"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["x",null],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["x",{"a":1}],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["x","y"],"err":"selection is nil"}
{"fn":"buildSeasonQuery","args":["x",["y"]],"err":"selection is nil"}
{"fn":"buildPartQuery","args":[],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["x"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":[5,"y"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["x",null],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["x",{"a":1}],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["x","y"],"err":"selection is nil"}
{"fn":"buildPartQuery","args":["x",["y"]],"err":"selection is nil"}
"#;
