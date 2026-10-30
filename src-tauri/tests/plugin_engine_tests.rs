//! The plugin engine: the globals a plugin sees, real community payloads, and the sandbox.

mod plugin_support;

use anix_lib::plugins::engine::{EngineLimits, PluginEngine, PluginError, PluginRuntime};
use anix_lib::plugins::transpile::typescript_to_javascript;
use anix_lib::plugins::userconfig::{self, UserConfig};
use anix_lib::web_session::SessionUse;
use base64::Engine as _;
use plugin_support::{reference_repo, runtime, spec, Req, Resp, Site};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A plugin whose `run(name, arg)` evaluates one of the probes below inside the engine.
const PROBES: &str = r#"
const probes = {
  async headers(base) {
    const res = await fetch(base + '/echo', { headers: { Referer: 'https://ref.example/', Origin: 'https://ref.example', Cookie: 'a=1; b=2', 'X-Custom': 'yes' } });
    return { status: res.status, ok: res.ok, statusText: res.statusText, seen: res.json(), type: res.headers['Content-Type'], lower: res.headers['content-type'], viaGet: res.headers.get('CONTENT-TYPE') };
  },
  async defaults(base) {
    const res = await fetch(base + '/echo');
    return res.json();
  },
  async redirect(base) {
    const res = await fetch(base + '/moved');
    return { url: res.url, redirected: res.redirected, text: await res.text() };
  },
  async manual(base) {
    const res = await fetch(base + '/moved', { redirect: 'manual' });
    return { status: res.status, location: res.headers['Location'] };
  },
  async binary(base) {
    const res = await fetch(base + '/bytes');
    const viaHelper = await $anilili.fetchBytes(base + '/bytes');
    return { length: res.body.length, first: Array.from(res.body.slice(0, 4)), isBytes: res.body instanceof Uint8Array, helper: viaHelper.base64, helperOk: viaHelper.ok, cookie: res.cookies.sid };
  },
  async post(base) {
    const asText = await fetch(base + '/echo', { method: 'POST', body: 'plain body' });
    const asJson = await fetch(base + '/echo', { method: 'post', body: { a: 1 } });
    const asForm = await fetch(base + '/echo', { method: 'POST', body: new URLSearchParams({ q: 'one piece', n: '1' }) });
    return [asText.json(), asJson.json(), asForm.json()];
  },
  async missing(base) {
    const res = await fetch(base + '/nothing');
    const helper = await $anilili.fetchBytes(base + '/nothing');
    return { ok: res.ok, status: res.status, json: res.json(), helperOk: helper.ok, helperStatus: helper.status };
  },
  async refused(url) {
    try { await fetch(url); return 'answered'; } catch (e) { return String(e.message); }
  },
  async parallel(base) {
    const all = await Promise.all([1, 2, 3, 4].map((n) => fetch(base + '/slow?n=' + n).then((r) => r.text())));
    return all;
  },
  doc(html) {
    const $ = LoadDoc(html);
    const items = $('ul.list > li');
    const out = { count: items.length(), titles: [], hrefs: items.map((i, el) => el.find('a').attr('href')) };
    items.each((i, el) => { out.titles.push(el.find('a').text().trim()); });
    out.second = items.eq(1).attr('data-id');
    out.last = items.last().data('id');
    out.missing = typeof items.first().attr('nope');
    out.active = items.filter('.active').length();
    out.notActive = items.not('.active').length();
    out.byCallback = items.filter((i, el) => el.attr('data-id') === '3').text().trim();
    out.html = $('#box').html();
    out.text = $('#box').text();
    out.parent = $('a.first').parent().attr('data-id');
    out.closest = $('a.first').closest('ul').attr('class');
    out.next = items.first().next().attr('data-id');
    out.prevAll = items.last().prevAll().length();
    out.siblings = items.eq(1).siblings().length();
    out.children = $('ul.list').children('.active').length();
    out.has = $('div').has('b').attr('id');
    out.is = items.first().is('.active');
    out.attrs = $('meta[name="csrf-token"]').attrs();
    out.content = $('meta[name="csrf-token"]').attr('content');
    out.script = $('script').map((i, el) => el.text()).filter((t) => t.includes('window.data'))[0];
    out.escaped = $('article.group\\/item').length();
    out.none = $('.nothing').text() + '|' + $('.nothing').length() + '|' + $('.nothing').html();
    out.nested = $('ul.list').find('li').first().find('a').length();
    out.iframe = $('#embed iframe').attr('src') || '';
    return out;
  },
  crypto() {
    const key = CryptoJS.enc.Utf8.parse('0123456789abcdef0123456789abcdef');
    const iv = CryptoJS.enc.Utf8.parse('abcdef9876543210');
    const sealed = CryptoJS.AES.encrypt('attack at dawn', key, { iv }).toString();
    const opened = CryptoJS.AES.decrypt(sealed, key, { iv }).toString(CryptoJS.enc.Utf8);
    const loose = CryptoJS.AES.encrypt('seanime style', 'a passphrase of any length');
    const looseOpened = CryptoJS.AES.decrypt(loose.toString(), 'a passphrase of any length').toString(CryptoJS.enc.Utf8);
    const openssl = CryptoJS.AES.decrypt('U2FsdGVkX18BAgMEBQYHCOliXbnJOFxwn7jVxRHAdX4=', 'secret').toString(CryptoJS.enc.Utf8);
    const parsed = CryptoJS.enc.Base64.parse('aGVsbG8=');
    return {
      sealed, opened, looseOpened, openssl,
      parsedIsBytes: parsed instanceof Uint8Array, parsedText: CryptoJS.enc.Utf8.stringify(parsed),
      hex: CryptoJS.enc.Hex.stringify(CryptoJS.enc.Utf8.parse('hi')),
      md5: CryptoJS.MD5('hello').toString(),
      sha256: CryptoJS.SHA256('hello').toString(),
      hmac: CryptoJS.HmacSHA256('message', 'key').toString(),
    };
  },
  encodings() {
    const url = new URL('/a/b?x=1&y=two words#frag', 'https://example.org:8443/base/');
    url.searchParams.set('z', '3');
    const params = new URLSearchParams('a=1&b=2&a=3');
    return {
      b64: Buffer.from('héllo wörld').toString('base64'),
      back: Buffer.from('aMOpbGxvIHfDtnJsZA==', 'base64').toString('utf-8'),
      hex: Buffer.from('abc').toString('hex'),
      fromHex: Buffer.from('616263', 'hex').toString(),
      concat: Buffer.concat([Buffer.from('ab'), Buffer.from('cd')]).toString(),
      isBuffer: Buffer.isBuffer(Buffer.alloc(2)),
      atob: atob('aGVsbG8='), btoa: btoa('hello'),
      urlsafe: atob('aGk_aGk-'.replace(/_/g, '/').replace(/-/g, '+')),
      encoded: Array.from(new TextEncoder().encode('é')),
      decoded: new TextDecoder().decode(new Uint8Array([0xc3, 0xa9, 0x21])),
      href: url.href, host: url.host, hostname: url.hostname, port: url.port, pathname: url.pathname, search: url.search, hash: url.hash, origin: url.origin, x: url.searchParams.get('x'),
      params: params.getAll('a'), paramsText: new URLSearchParams({ q: 'a b&c' }).toString(),
      invalid: (() => { try { new URL('not a url'); return 'parsed'; } catch (e) { return 'threw'; } })(),
    };
  },
  async timers() {
    const order = [];
    const cancelled = setTimeout(() => order.push('cancelled'), 5);
    clearTimeout(cancelled);
    await new Promise((resolve) => { setTimeout(() => { order.push('late'); resolve(); }, 30); setTimeout(() => order.push('early'), 5); });
    return order;
  },
  anilili(input) {
    return {
      version: typeof $anilili.version, id: $anilili.pluginId,
      gunzip: $anilili.gunzip(input.gzip),
      alias: $anix === $anilili && $anix.gunzip(input.gzip) === $anilili.gunzip(input.gzip),
      xor: $anilili.xor(input.plain, 'key'),
      twice: Buffer.from($anilili.xor($anilili.xor(input.plain, 'key'), 'key'), 'base64').toString(),
      fromBytes: $anilili.gunzip(Buffer.from(input.gzip, 'base64')),
    };
  },
  sandbox() {
    const names = ['require', 'process', 'module', 'std', 'os', 'Deno', 'Bun', 'XMLHttpRequest', 'WebSocket', '__host'];
    const present = names.filter((name) => typeof globalThis[name] !== 'undefined');
    let imported = 'no';
    try { imported = String(typeof eval('import("fs")')); } catch (e) { imported = 'threw'; }
    return { present, imported };
  },
  async importing() {
    try { await import('fs'); return 'loaded'; } catch (e) { return 'refused'; }
  },
  spin() { while (true) {} },
  hog() { const all = []; while (true) { all.push(new Array(100000).fill('x'.repeat(10))); } },
  recurse() { const f = () => f() + 1; return f(); },
  boom() { throw new Error('the site changed'); },
  async never() { await new Promise(() => {}); },
  cyclic() { const a = {}; a.self = a; return a; },
  fine() { return 'still here'; },
};
class Provider {
  getSettings() { return { episodeServers: ['one'], supportsDub: false }; }
  run(name, arg) { return probes[name](arg); }
}
"#;

async fn run(engine: &PluginRuntime, name: &str, arg: Value) -> Result<Value, PluginError> {
    engine
        .call("run", &[json!(name), arg], Duration::from_secs(10), SessionUse::Existing)
        .await
}

fn echo_site_handler(request: &Req) -> Resp {
    match request.route() {
        "/echo" => Resp::json(json!({
            "method": request.method,
            "referer": request.header("referer"),
            "origin": request.header("origin"),
            "cookie": request.header("cookie"),
            "custom": request.header("x-custom"),
            "agent": request.header("user-agent"),
            "type": request.header("content-type"),
            "body": String::from_utf8_lossy(&request.body),
        })),
        "/moved" => Resp::status(302).with_header("Location", "/landed"),
        "/landed" => Resp::text("landed here"),
        "/bytes" => Resp::new(200, "application/octet-stream", vec![0u8, 255, 128, 10, 13, 0, 200])
            .with_header("Set-Cookie", "sid=abc123; Path=/; HttpOnly"),
        "/slow" => Resp::text(&format!("n{}", request.query("n"))),
        _ => Resp::new(404, "text/html", "<html>not here</html>"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_passes_headers_follows_redirects_and_keeps_bytes() {
    let site = Site::start(echo_site_handler).await;
    let engine = runtime("probe", PROBES);
    let base = json!(site.base());

    let headers = run(&engine, "headers", base.clone()).await.unwrap();
    assert_eq!(headers["status"], 200);
    assert_eq!(headers["ok"], true);
    assert_eq!(headers["statusText"], "200 OK");
    assert_eq!(headers["seen"]["referer"], "https://ref.example/");
    assert_eq!(headers["seen"]["origin"], "https://ref.example");
    assert_eq!(headers["seen"]["cookie"], "a=1; b=2");
    assert_eq!(headers["seen"]["custom"], "yes");
    assert_eq!(headers["type"], "application/json");
    assert_eq!(headers["lower"], "application/json");
    assert_eq!(headers["viaGet"], "application/json");

    let defaults = run(&engine, "defaults", base.clone()).await.unwrap();
    assert!(defaults["agent"].as_str().unwrap().starts_with("Mozilla/5.0"), "{}", defaults);
    assert_eq!(defaults["referer"], "");

    let redirect = run(&engine, "redirect", base.clone()).await.unwrap();
    assert_eq!(redirect["text"], "landed here");
    assert_eq!(redirect["redirected"], true);
    assert!(redirect["url"].as_str().unwrap().ends_with("/landed"));

    let manual = run(&engine, "manual", base.clone()).await.unwrap();
    assert_eq!(manual["status"], 302);
    assert_eq!(manual["location"], "/landed");

    let binary = run(&engine, "binary", base.clone()).await.unwrap();
    assert_eq!(binary["length"], 7);
    assert_eq!(binary["first"], json!([0, 255, 128, 10]));
    assert_eq!(binary["isBytes"], true);
    assert_eq!(binary["helperOk"], true);
    assert_eq!(binary["cookie"], "abc123");
    let helper = base64::engine::general_purpose::STANDARD
        .decode(binary["helper"].as_str().unwrap())
        .unwrap();
    assert_eq!(helper, vec![0u8, 255, 128, 10, 13, 0, 200]);

    let post = run(&engine, "post", base.clone()).await.unwrap();
    assert_eq!(post[0]["method"], "POST");
    assert_eq!(post[0]["body"], "plain body");
    assert_eq!(post[1]["body"], "{\"a\":1}");
    assert_eq!(post[1]["type"], "application/json");
    assert_eq!(post[2]["body"], "q=one+piece&n=1");
    assert!(post[2]["type"].as_str().unwrap().starts_with("application/x-www-form-urlencoded"));

    let missing = run(&engine, "missing", base.clone()).await.unwrap();
    assert_eq!(missing["ok"], false);
    assert_eq!(missing["status"], 404);
    assert_eq!(missing["json"], Value::Null);
    assert_eq!(missing["helperOk"], false);
    assert_eq!(missing["helperStatus"], 404);

    let parallel = run(&engine, "parallel", base).await.unwrap();
    assert_eq!(parallel, json!(["n1", "n2", "n3", "n4"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_refuses_other_schemes_and_private_addresses() {
    let site = Site::start(echo_site_handler).await;
    let engine = runtime("probe", PROBES);
    for url in ["file:///etc/passwd", "ftp://example.org/x", "data:text/plain,hi", "javascript:1"] {
        let answer = run(&engine, "refused", json!(url)).await.unwrap();
        assert!(answer.as_str().unwrap().contains("only http and https"), "{}: {}", url, answer);
    }

    // Without the test allowance, loopback and private addresses are out of reach.
    let mut strict = spec("strict", PROBES);
    strict.allow_private_network = false;
    let strict = PluginRuntime::new(strict);
    for url in [site.url("/echo"), "http://localhost:1/".to_string(), "http://192.168.1.1/".to_string(), "http://[::1]/".to_string(), "http://169.254.169.254/latest".to_string()] {
        let answer = run(&strict, "refused", json!(url)).await.unwrap();
        assert!(answer.as_str().unwrap().contains("private network access denied"), "{}: {}", url, answer);
    }
    // A name that resolves to loopback (`localhost4` in /etc/hosts) is refused before the
    // connection is made, so the site never sees the request.
    let hosts = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    let alias = hosts.lines().find_map(|line| {
        let mut fields = line.split('#').next()?.split_whitespace();
        (fields.next()? == "127.0.0.1").then(|| fields.find(|n| !n.contains('.') && *n != "localhost"))?
    });
    if let Some(alias) = alias {
        let url = format!("http://{}:{}/echo", alias, site.addr.port());
        let answer = run(&strict, "refused", json!(url)).await.unwrap();
        assert!(answer.as_str().unwrap().contains("private network access denied"), "{}: {}", url, answer);
    }
    assert_eq!(site.count("/echo"), 0);
}

const FIXTURE_HTML: &str = r#"<!doctype html><html><head>
<meta name="csrf-token" content="tok-123">
<script>window.data = {"a":1};</script><script src="x.js"></script></head>
<body>
<ul class="list">
  <li data-id="1" class="active"><a class="first" href="/watch/one">  One </a></li>
  <li data-id="2"><a href="/watch/two">Two</a></li>
  <li data-id="3"><a href="/watch/three">Three</a></li>
</ul>
<div id="box">Hello <b>bold</b> world</div>
<div id="plain">nothing</div>
<article class="group/item">x</article>
<div id="embed"><iframe src="https://player.example/e/abc"></iframe></div>
</body></html>"#;

#[tokio::test(flavor = "multi_thread")]
async fn load_doc_answers_the_calls_providers_make() {
    let engine = runtime("probe", PROBES);
    let doc = run(&engine, "doc", json!(FIXTURE_HTML)).await.unwrap();
    assert_eq!(doc["count"], 3);
    assert_eq!(doc["titles"], json!(["One", "Two", "Three"]));
    assert_eq!(doc["hrefs"], json!(["/watch/one", "/watch/two", "/watch/three"]));
    assert_eq!(doc["second"], "2");
    assert_eq!(doc["last"], "3");
    assert_eq!(doc["missing"], "undefined");
    assert_eq!(doc["active"], 1);
    assert_eq!(doc["notActive"], 2);
    assert_eq!(doc["byCallback"], "Three");
    assert_eq!(doc["html"], "Hello <b>bold</b> world");
    assert_eq!(doc["text"], "Hello bold world");
    assert_eq!(doc["parent"], "1");
    assert_eq!(doc["closest"], "list");
    assert_eq!(doc["next"], "2");
    assert_eq!(doc["prevAll"], 2);
    assert_eq!(doc["siblings"], 2);
    assert_eq!(doc["children"], 1);
    assert_eq!(doc["has"], "box");
    assert_eq!(doc["is"], true);
    assert_eq!(doc["attrs"], json!({ "name": "csrf-token", "content": "tok-123" }));
    assert_eq!(doc["content"], "tok-123");
    assert_eq!(doc["script"], "window.data = {\"a\":1};");
    assert_eq!(doc["escaped"], 1);
    assert_eq!(doc["none"], "|0|");
    assert_eq!(doc["nested"], 1);
    assert_eq!(doc["iframe"], "https://player.example/e/abc");
}

#[tokio::test(flavor = "multi_thread")]
async fn crypto_buffer_url_and_timers_behave() {
    let engine = runtime("probe", PROBES);

    let crypto = run(&engine, "crypto", Value::Null).await.unwrap();
    assert_eq!(crypto["opened"], "attack at dawn");
    assert_eq!(crypto["looseOpened"], "seanime style");
    assert_eq!(crypto["openssl"], "salted hello");
    assert_eq!(crypto["parsedIsBytes"], true);
    assert_eq!(crypto["parsedText"], "hello");
    assert_eq!(crypto["hex"], "6869");
    assert_eq!(crypto["md5"], "5d41402abc4b2a76b9719d911017c592");
    assert_eq!(crypto["sha256"], "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    assert_eq!(crypto["hmac"], "6e9ef29b75fffc5b7abae527d58fdadb2fe42e7219011976917343065f58ed4a");
    // AES-256-CBC with PKCS#7 of a known plaintext, key and IV.
    let sealed = base64::engine::general_purpose::STANDARD
        .decode(crypto["sealed"].as_str().unwrap())
        .unwrap();
    assert_eq!(sealed.len(), 16);

    let enc = run(&engine, "encodings", Value::Null).await.unwrap();
    assert_eq!(enc["b64"], "aMOpbGxvIHfDtnJsZA==");
    assert_eq!(enc["back"], "héllo wörld");
    assert_eq!(enc["hex"], "616263");
    assert_eq!(enc["fromHex"], "abc");
    assert_eq!(enc["concat"], "abcd");
    assert_eq!(enc["isBuffer"], true);
    assert_eq!(enc["atob"], "hello");
    assert_eq!(enc["btoa"], "aGVsbG8=");
    assert_eq!(enc["urlsafe"], "hi?hi>");
    assert_eq!(enc["encoded"], json!([195, 169]));
    assert_eq!(enc["decoded"], "é!");
    assert_eq!(enc["href"], "https://example.org:8443/a/b?x=1&y=two+words&z=3#frag");
    assert_eq!(enc["host"], "example.org:8443");
    assert_eq!(enc["hostname"], "example.org");
    assert_eq!(enc["port"], "8443");
    assert_eq!(enc["pathname"], "/a/b");
    assert_eq!(enc["hash"], "#frag");
    assert_eq!(enc["origin"], "https://example.org:8443");
    assert_eq!(enc["x"], "1");
    assert_eq!(enc["params"], json!(["1", "3"]));
    assert_eq!(enc["paramsText"], "q=a+b%26c");
    assert_eq!(enc["invalid"], "threw");

    let timers = run(&engine, "timers", Value::Null).await.unwrap();
    assert_eq!(timers, json!(["early", "late"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn app_helpers_gunzip_and_xor_under_both_names() {
    let engine = runtime("probe", PROBES);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all("{\"title\":\"ワンピース\"}".as_bytes()).unwrap();
    let gzip = base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap());
    let plain = base64::engine::general_purpose::STANDARD.encode("secret text");

    let out = run(&engine, "anilili", json!({ "gzip": gzip, "plain": plain })).await.unwrap();
    assert_eq!(out["version"], "string");
    assert_eq!(out["id"], "probe");
    assert_eq!(out["alias"], true, "`$anix` and `$anilili` are the same helpers");
    assert_eq!(out["gunzip"], "{\"title\":\"ワンピース\"}");
    assert_eq!(out["fromBytes"], "{\"title\":\"ワンピース\"}");
    assert_eq!(out["twice"], "secret text");
    let xored = base64::engine::general_purpose::STANDARD
        .decode(out["xor"].as_str().unwrap())
        .unwrap();
    let expected: Vec<u8> = "secret text".bytes().zip("key".bytes().cycle()).map(|(a, b)| a ^ b).collect();
    assert_eq!(xored, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sandbox_has_no_modules_files_or_processes() {
    let engine = runtime("probe", PROBES);
    let sandbox = run(&engine, "sandbox", Value::Null).await.unwrap();
    assert_eq!(sandbox["present"], json!([]), "{}", sandbox);
    assert_eq!(run(&engine, "importing", Value::Null).await.unwrap(), "refused");
}

#[tokio::test(flavor = "multi_thread")]
async fn runaway_scripts_only_fail_themselves() {
    let mut limited = spec("limited", PROBES);
    limited.limits = EngineLimits {
        memory_bytes: 24 * 1024 * 1024,
        slice: Duration::from_millis(400),
        ..EngineLimits::default()
    };
    let engine = PluginRuntime::new(limited);

    let started = Instant::now();
    let spin = run(&engine, "spin", Value::Null).await.unwrap_err();
    assert!(matches!(spin, PluginError::Stopped(_)), "{:?}", spin);
    assert!(spin.reason().contains("without pausing"), "{}", spin);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(run(&engine, "fine", Value::Null).await.unwrap(), "still here");

    let hog = run(&engine, "hog", Value::Null).await.unwrap_err();
    assert!(hog.reason().to_lowercase().contains("memory") || matches!(hog, PluginError::Stopped(_)), "{:?}", hog);
    assert_eq!(run(&engine, "fine", Value::Null).await.unwrap(), "still here");

    let deep = run(&engine, "recurse", Value::Null).await.unwrap_err();
    assert!(deep.reason().to_lowercase().contains("stack"), "{:?}", deep);

    let boom = run(&engine, "boom", Value::Null).await.unwrap_err();
    assert_eq!(boom, PluginError::Script("the site changed".to_string()));

    let cyclic = run(&engine, "cyclic", Value::Null).await.unwrap_err();
    assert!(cyclic.reason().contains("cannot be serialised"), "{:?}", cyclic);

    let never = engine
        .call("run", &[json!("never"), Value::Null], Duration::from_millis(200), SessionUse::Existing)
        .await
        .unwrap_err();
    assert_eq!(never, PluginError::Timeout);

    let unknown = engine
        .call("nothingLikeThis", &[], Duration::from_secs(2), SessionUse::Existing)
        .await
        .unwrap_err();
    assert!(unknown.reason().contains("is not a function"), "{:?}", unknown);
    assert_eq!(run(&engine, "fine", Value::Null).await.unwrap(), "still here");
}

#[tokio::test(flavor = "multi_thread")]
async fn broken_payloads_are_reported_at_load() {
    let syntax = PluginEngine::start(spec("bad-syntax", "class Provider { getSettings( { }")).err().unwrap();
    assert!(!syntax.is_empty());

    let missing = PluginEngine::start(spec("no-provider", "const x = 1;")).err().unwrap();
    assert!(missing.contains("does not define a Provider class"), "{}", missing);

    let throws = PluginEngine::start(spec("throws", "throw new Error('needs Seanime');")).err().unwrap();
    assert!(throws.contains("needs Seanime"), "{}", throws);

    let mut typescript = spec("bad-ts", "class Provider { getSettings(): { return 1 } ");
    typescript.typescript = true;
    let reason = PluginEngine::start(typescript).err().unwrap();
    assert!(reason.contains("TypeScript"), "{}", reason);

    let mut spinning = spec("spins", "while (true) {}");
    spinning.limits.slice = Duration::from_millis(300);
    let reason = PluginEngine::start(spinning).err().unwrap();
    assert!(reason.contains("too long"), "{}", reason);
}

#[test]
fn typescript_is_reduced_to_javascript() {
    let source = r#"
/// <reference path="./online-streaming-provider.d.ts" />
type Hit = { id: string }
enum Kind { Sub = "sub", Dub = "dub" }
interface Options { query: string; dub?: boolean }
class Provider {
    private api: string = "https://example.org"
    constructor(private readonly retries: number = 3) {}
    getSettings(): Settings { return { episodeServers: ["a"], supportsDub: true } }
    async search(opts: Options): Promise<Hit[]> {
        const kind: Kind = opts.dub ? Kind.Dub : Kind.Sub
        return [{ id: `${this.api}/${kind}/${this.retries}` } as Hit]
    }
}
"#;
    let script = typescript_to_javascript(source).unwrap();
    assert!(script.contains("class Provider"));
    assert!(!script.contains("interface Options"));
    assert!(!script.contains(": Settings"));
}

#[tokio::test(flavor = "multi_thread")]
async fn transpiled_typescript_runs() {
    let mut ts = spec(
        "ts",
        r#"enum Kind { Sub = "sub", Dub = "dub" }
class Provider {
    private api: string = "https://example.org"
    constructor(private readonly retries: number = 3) {}
    getSettings(): { episodeServers: string[]; supportsDub: boolean } { return { episodeServers: ["a"], supportsDub: true } }
    async search(opts: { dub?: boolean }): Promise<{ id: string }[]> {
        const kind: Kind = opts.dub ? Kind.Dub : Kind.Sub
        return [{ id: `${this.api}/${kind}/${this.retries}` }]
    }
}"#,
    );
    ts.typescript = true;
    let engine = PluginRuntime::new(ts);
    let hits = engine
        .call("search", &[json!({ "dub": true })], Duration::from_secs(5), SessionUse::Existing)
        .await
        .unwrap();
    assert_eq!(hits, json!([{ "id": "https://example.org/dub/3" }]));
}

/// Every onlinestream provider in the two reference repositories next to the app, as
/// (manifest, payload). Empty when the repositories are not there.
fn community_plugins() -> Vec<(Value, String)> {
    let mut found = Vec::new();
    for root in ["Seanime-Providers-main/src/anime", "Seanime-streaming-providers-main/src"] {
        let Ok(entries) = std::fs::read_dir(reference_repo(root)) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let Ok(files) = std::fs::read_dir(&dir) else { continue };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Ok(manifest) = serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap_or_default()) else {
                    continue;
                };
                if manifest["type"] != "onlinestream-provider" {
                    continue;
                }
                let payload = match manifest["payload"].as_str().filter(|p| !p.trim().is_empty()) {
                    Some(inline) => inline.to_string(),
                    None => {
                        let name = manifest["payloadURI"].as_str().unwrap_or_default().rsplit('/').next().unwrap_or_default().to_string();
                        match std::fs::read_to_string(dir.join(&name)) {
                            Ok(source) => source,
                            Err(_) => continue,
                        }
                    }
                };
                found.push((manifest, payload));
            }
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread")]
async fn community_payloads_load_and_report_their_settings() {
    let plugins = community_plugins();
    if plugins.is_empty() {
        eprintln!("skipped: the Seanime reference repositories are not next to the app");
        return;
    }
    let mut javascript = 0;
    let mut typescript = 0;
    for (manifest, payload) in &plugins {
        let id = manifest["id"].as_str().unwrap_or("unknown");
        // As the app runs a plugin nobody configured: its settings at their defaults.
        let config = UserConfig::from_manifest(manifest);
        let (code, preferences) = userconfig::apply(payload, config.as_ref(), None).expect("defaults apply");
        if let Some(config) = &config {
            for field in &config.fields {
                assert!(!code.contains(&format!("{{{{{}}}}}", field.name)), "{}: {} left in the code", id, field.name);
            }
        }
        let mut plugin = spec(id, &code);
        plugin.preferences = preferences;
        plugin.typescript = manifest["language"] == "typescript";
        let engine = PluginRuntime::new(plugin);
        let settings = engine
            .call("getSettings", &[], Duration::from_secs(5), SessionUse::Existing)
            .await
            .unwrap_or_else(|e| panic!("{} did not answer getSettings: {}", id, e));
        assert!(settings["episodeServers"].is_array(), "{}: {}", id, settings);
        assert!(settings["supportsDub"].is_boolean(), "{}: {}", id, settings);
        if manifest["language"] == "typescript" {
            typescript += 1;
        } else {
            javascript += 1;
        }
    }
    assert!(javascript >= 1 && typescript >= 1, "{} JavaScript, {} TypeScript", javascript, typescript);
    eprintln!("loaded {} JavaScript and {} TypeScript community providers", javascript, typescript);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_community_provider_searches_a_site() {
    let plugins = community_plugins();
    let Some((_, payload)) = plugins.iter().find(|(manifest, _)| manifest["id"] == "animegg") else {
        eprintln!("skipped: the Seanime reference repositories are not next to the app");
        return;
    };
    let site = Site::start(|request| match request.route() {
        "/search/" => Resp::html(
            r#"<div><a href="/series/one-piece" class="mse"><img src="x.jpg"><div class="media-body"><h2>One Piece</h2></div></a>
<a href="/series/one-piece-film-red" class="mse"><div><h2>One Piece Film: Red</h2></div></a></div>"#,
        ),
        "/series/one-piece" => Resp::html(
            r#"<a href="/one-piece-episode-2" class="anm_det_pop"><strong>One Piece 2</strong><i class="anititle">Enter the Great Swordsman!</i></a>
<a href="/one-piece-episode-1" class="anm_det_pop"><strong>One Piece 1</strong><i class="anititle">I'm Luffy!</i></a>"#,
        ),
        _ => Resp::status(404),
    })
    .await;
    assert!(payload.contains("https://www.animegg.org"));
    let local = payload.replace("https://www.animegg.org", &site.base());
    let engine = runtime("animegg", &local);

    let hits = engine
        .call("search", &[json!({ "query": "One Piece", "dub": false, "media": { "id": 21 } })], Duration::from_secs(5), SessionUse::Existing)
        .await
        .unwrap();
    assert_eq!(hits[0]["id"], "one-piece");
    assert_eq!(hits[0]["title"], "One Piece");
    assert_eq!(hits[1]["id"], "one-piece-film-red");
    assert_eq!(site.count("/search/?q=One%20Piece"), 1);

    let episodes = engine
        .call("findEpisodes", &[json!("one-piece")], Duration::from_secs(5), SessionUse::Existing)
        .await
        .unwrap();
    assert_eq!(episodes.as_array().unwrap().len(), 2);
    assert_eq!(episodes[1]["number"], 1);
    assert_eq!(episodes[1]["title"], "I'm Luffy!");
}

// A single-threaded runtime on purpose: a plugin start on its one thread would stop every
// other task until the plugin's code has loaded.
#[tokio::test]
async fn starting_a_slow_plugin_leaves_async_work_running() {
    let engine = runtime(
        "slow-start",
        "const until = Date.now() + 800; while (Date.now() < until) {}\nclass Provider { getSettings() { return { episodeServers: [], supportsDub: false }; } }",
    );
    let ticks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = ticks.clone();
    let ticker = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });
    assert!(engine.running().is_none(), "nothing is started before the first call");
    engine.call("getSettings", &[], Duration::from_secs(5), SessionUse::Existing).await.unwrap();
    ticker.abort();
    let ticked = ticks.load(std::sync::atomic::Ordering::SeqCst);
    assert!(ticked >= 10, "other tasks ran only {} times while the plugin loaded", ticked);
    assert!(engine.running().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn plugins_do_not_share_state() {
    let a = runtime("a", "globalThis.shared = 'from a'; class Provider { getSettings() { return { mine: globalThis.shared, id: $anilili.pluginId }; } }");
    let b = runtime("b", "class Provider { getSettings() { return { mine: typeof globalThis.shared, id: $anilili.pluginId }; } }");
    let call = |engine: Arc<PluginRuntime>| async move {
        engine.call("getSettings", &[], Duration::from_secs(5), SessionUse::Existing).await.unwrap()
    };
    assert_eq!(call(a).await, json!({ "mine": "from a", "id": "a" }));
    assert_eq!(call(b).await, json!({ "mine": "undefined", "id": "b" }));
}

#[tokio::test(flavor = "multi_thread")]
async fn scanner_utils_are_available_to_plugins() {
    let engine = runtime(
        "scanner",
        r#"
class Provider {
  getSettings() { return { episodeServers: [], supportsDub: false }; }
  search(opts) {
    const u = $scannerUtils;
    const thrown = (run) => { try { run(); return null; } catch (e) { return String(e && e.message ? e.message : e); } };
    return {
      names: Object.keys(u).sort(),
      season: u.buildSeasonQuery('Overlord', 2),
      part: u.buildPartQuery('Re:Zero', 2),
      sanitized: u.sanitizeQuery('Fate/stay night [UBW] (2014)!'),
      query: u.buildSearchQuery(opts.query),
      smart: u.buildSmartSearchTitles([opts.query, 'Attack on Titan Season 2']),
      normalized: Object.keys(u.normalizeTitle(opts.query)).sort(),
      seasonNumber: u.extractSeasonNumber('Kono Subarashii Sekai ni Shukufuku wo! 2'),
      year: u.extractYear('Hunter x Hunter (2011)'),
      same: u.compareTitles('One Piece', 'ONE PIECE'),
      best: u.findBestMatch('One Piece', ['Two Piece', 'One Piece', 'One Punch Man']),
      noArgs: thrown(() => u.normalizeTitle()),
      wrongType: thrown(() => u.buildSearchQuery(5)),
      frozen: thrown(() => { 'use strict'; u.normalizeTitle = null; }),
    };
  }
}
"#,
    );
    let out = engine
        .call("search", &[json!({ "query": "Shingeki no Kyojin Season 2" })], Duration::from_secs(5), SessionUse::Existing)
        .await
        .unwrap();
    assert_eq!(
        out["names"],
        json!([
            "buildAdvancedQuery", "buildPartQuery", "buildSearchQuery", "buildSeasonQuery", "buildSmartSearchTitles",
            "compareTitles", "extractPartNumber", "extractSeasonNumber", "extractYear", "findBestMatch",
            "getSignificantTokens", "normalizeTitle", "sanitizeQuery"
        ])
    );
    // The values Seanime's own functions give for these arguments.
    assert_eq!(out["season"], json!("(overlord S02 | overlord S2 | overlord Season 2 | overlord 2nd Season)"));
    assert_eq!(out["part"], json!("(re zero Part 2 | re zero Part II | re zero 2nd Cour)"));
    assert_eq!(out["sanitized"], json!("Fate/stay night UBW 2014"));
    assert_eq!(out["seasonNumber"], json!(2));
    assert_eq!(out["year"], json!(2011));
    assert_eq!(out["same"].as_f64(), Some(1.0));
    assert_eq!(out["best"], json!("One Piece"));
    assert_eq!(out["smart"]["season"], json!(2));
    assert!(out["smart"]["titles"].as_array().map_or(false, |titles| !titles.is_empty()), "{}", out["smart"]);
    assert!(out["query"].as_str().map_or(false, |query| query.contains("shingeki")), "{}", out["query"]);
    assert_eq!(
        out["normalized"],
        json!(["cleanBaseTitle", "denoisedTitle", "isExtra", "isMain", "normalized", "original", "part", "season", "tokens", "year"])
    );
    assert_eq!(out["noArgs"], json!("TypeError: normalizeTitle requires at least 1 argument"));
    assert_eq!(out["wrongType"], json!("argument is not a string"));
    assert!(out["frozen"].is_string(), "{}", out["frozen"]);
}

/// The community plugins that take their site address from `userConfig` placeholders: with the
/// manifest's defaults in place they load and build real addresses.
#[tokio::test(flavor = "multi_thread")]
async fn community_placeholders_are_replaced_by_their_defaults() {
    let plugins = community_plugins();
    if plugins.is_empty() {
        eprintln!("skipped: the Seanime reference repositories are not next to the app");
        return;
    }
    let mut checked = Vec::new();
    for (manifest, payload) in &plugins {
        let Some(config) = UserConfig::from_manifest(manifest) else { continue };
        let used: Vec<_> = config
            .fields
            .iter()
            .filter(|field| payload.contains(&format!("{{{{{}}}}}", field.name)))
            .collect();
        if used.is_empty() {
            continue;
        }
        let id = manifest["id"].as_str().unwrap_or("unknown").to_string();
        let (code, preferences) = userconfig::apply(payload, Some(&config), None).unwrap();
        for field in &used {
            assert!(!code.contains(&format!("{{{{{}}}}}", field.name)), "{}: {}", id, field.name);
            assert!(code.contains(&field.default), "{}: default of {}", id, field.name);
            assert_eq!(preferences[&field.name], field.default);
        }
        let mut plugin = spec(&id, &code);
        plugin.typescript = manifest["language"] == "typescript";
        plugin.preferences = preferences;
        PluginEngine::start(plugin).unwrap_or_else(|e| panic!("{} does not load: {}", id, e));
        checked.push(id.to_ascii_lowercase());
    }
    checked.sort();
    assert_eq!(checked, ["animekai", "animeunity", "gojowtf"]);
}
