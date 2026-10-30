use anix_lib::stream_proxy::{
    ass_to_vtt, parse_target_url, proxied_url, rewrite_playlist, srt_to_vtt,
    starts_with_playlist_tag, subtitle_to_vtt, UpstreamParams,
};
use std::sync::Arc;
use tiny_http::{Header, Response, Server, StatusCode};
use url::Url;

const PROXY: &str = "http://127.0.0.1:9/token";

fn params() -> UpstreamParams {
    UpstreamParams {
        referer: Some("https://provider.example/".to_string()),
        origin: None,
        headers_b64: None,
    }
}

#[test]
fn test_stream_proxy_parse_target_url_only_http() {
    assert!(parse_target_url("https://cdn.example/a.m3u8").is_ok());
    assert!(parse_target_url("http://cdn.example/a.mp4").is_ok());
    for rejected in ["file:///etc/passwd", "ftp://x/y", "/etc/passwd", "data:text/plain,x", ""] {
        assert!(parse_target_url(rejected).is_err(), "{:?}", rejected);
    }
}

#[test]
fn test_stream_proxy_request_headers() {
    // Origin is derived from the referer when the provider gave none.
    assert_eq!(
        params().request_headers(),
        vec![
            ("Referer".to_string(), "https://provider.example/".to_string()),
            ("Origin".to_string(), "https://provider.example".to_string()),
        ]
    );

    // {"Referer":"https://other.example/","X-Token":"abc","Host":"evil","Bad Name":"x"}
    let encoded = "eyJSZWZlcmVyIjoiaHR0cHM6Ly9vdGhlci5leGFtcGxlLyIsIlgtVG9rZW4iOiJhYmMiLCJIb3N0IjoiZXZpbCIsIkJhZCBOYW1lIjoieCJ9";
    let with_custom = UpstreamParams {
        referer: Some("https://provider.example/".to_string()),
        origin: Some("https://explicit.example".to_string()),
        headers_b64: Some(encoded.to_string()),
    };
    let headers = with_custom.request_headers();
    // One Referer only, the stream's own header map winning; hop-by-hop and malformed names dropped.
    assert_eq!(headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case("referer")).count(), 1);
    assert!(headers.contains(&("Referer".to_string(), "https://other.example/".to_string())));
    assert!(headers.contains(&("Origin".to_string(), "https://explicit.example".to_string())));
    assert!(headers.contains(&("X-Token".to_string(), "abc".to_string())));
    assert!(!headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("host") || n.contains(' ')));

    let garbage = UpstreamParams { headers_b64: Some("!!not base64!!".to_string()), ..Default::default() };
    assert!(garbage.request_headers().is_empty());
}

#[test]
fn test_stream_proxy_rewrite_playlist() {
    let playlist_url = Url::parse("https://cdn.example/show/ep1/master.m3u8?token=abc").unwrap();
    let playlist = "#EXTM3U\n\
        #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",URI=\"audio/en.m3u8\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n\
        720/index.m3u8\n\
        #EXT-X-KEY:METHOD=AES-128,URI=\"https://keys.example/k?id=1&x=2\",IV=0x1\n\
        #EXTINF:4.0,\n\
        /abs/seg0.ts\n\
        //other.example/seg1.ts\n";

    let out = rewrite_playlist(playlist, &playlist_url, PROXY, &params());
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "#EXTM3U");

    let expect = |target: &str| proxied_url(PROXY, &Url::parse(target).unwrap(), &params());
    assert_eq!(
        lines[1],
        format!(
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",URI=\"{}\"",
            expect("https://cdn.example/show/ep1/audio/en.m3u8")
        )
    );
    assert_eq!(lines[2], "#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720");
    assert_eq!(lines[3], expect("https://cdn.example/show/ep1/720/index.m3u8"));
    assert_eq!(
        lines[4],
        format!(
            "#EXT-X-KEY:METHOD=AES-128,URI=\"{}\",IV=0x1",
            expect("https://keys.example/k?id=1&x=2")
        )
    );
    assert_eq!(lines[6], expect("https://cdn.example/abs/seg0.ts"));
    assert_eq!(lines[7], expect("https://other.example/seg1.ts"));

    // Every rewritten URL carries the referer and stays a single unbroken token.
    assert!(lines[3].starts_with("http://127.0.0.1:9/token/proxy?url=https%3A%2F%2Fcdn.example"));
    assert!(lines[3].contains("referer=https%3A%2F%2Fprovider.example%2F"));
    assert!(!lines[4].contains("id=1&x=2"), "nested query must be encoded: {}", lines[4]);
}

#[test]
fn test_stream_proxy_playlist_sniffing() {
    assert!(starts_with_playlist_tag(b"#EXTM3U\n#EXT-X-VERSION:3"));
    assert!(starts_with_playlist_tag(b"\xEF\xBB\xBF\r\n  #EXTM3U"));
    assert!(!starts_with_playlist_tag(b"\x00\x00\x00\x18ftypmp42"));
    assert!(!starts_with_playlist_tag(b""));
}

#[test]
fn test_stream_proxy_subtitle_conversion() {
    let srt = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,500\r\nHello, world\r\n\r\n2\r\n00:01:00,010 --> 00:01:01,000\r\nBye\r\n";
    let vtt = srt_to_vtt(srt);
    assert!(vtt.starts_with("WEBVTT\n\n1\n00:00:01.000 --> 00:00:02.500\nHello, world\n"));
    assert!(vtt.contains("00:01:00.010 --> 00:01:01.000"));
    assert!(!vtt.contains('\r'));

    let ass = "[Script Info]\nTitle: x\n\n[Events]\n\
        Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
        Dialogue: 0,0:00:05.50,0:00:07.00,Default,,0,0,0,,{\\an8\\b1}Second, line\\Nbreak\n\
        Dialogue: 0,0:00:01.00,0:00:02.25,Default,,0,0,0,,First\n\
        Comment: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,ignored\n";
    let vtt = ass_to_vtt(ass);
    assert_eq!(
        vtt,
        "WEBVTT\n\n00:00:01.000 --> 00:00:02.250\nFirst\n\n00:00:05.500 --> 00:00:07.000\nSecond, line\nbreak\n\n"
    );

    assert_eq!(subtitle_to_vtt("WEBVTT\n\n00:01.000 --> 00:02.000\nHi"), "WEBVTT\n\n00:01.000 --> 00:02.000\nHi");
    assert!(subtitle_to_vtt(ass).contains("00:00:05.500 --> 00:00:07.000"));
    assert!(subtitle_to_vtt(srt).contains("00:00:01.000 --> 00:00:02.500"));
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

const MOVIE_BYTES: usize = 300_000;

fn movie_bytes() -> Vec<u8> {
    (0..MOVIE_BYTES as u32).map(|i| (i.wrapping_mul(31) % 253) as u8).collect()
}

/// A stand-in for a provider CDN: refuses requests without the provider's Referer.
fn start_fake_cdn() -> (Arc<Server>, u16) {
    let server = Arc::new(Server::http("127.0.0.1:0").unwrap());
    let port = server.server_addr().to_ip().unwrap().port();
    let worker = server.clone();
    std::thread::spawn(move || {
        for request in worker.incoming_requests() {
            let get = |name: &'static str| {
                request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv(name))
                    .map(|h| h.value.as_str().to_string())
            };
            let referer_ok = get("Referer").as_deref() == Some("https://provider.example/");
            let origin_ok = get("Origin").as_deref() == Some("https://provider.example");
            let path = request.url().split('?').next().unwrap_or("").to_string();

            if !(referer_ok && origin_ok) {
                let _ = request.respond(Response::from_string("hotlinking denied").with_status_code(403));
                continue;
            }
            match path.as_str() {
                "/show/master" => {
                    // No .m3u8 extension and a generic content type: must be sniffed.
                    let body = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nlow/index.m3u8\n";
                    let _ = request.respond(
                        Response::from_string(body).with_header(header("Content-Type", "text/plain")),
                    );
                }
                "/show/low/index.m3u8" => {
                    let body = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:4,\nseg0.ts\n#EXT-X-ENDLIST\n";
                    let _ = request.respond(
                        Response::from_string(body)
                            .with_header(header("Content-Type", "application/vnd.apple.mpegurl")),
                    );
                }
                "/show/low/seg0.ts" => {
                    let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
                    let range = get("Range").and_then(|r| {
                        let (start, end) = r.strip_prefix("bytes=")?.split_once('-')?;
                        Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
                    });
                    match range {
                        Some((start, end)) => {
                            let slice = data[start..=end].to_vec();
                            let _ = request.respond(
                                Response::from_data(slice)
                                    .with_status_code(StatusCode(206))
                                    .with_header(header("Content-Type", "video/mp2t"))
                                    .with_header(header(
                                        "Content-Range",
                                        &format!("bytes {}-{}/1000", start, end),
                                    )),
                            );
                        }
                        None => {
                            let _ = request.respond(
                                Response::from_data(data).with_header(header("Content-Type", "video/mp2t")),
                            );
                        }
                    }
                }
                "/show/movie.mp4" => {
                    // A file host as the Italian CDNs behave: ranges of every form, a 206 that
                    // does not repeat Accept-Ranges, HEAD with the full length.
                    let data = movie_bytes();
                    let total = data.len();
                    let range = get("Range").and_then(|r| {
                        let (start, end) = r.strip_prefix("bytes=")?.split_once('-')?;
                        match (start.parse::<usize>().ok(), end.parse::<usize>().ok()) {
                            (Some(start), Some(end)) => Some((start, end.min(total - 1))),
                            (Some(start), None) => Some((start, total - 1)),
                            (None, Some(suffix)) => Some((total - suffix.min(total), total - 1)),
                            (None, None) => None,
                        }
                    });
                    let response = match range {
                        Some((start, end)) => Response::from_data(data[start..=end].to_vec())
                            .with_status_code(StatusCode(206))
                            .with_header(header("Content-Range", &format!("bytes {}-{}/{}", start, end, total))),
                        None => Response::from_data(data).with_header(header("Accept-Ranges", "bytes")),
                    };
                    let _ = request.respond(
                        response
                            .with_header(header("Content-Type", "video/mp4"))
                            .with_header(header("ETag", "\"movie\""))
                            .with_chunked_threshold(usize::MAX),
                    );
                }
                "/show/sub.srt" => {
                    let _ = request.respond(Response::from_string(
                        "1\n00:00:01,000 --> 00:00:02,000\nHello\n",
                    ));
                }
                _ => {
                    let _ = request.respond(Response::from_string("missing").with_status_code(404));
                }
            }
        }
    });
    (server, port)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_stream_proxy_end_to_end() {
    let (_cdn, port) = start_fake_cdn();
    let base = anix_lib::stream_proxy::start_allowing_private_upstream().expect("proxy starts");
    assert!(base.starts_with("http://127.0.0.1:"));

    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let cdn = |path: &str| format!("http://127.0.0.1:{}{}", port, path);
    let via = |endpoint: &str, target: &str, referer: Option<&str>| {
        let mut url = Url::parse(&format!("{}/{}", base, endpoint)).unwrap();
        url.query_pairs_mut().append_pair("url", target);
        if let Some(r) = referer {
            url.query_pairs_mut().append_pair("referer", r);
        }
        url.to_string()
    };
    let provider = Some("https://provider.example/");

    // Without the provider's headers the CDN refuses, exactly what the webview hits on its own.
    let denied = client.get(via("proxy", &cdn("/show/master"), None)).send().await.unwrap();
    assert_eq!(denied.status(), 403);

    // With them, the master playlist comes back rewritten and CORS-readable.
    let master = client.get(via("proxy", &cdn("/show/master"), provider)).send().await.unwrap();
    assert_eq!(master.status(), 200);
    assert_eq!(master.headers()["access-control-allow-origin"], "*");
    assert!(master.headers()["content-type"].to_str().unwrap().contains("mpegurl"));
    let master_text = master.text().await.unwrap();
    let variant_url = master_text
        .lines()
        .find(|l| !l.starts_with('#') && !l.is_empty())
        .expect("variant line")
        .to_string();
    assert!(variant_url.starts_with(&format!("{}/proxy?url=", base)), "{}", variant_url);

    // The nested playlist is reachable through the rewritten URL alone.
    let variant_text = client.get(&variant_url).send().await.unwrap().text().await.unwrap();
    let segment_url = variant_text
        .lines()
        .find(|l| !l.starts_with('#') && !l.is_empty())
        .expect("segment line")
        .to_string();
    assert!(segment_url.starts_with(&format!("{}/proxy?url=", base)));
    assert!(variant_text.contains(&format!("URI=\"{}/proxy?url=", base)));

    // Segments stream through, and Range requests are honoured for seeking.
    let full = client.get(&segment_url).send().await.unwrap();
    assert_eq!(full.status(), 200);
    assert_eq!(full.bytes().await.unwrap().len(), 1000);

    let ranged = client.get(&segment_url).header("Range", "bytes=10-19").send().await.unwrap();
    assert_eq!(ranged.status(), 206);
    assert_eq!(ranged.headers()["content-range"], "bytes 10-19/1000");
    assert_eq!(ranged.bytes().await.unwrap().as_ref(), &[10u8, 11, 12, 13, 14, 15, 16, 17, 18, 19]);

    // Subtitles arrive as WebVTT.
    let subtitle = client.get(via("subtitle", &cdn("/show/sub.srt"), provider)).send().await.unwrap();
    assert_eq!(subtitle.status(), 200);
    assert!(subtitle.headers()["content-type"].to_str().unwrap().starts_with("text/vtt"));
    assert!(subtitle.text().await.unwrap().starts_with("WEBVTT\n\n1\n00:00:01.000 --> 00:00:02.000"));

    // The proxy is not an open one: wrong token, non-http targets and other endpoints are refused.
    let port_and_token = base.trim_start_matches("http://");
    let proxy_port = port_and_token.split('/').next().unwrap();
    let wrong_token = format!("http://{}/deadbeef/proxy?url={}", proxy_port, cdn("/show/master"));
    assert_eq!(client.get(wrong_token).send().await.unwrap().status(), 403);
    assert_eq!(client.get(via("proxy", "file:///etc/passwd", None)).send().await.unwrap().status(), 400);
    assert_eq!(client.get(via("other", &cdn("/show/master"), provider)).send().await.unwrap().status(), 404);
    assert_eq!(client.get(format!("{}/proxy", base)).send().await.unwrap().status(), 400);

    // Upstream failures surface as errors the player can show.
    let missing = client.get(via("proxy", &cdn("/show/nope"), provider)).send().await.unwrap();
    assert_eq!(missing.status(), 404);
    let unreachable = client.get(via("proxy", "http://127.0.0.1:1/x.m3u8", provider)).send().await.unwrap();
    assert_eq!(unreachable.status(), 502);
}

/// A media element needs the length of what it is sent: without it WebKit cannot seek and
/// refuses a file whose index sits at its end.
#[tokio::test(flavor = "multi_thread")]
async fn test_stream_proxy_serves_files_with_their_length_for_every_kind_of_range() {
    let (_cdn, port) = start_fake_cdn();
    let base = anix_lib::stream_proxy::start_allowing_private_upstream().expect("proxy starts");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut url = Url::parse(&format!("{}/proxy", base)).unwrap();
    url.query_pairs_mut()
        .append_pair("url", &format!("http://127.0.0.1:{}/show/movie.mp4", port))
        .append_pair("referer", "https://provider.example/");
    let data = movie_bytes();
    let total = data.len();

    let header_of = |response: &reqwest::Response, name: &str| {
        response.headers().get(name).map(|v| v.to_str().unwrap().to_string())
    };

    let plain = client.get(url.clone()).send().await.unwrap();
    assert_eq!(plain.status(), 200);
    assert_eq!(header_of(&plain, "content-length"), Some(total.to_string()));
    assert_eq!(header_of(&plain, "transfer-encoding"), None);
    assert_eq!(header_of(&plain, "accept-ranges").as_deref(), Some("bytes"));
    assert_eq!(header_of(&plain, "content-type").as_deref(), Some("video/mp4"));
    assert_eq!(header_of(&plain, "etag").as_deref(), Some("\"movie\""));
    assert_eq!(plain.bytes().await.unwrap().as_ref(), data.as_slice());

    // (Range sent, first byte, last byte) for an open-ended, a mid-file and a suffix range.
    let cases = [
        ("bytes=0-", 0, total - 1),
        ("bytes=100000-249999", 100_000, 249_999),
        ("bytes=250000-", 250_000, total - 1),
        ("bytes=-4096", total - 4096, total - 1),
    ];
    for (range, first, last) in cases {
        let ranged = client.get(url.clone()).header("Range", range).send().await.unwrap();
        assert_eq!(ranged.status(), 206, "{}", range);
        assert_eq!(
            header_of(&ranged, "content-range"),
            Some(format!("bytes {}-{}/{}", first, last, total)),
            "{}",
            range
        );
        assert_eq!(header_of(&ranged, "content-length"), Some((last - first + 1).to_string()), "{}", range);
        assert_eq!(header_of(&ranged, "transfer-encoding"), None, "{}", range);
        // The host did not repeat Accept-Ranges on its partial answer; the proxy states it.
        assert_eq!(header_of(&ranged, "accept-ranges").as_deref(), Some("bytes"), "{}", range);
        assert_eq!(ranged.bytes().await.unwrap().as_ref(), &data[first..=last], "{}", range);
    }

    let head = client.head(url.clone()).send().await.unwrap();
    assert_eq!(head.status(), 200);
    assert_eq!(header_of(&head, "content-length"), Some(total.to_string()));
    assert_eq!(header_of(&head, "transfer-encoding"), None);
    assert_eq!(header_of(&head, "accept-ranges").as_deref(), Some("bytes"));
    assert_eq!(header_of(&head, "content-type").as_deref(), Some("video/mp4"));
    assert!(head.bytes().await.unwrap().is_empty());
}
