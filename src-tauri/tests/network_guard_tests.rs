//! Extensions must not reach the user's machine or network: the address rule, the resolver
//! filter, redirect hops, stream checks and the stream proxy's upstream. This file starts the
//! proxy with `start()` (no test allowance), so it is kept apart from `stream_proxy_tests.rs`.

use anix_lib::clients::net_guard::{
    check_public_url, is_private_address, public_download_client, public_http_client, PublicResolver,
};
use anix_lib::models::StreamItem;
use anix_lib::providers::manager::{verify_stream_segment, verify_streams};
use reqwest::dns::{Name, Resolve};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use url::Url;

/// A loopback server that counts every connection it accepts and answers each with `reply`.
struct Counting {
    port: u16,
    connections: Arc<AtomicUsize>,
}

impl Counting {
    fn start(bind: IpAddr, reply: String) -> Counting {
        let listener = TcpListener::bind((bind, 0)).expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let connections = Arc::new(AtomicUsize::new(0));
        let seen = connections.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                seen.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        Counting { port, connections }
    }

    fn ok(bind: IpAddr) -> Counting {
        Counting::start(bind, "HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\n#EXTM3U".to_string())
    }

    fn redirecting_to(location: &str) -> Counting {
        Counting::start(
            "127.0.0.1".parse().unwrap(),
            format!("HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", location),
        )
    }

    fn seen(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

/// A name from `/etc/hosts` that points at loopback without looking like it (`localhost4`,
/// `ip6-localhost`, …), with its address. Names with a dot are skipped so the lookup stays on
/// the system resolver and never goes out to DNS-over-HTTPS.
fn loopback_alias() -> Option<(String, IpAddr)> {
    let hosts = std::fs::read_to_string("/etc/hosts").ok()?;
    hosts.lines().find_map(|line| {
        let mut fields = line.split('#').next()?.split_whitespace();
        let ip: IpAddr = fields.next()?.parse().ok()?;
        if !ip.is_loopback() {
            return None;
        }
        let name = fields.find(|name| !name.contains('.') && *name != "localhost")?;
        Some((name.to_string(), ip))
    })
}

fn hls(url: String) -> StreamItem {
    StreamItem::new(url, "hls".to_string(), Some("auto".to_string()), Some("sub".to_string()), None, true)
}

fn file(url: String) -> StreamItem {
    StreamItem::new(url, "mp4".to_string(), None, None, None, true)
}

#[test]
fn only_public_internet_addresses_count_as_public() {
    let private = [
        "127.0.0.1", "127.1.2.3", "10.0.0.1", "172.16.0.1", "172.31.255.255", "192.168.1.1",
        "169.254.169.254", "100.64.0.1", "100.127.255.255", "0.0.0.0", "0.1.2.3", "224.0.0.1",
        "240.0.0.1", "255.255.255.255", "::1", "::", "::ffff:127.0.0.1", "::ffff:192.168.0.1",
        "::ffff:169.254.169.254", "::127.0.0.1", "fc00::1", "fd12:3456::1", "fe80::1", "fec0::1",
        "ff02::1", "64:ff9b::7f00:1", "64:ff9b::a00:1",
    ];
    for ip in private {
        assert!(is_private_address(ip.parse().unwrap()), "{} must be refused", ip);
    }
    // 49.44.79.236 is where one ISP sinkholes blocked names: a public address, not special.
    let public = [
        "8.8.8.8", "1.1.1.1", "49.44.79.236", "100.63.255.255", "100.128.0.1", "172.32.0.1",
        "192.169.0.1", "223.255.255.255", "2606:4700::6810:f8f9", "2001:4860:4860::8888",
        "::ffff:8.8.8.8", "64:ff9b::808:808",
    ];
    for ip in public {
        assert!(!is_private_address(ip.parse().unwrap()), "{} must be allowed", ip);
    }
}

#[test]
fn urls_naming_this_machine_or_the_lan_are_refused_as_written() {
    for raw in [
        "http://localhost:8080/",
        "http://LOCALHOST./x",
        "http://cdn.localhost/x",
        "http://127.0.0.1/x",
        "http://2130706433/x",
        "http://0x7f.1/x",
        "http://0.0.0.0:11434/api",
        "http://[::1]/x",
        "http://[::ffff:127.0.0.1]/x",
        "http://169.254.169.254/latest/meta-data",
        "http://100.100.100.100/",
        "https://192.168.1.1/cgi-bin/x.mp4",
    ] {
        let reason = check_public_url(&Url::parse(raw).unwrap()).unwrap_err();
        assert!(reason.contains("private network access denied"), "{}: {}", raw, reason);
    }
    for raw in ["file:///etc/passwd", "ftp://example.org/x", "data:text/plain,hi"] {
        assert!(check_public_url(&Url::parse(raw).unwrap()).is_err(), "{}", raw);
    }
    for raw in ["https://cdn.example.org/a.m3u8", "http://49.44.79.236/", "https://[2606:4700::1111]/"] {
        assert_eq!(check_public_url(&Url::parse(raw).unwrap()), Ok(()), "{}", raw);
    }
}

#[tokio::test]
async fn the_public_resolver_never_hands_out_a_private_address() {
    let answer = PublicResolver.resolve(Name::from_str("localhost").unwrap()).await;
    let reason = answer.err().expect("localhost has only loopback addresses").to_string();
    assert!(reason.contains("private network access denied"), "{}", reason);
}

#[tokio::test]
async fn a_name_that_resolves_to_loopback_is_refused_before_any_connection() {
    let Some((alias, ip)) = loopback_alias() else {
        eprintln!("skipped: /etc/hosts has no loopback alias without a dot");
        return;
    };
    let server = Counting::ok(ip);
    let url = format!("http://{}:{}/live/index.m3u8", alias, server.port);

    for client in [public_http_client().unwrap(), public_download_client().unwrap()] {
        let error = client.get(&url).send().await.expect_err("a loopback name is refused");
        assert!(format!("{:?}", error).contains("private network access denied"), "{:?}", error);
    }
    assert!(!verify_stream_segment(&hls(url.clone()), false).await);

    let base = anix_lib::stream_proxy::start().expect("proxy starts");
    let mut via = Url::parse(&format!("{}/proxy", base)).unwrap();
    via.query_pairs_mut().append_pair("url", &url);
    let answer = reqwest::Client::builder().no_proxy().build().unwrap().get(via).send().await.unwrap();
    assert_eq!(answer.status().as_u16(), 502);

    assert_eq!(server.seen(), 0, "the loopback server must never be reached");
}

#[tokio::test]
async fn a_redirect_to_a_private_address_is_not_followed() {
    let private = Counting::ok("127.0.0.1".parse().unwrap());
    let mut targets = vec![
        format!("http://127.0.0.1:{}/secret", private.port),
        format!("http://[::ffff:127.0.0.1]:{}/secret", private.port),
        format!("http://0.0.0.0:{}/secret", private.port),
        format!("http://localhost:{}/secret", private.port),
    ];
    let alias = loopback_alias().filter(|(_, ip)| ip.is_ipv4());
    if let Some((alias, _)) = &alias {
        targets.push(format!("http://{}:{}/secret", alias, private.port));
    }
    for target in targets {
        // The first hop is on loopback too, since a test has no public server; only the hop
        // matters here, as the first URL is checked by every caller before sending.
        let first = Counting::redirecting_to(&target);
        let start = format!("http://127.0.0.1:{}/start", first.port);
        for client in [public_http_client().unwrap(), public_download_client().unwrap()] {
            let error = client.get(&start).send().await.expect_err("the hop is refused");
            assert!(format!("{:?}", error).contains("private network access denied"), "{}: {:?}", target, error);
        }
        assert_eq!(first.seen(), 2);
    }
    assert_eq!(private.seen(), 0, "the private target must never be reached");
}

#[tokio::test]
async fn stream_checks_refuse_streams_on_private_addresses() {
    let private = Counting::ok("127.0.0.1".parse().unwrap());
    let at = |path: &str| format!("http://127.0.0.1:{}{}", private.port, path);
    assert!(!verify_stream_segment(&hls(at("/index.m3u8")), false).await);
    assert!(!verify_stream_segment(&file(at("/video.mp4")), false).await);
    for url in [at("/index.m3u8"), "http://192.168.1.1/x.m3u8".into(), "http://[::1]/x.m3u8".into(), "http://169.254.169.254/x.mp4".into()] {
        let reason = verify_streams(vec![hls(url.clone())], false).await.unwrap_err();
        assert!(reason.contains("private network access denied"), "{}: {}", url, reason);
    }
    assert_eq!(private.seen(), 0);

    // With the test allowance the same server is reached, so the refusal is the guard's doing.
    verify_stream_segment(&hls(at("/index.m3u8")), true).await;
    assert!(private.seen() > 0);
}

#[tokio::test]
async fn the_proxy_refuses_private_upstreams_but_still_serves_its_own_routes() {
    let private = Counting::ok("127.0.0.1".parse().unwrap());
    let base = anix_lib::stream_proxy::start().expect("proxy starts");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let via = |endpoint: &str, target: &str| {
        let mut url = Url::parse(&format!("{}/{}", base, endpoint)).unwrap();
        url.query_pairs_mut().append_pair("url", target);
        url
    };

    for target in [
        format!("http://127.0.0.1:{}/video.mp4", private.port),
        format!("http://[::ffff:127.0.0.1]:{}/video.mp4", private.port),
        "http://192.168.1.1/x.m3u8".to_string(),
        "http://169.254.169.254/latest".to_string(),
    ] {
        for endpoint in ["proxy", "subtitle"] {
            let answer = client.get(via(endpoint, &target)).send().await.unwrap();
            assert_eq!(answer.status().as_u16(), 502, "{} {}", endpoint, target);
            assert!(answer.text().await.unwrap().contains("private network access denied"));
        }
    }
    assert_eq!(private.seen(), 0);

    // A local file link points at the proxy itself and is answered without an upstream fetch.
    let own = format!("{}/local/{}", base, "0".repeat(32));
    assert_eq!(client.get(via("proxy", &own)).send().await.unwrap().status().as_u16(), 404);
}

#[tokio::test]
async fn external_players_get_local_sidecars_but_never_private_subtitle_links() {
    use anix_lib::commands::player::{fetch_subtitle_files, ExternalSubtitle};
    use anix_lib::local_media::{register_playable, PlayableKind};

    let private = Counting::ok("127.0.0.1".parse().unwrap());
    let base = anix_lib::stream_proxy::start().expect("proxy starts");
    let dir = std::env::temp_dir().join(format!("anix_guard_sidecar_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sidecar = dir.join("Show - 01.en.srt");
    let cue = "1\n00:00:01,000 --> 00:00:02,000\nHello\n";
    std::fs::write(&sidecar, cue).unwrap();
    let local = format!("{}/localsub/{}", base, register_playable(PlayableKind::Subtitle, &sidecar));

    let subtitle = |url: String| ExternalSubtitle { url, language: Some("English".to_string()) };
    let files = fetch_subtitle_files(
        &[
            subtitle(format!("http://127.0.0.1:{}/x.srt", private.port)),
            subtitle(local),
            subtitle("http://192.168.1.1/x.srt".to_string()),
            subtitle(format!("http://localhost:{}/x.srt", private.port)),
            // Another port on loopback is not this proxy, even with its token path.
            subtitle(format!("http://127.0.0.1:{}/{}/localsub/{}", private.port, "0".repeat(32), "0".repeat(32))),
        ],
        None,
    )
    .await;

    assert_eq!(files.len(), 1, "{:?}", files);
    assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), cue);
    assert_eq!(private.seen(), 0, "the loopback server must never be reached");
    let _ = std::fs::remove_file(&files[0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_users_own_proxy_names_are_let_through_the_guard() {
    use anix_lib::clients::net_guard::proxy_hosts;
    assert_eq!(
        proxy_hosts(["http://localhost:7890", "socks5h://Proxy.Lan.:1080", "127.0.0.1:8080", "clash:7890", ""]),
        vec!["localhost", "proxy.lan", "clash"]
    );
}
