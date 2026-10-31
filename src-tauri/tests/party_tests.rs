//! Watch party: the message decoder, the code exchange and sealed frames, the clock maths,
//! and a host and guests talking over 127.0.0.1.

use anix_lib::party::lan::{is_skipped_interface, parse_host_address, DEFAULT_PORT};
use anix_lib::party::protocol::{self, Category, Load, Message, ProtocolError, RateLimit, MAX_MESSAGE_BYTES};
use anix_lib::party::secure::{normalize_code, Pake, Side};
use anix_lib::party::session::{MAX_FAILED_JOINS, MAX_GUESTS, START_LEAD_MS};
use anix_lib::party::sync::{age_ms, position_after, Clock, ClockSample};
use anix_lib::party::{EndReason, Guest, Host, PartyError, PartyEvent};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

fn load() -> Load {
    Load {
        anime_id: -100_005_114,
        episode: 3.0,
        category: Category::Dub,
        language: Some("en".into()),
        extension: Some("anizone".into()),
        server: Some("Main server".into()),
        duration_ms: Some(1_420_000),
    }
}

// ------------------------------------------------------------------ protocol

#[test]
fn every_message_survives_encoding() {
    let messages = [
        Message::Hello { proto: 1, t1: 5 },
        Message::Welcome { t1: 5, t2: 9 },
        Message::Ping { t1: 1 },
        Message::Pong { t1: 1, t2: 2 },
        Message::Load(load()),
        Message::Prepare { seq: 4, position_ms: 61_000 },
        Message::Ready { seq: 4 },
        Message::State { seq: 4, paused: false, position_ms: 61_000, rate: 1.5, at_ms: 123 },
        Message::Bye {},
    ];
    for message in messages {
        assert_eq!(protocol::decode(&protocol::encode(&message)).unwrap(), message);
    }
}

#[test]
fn a_negative_stand_in_anime_id_is_accepted() {
    let bytes = protocol::encode(&Message::Load(load()));
    let Message::Load(back) = protocol::decode(&bytes).unwrap() else { panic!("not a load") };
    assert_eq!(back.anime_id, -100_005_114);
}

#[test]
fn an_oversize_frame_is_refused_before_parsing() {
    let mut big = br#"{"kind":"bye","pad":""#.to_vec();
    big.resize(MAX_MESSAGE_BYTES + 1, b'a');
    assert_eq!(protocol::decode(&big), Err(ProtocolError::TooLarge));
}

#[test]
fn unknown_kinds_extra_fields_and_missing_fields_are_refused() {
    for raw in [
        r#"{"kind":"chat","text":"hi"}"#,
        r#"{"kind":"ping","t1":1,"url":"https://example.com"}"#,
        r#"{"kind":"ping"}"#,
        r#"{"kind":"load","anime_id":1,"episode":1,"category":"sub","language":null,"extension":null,"server":null,"duration_ms":null,"path":"/etc"}"#,
        r#"{"kind":"state","seq":1,"paused":false,"position_ms":-5,"rate":1,"at_ms":0}"#,
        r#"{"kind":"load","anime_id":1,"episode":1,"category":"raw","language":null,"extension":null,"server":null,"duration_ms":null}"#,
        r#"{"t1":1}"#,
        r#"not json"#,
        r#"{"kind":"ping","t1":1}{"kind":"bye"}"#,
        r#"{"kind":"bye","url":"https://example.com"}"#,
    ] {
        assert!(protocol::decode(raw.as_bytes()).is_err(), "accepted {raw}");
    }
}

#[test]
fn values_out_of_range_are_refused() {
    let refused = |l: Load| protocol::decode(&protocol::encode(&Message::Load(l))).is_err();
    assert!(refused(Load { anime_id: 0, ..load() }));
    assert!(refused(Load { episode: 1e9, ..load() }));
    assert!(refused(Load { episode: -1.0, ..load() }));
    assert!(refused(Load { language: Some("EN".into()), ..load() }));
    assert!(refused(Load { language: Some("english".into()), ..load() }));
    assert!(refused(Load { extension: Some("../x".into()), ..load() }));
    assert!(refused(Load { extension: Some("a".repeat(65)), ..load() }));
    assert!(refused(Load { server: Some("a\nb".into()), ..load() }));
    assert!(refused(Load { server: Some(String::new()), ..load() }));
    assert!(refused(Load { duration_ms: Some(90_000_000), ..load() }));
    let state = |rate: f64, position_ms: u64| Message::State { seq: 1, paused: false, position_ms, rate, at_ms: 0 };
    assert!(protocol::decode(&protocol::encode(&state(10.0, 0))).is_err());
    assert!(protocol::decode(&protocol::encode(&state(0.1, 0))).is_err());
    assert!(protocol::decode(&protocol::encode(&state(1.0, 90_000_000))).is_err());
    assert!(protocol::decode(&protocol::encode(&Message::Hello { proto: 2, t1: 0 })).is_err());
    // JSON has no NaN; a non-finite rate cannot even be encoded as a number.
    assert!(protocol::validate(&state(f64::NAN, 0)).is_err());
}

#[test]
fn each_side_sends_only_its_own_kinds() {
    assert!(Message::State { seq: 1, paused: true, position_ms: 0, rate: 1.0, at_ms: 0 }.is_from_host());
    assert!(!Message::State { seq: 1, paused: true, position_ms: 0, rate: 1.0, at_ms: 0 }.is_from_guest());
    assert!(Message::Ready { seq: 1 }.is_from_guest());
    assert!(!Message::Ready { seq: 1 }.is_from_host());
    assert!(!Message::Load(load()).is_from_guest());
    assert!(Message::Bye {}.is_from_host() && Message::Bye {}.is_from_guest());
}

#[test]
fn the_rate_limit_allows_a_burst_then_refills_over_time() {
    let start = Instant::now();
    let mut limit = RateLimit::new(start);
    let allowed = (0..30).filter(|_| limit.allow(start)).count();
    assert_eq!(allowed, RateLimit::BURST as usize);
    assert!(!limit.allow(start));
    assert!(limit.allow(start + Duration::from_millis(150)));
}

// ------------------------------------------------------------------ code and encryption

#[test]
fn codes_are_six_digits_with_optional_separators() {
    assert_eq!(normalize_code("123 456").as_deref(), Some("123456"));
    assert_eq!(normalize_code("123-456").as_deref(), Some("123456"));
    assert_eq!(normalize_code("12345"), None);
    assert_eq!(normalize_code("1234567"), None);
    assert_eq!(normalize_code("12a456"), None);
    assert_eq!(normalize_code("١٢٣٤٥٦"), None);
}

fn channels(guest_code: &str, host_code: &str) -> (anix_lib::party::secure::Channel, anix_lib::party::secure::Channel) {
    let (guest, to_host) = Pake::start(Side::Guest, guest_code);
    let (host, to_guest) = Pake::start(Side::Host, host_code);
    (guest.finish(&to_guest).unwrap(), host.finish(&to_host).unwrap())
}

#[test]
fn the_right_code_gives_both_sides_the_same_key() {
    let (mut guest, mut host) = channels("482913", "482913");
    let frame = guest.seal(b"hello");
    assert_eq!(host.open(&frame).unwrap(), b"hello");
    let reply = host.seal(b"welcome");
    assert_eq!(guest.open(&reply).unwrap(), b"welcome");
}

#[test]
fn a_wrong_code_gives_frames_that_do_not_open() {
    let (mut guest, mut host) = channels("482913", "482914");
    assert!(host.open(&guest.seal(b"hello")).is_err());
}

#[test]
fn a_bad_key_exchange_message_is_refused() {
    let (host, _) = Pake::start(Side::Host, "000000");
    assert!(host.finish(&[0u8; 10]).is_err());
}

#[test]
fn replayed_reordered_and_altered_frames_are_refused() {
    let (mut guest, mut host) = channels("111111", "111111");
    let first = guest.seal(b"one");
    let second = guest.seal(b"two");
    // Out of order.
    assert!(host.open(&second).is_err());
    assert_eq!(host.open(&first).unwrap(), b"one");
    // Replayed.
    assert!(host.open(&first).is_err());
    // Altered.
    let mut altered = second.clone();
    altered[0] ^= 1;
    assert!(host.open(&altered).is_err());
    assert_eq!(host.open(&second).unwrap(), b"two");
    // A frame the host sealed never opens as one from the guest (direction is in the nonce).
    let (mut g2, mut h2) = channels("222222", "222222");
    let own = h2.seal(b"x");
    assert!(h2.open(&own).is_err());
    assert_eq!(g2.open(&own).unwrap(), b"x");
}

// ------------------------------------------------------------------ clock maths

#[test]
fn a_clock_sample_puts_the_host_answer_half_way() {
    // Guest sent at 1000, host clock read 5050, guest got it back at 1100.
    let s = ClockSample::new(1000, 5050, 1100);
    assert_eq!(s.rtt_ms, 100);
    assert_eq!(s.offset_ms, 4000);
}

#[test]
fn the_clock_trusts_the_fastest_round_trip_among_jittery_samples() {
    let mut clock = Clock::default();
    assert_eq!(clock.offset_ms(), None);
    // True offset 4000; slow round trips skew the estimate by their one-way imbalance.
    clock.add(ClockSample::new(0, 4000 + 90, 120));
    clock.add(ClockSample::new(200, 4200 + 5 + 3, 210));
    clock.add(ClockSample::new(400, 4400 + 150, 650));
    assert_eq!(clock.rtt_ms(), Some(10));
    assert_eq!(clock.offset_ms(), Some(4003));
    // Only the last 8 are kept: the fast one ages out.
    for i in 0..8 {
        clock.add(ClockSample::new(1000 + i * 100, 5000 + i * 100 + 40, 1000 + i * 100 + 60));
    }
    assert_eq!(clock.rtt_ms(), Some(60));
    assert_eq!(clock.offset_ms(), Some(4010));
}

#[test]
fn a_host_time_becomes_an_age_in_guest_time() {
    // Host clock = guest clock + 4000. Host time 9000 is guest time 5000.
    assert_eq!(age_ms(9000, 4000, 5250), 250);
    // A start scheduled a second ahead is a negative age.
    assert_eq!(age_ms(10_000, 4000, 5000), -1000);
    // A negative offset (guest ahead) works too.
    assert_eq!(age_ms(1000, -500, 1600), 100);
}

#[test]
fn the_position_moves_with_age_only_while_playing() {
    assert_eq!(position_after(60_000, true, 1.0, 5000), 60_000);
    assert_eq!(position_after(60_000, false, 1.0, 250), 60_250);
    assert_eq!(position_after(60_000, false, 1.5, 1000), 61_500);
    assert_eq!(position_after(60_000, false, 1.0, -1000), 59_000);
    assert_eq!(position_after(500, false, 1.0, -1000), 0);
}

// ------------------------------------------------------------------ addresses

#[test]
fn a_guest_connects_only_to_private_ipv4_addresses() {
    assert_eq!(parse_host_address("192.168.1.20"), Some("192.168.1.20:47830".parse().unwrap()));
    assert_eq!(parse_host_address(" 10.0.0.5:47831 "), Some("10.0.0.5:47831".parse().unwrap()));
    assert_eq!(parse_host_address("172.16.4.4").map(|a| a.port()), Some(DEFAULT_PORT));
    for refused in ["8.8.8.8", "127.0.0.1", "0.0.0.0", "100.64.0.1", "[fe80::1]:47830", "example.local", "10.0.0.5:0", ""] {
        assert_eq!(parse_host_address(refused), None, "accepted {refused}");
    }
}

#[test]
fn vpn_and_container_interfaces_are_skipped() {
    for name in ["tun0", "wg0", "tap1", "utun3", "tailscale0", "docker0", "br-8b459da08e17", "virbr0", "veth12", "lo", "waydroid0", "anbox0", "vpn_home"] {
        assert!(is_skipped_interface(name), "{name}");
    }
    for name in ["wlo1", "wlan0", "eth0", "enp3s0", "eno1"] {
        assert!(!is_skipped_interface(name), "{name}");
    }
}

#[tokio::test]
async fn joining_refuses_a_public_or_loopback_address_and_a_malformed_code() {
    let (tx, _rx) = mpsc::unbounded_channel();
    assert!(matches!(Guest::join("8.8.8.8", "123456", tx.clone()).await, Err(PartyError::BadAddress)));
    assert!(matches!(Guest::join("127.0.0.1", "123456", tx.clone()).await, Err(PartyError::BadAddress)));
    let any = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9);
    assert!(matches!(Guest::join_allowing_loopback(any, "12345", tx).await, Err(PartyError::BadCode)));
}

// ------------------------------------------------------------------ loopback host and guests

type Rx = mpsc::UnboundedReceiver<PartyEvent>;

async fn next_matching(rx: &mut Rx, what: &str, pick: impl Fn(&PartyEvent) -> bool) -> PartyEvent {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(event)) if pick(&event) => return event,
            Ok(Some(_)) => continue,
            _ => panic!("no {what} event"),
        }
    }
}

async fn host() -> (Host, Rx, SocketAddr) {
    let (tx, rx) = mpsc::unbounded_channel();
    let host = Host::start_allowing_loopback(tx).await.unwrap();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), host.port());
    (host, rx, addr)
}

async fn join(addr: SocketAddr, code: &str) -> Result<(Guest, Rx), PartyError> {
    let (tx, rx) = mpsc::unbounded_channel();
    Guest::join_allowing_loopback(addr, code, tx).await.map(|g| (g, rx))
}

async fn wait_for_guests(host: &Host, count: usize) {
    for _ in 0..200 {
        if host.status().guests == count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("host has {} guests, expected {count}", host.status().guests);
}

#[tokio::test]
async fn a_guest_with_the_right_code_follows_load_play_pause_and_the_end() {
    let (host, mut host_rx, addr) = host().await;
    host.set_load(load()).unwrap();
    let code = host.code().to_string();
    let (guest, mut rx) = join(addr, &code).await.unwrap();
    wait_for_guests(&host, 1).await;

    // A late joiner gets the episode straight away.
    let PartyEvent::Load(got) = next_matching(&mut rx, "load", |e| matches!(e, PartyEvent::Load(_))).await else { unreachable!() };
    assert_eq!(got, load());
    assert_eq!(guest.load(), Some(load()));
    assert!(guest.offset_ms().is_some());

    // Play: prepare, the guest answers ready, then everyone starts a moment later.
    host.report(false, 61_000, 1.0).unwrap();
    assert!(host.status().waiting);
    let PartyEvent::Prepare { seq, position_ms } = next_matching(&mut rx, "prepare", |e| matches!(e, PartyEvent::Prepare { .. })).await else {
        unreachable!()
    };
    assert_eq!(position_ms, 61_000);
    guest.ready(seq);
    let PartyEvent::State(state) = next_matching(&mut rx, "state", |e| matches!(e, PartyEvent::State(_))).await else { unreachable!() };
    assert_eq!((state.seq, state.paused, state.position_ms), (seq, false, 61_000));
    // Scheduled about a second ahead; same machine, so the offset is near zero.
    assert!((state.age_ms + START_LEAD_MS as i64).abs() < 150, "age {}", state.age_ms);
    let PartyEvent::State(own) = next_matching(&mut host_rx, "host state", |e| matches!(e, PartyEvent::State(_))).await else { unreachable!() };
    assert_eq!(own.age_ms, -(START_LEAD_MS as i64));
    assert!(!host.status().waiting);

    // A player that opens now gets the same report, aged.
    assert!(matches!(guest.last_play(), Some(PartyEvent::State(s)) if s.seq == seq && !s.paused));

    // Heartbeats are rate limited, then sent.
    tokio::time::sleep(Duration::from_millis(1600)).await;
    host.heartbeat(false, 62_500, 1.0).unwrap();
    let PartyEvent::State(beat) = next_matching(&mut rx, "heartbeat", |e| matches!(e, PartyEvent::State(_))).await else { unreachable!() };
    assert_eq!((beat.seq, beat.position_ms), (seq, 62_500));
    assert!(beat.age_ms >= 0 && beat.age_ms < 150, "age {}", beat.age_ms);

    // Pause goes out at once with the exact position.
    host.report(true, 63_000, 1.0).unwrap();
    let PartyEvent::State(paused) = next_matching(&mut rx, "pause", |e| matches!(e, PartyEvent::State(s) if s.paused)).await else { unreachable!() };
    assert_eq!(paused.position_ms, 63_000);

    // The host leaves: the guest is told.
    host.stop();
    let ended = next_matching(&mut rx, "end", |e| matches!(e, PartyEvent::Ended { .. })).await;
    assert_eq!(ended, PartyEvent::Ended { reason: EndReason::HostLeft });
}

#[tokio::test]
async fn a_wrong_code_fails_and_five_wrong_codes_lock_the_party() {
    let (host, _host_rx, addr) = host().await;
    let right = host.code().to_string();
    let wrong = if right == "000000" { "000001" } else { "000000" };
    for attempt in 1..=MAX_FAILED_JOINS {
        let result = join(addr, wrong).await;
        assert!(matches!(result, Err(PartyError::WrongCode)), "attempt {attempt}");
    }
    assert!(host.status().locked);
    assert!(matches!(join(addr, &right).await, Err(PartyError::Locked)));
    assert_eq!(host.status().guests, 0);
}

#[tokio::test]
async fn the_ninth_guest_is_refused() {
    let (host, _host_rx, addr) = host().await;
    let code = host.code().to_string();
    let mut guests = Vec::new();
    for _ in 0..MAX_GUESTS {
        guests.push(join(addr, &code).await.unwrap());
    }
    wait_for_guests(&host, MAX_GUESTS).await;
    assert!(matches!(join(addr, &code).await, Err(PartyError::Full)));
    // A guest leaving makes room again.
    let (first, _) = guests.remove(0);
    first.leave();
    wait_for_guests(&host, MAX_GUESTS - 1).await;
    assert!(join(addr, &code).await.is_ok());
}

#[tokio::test]
async fn the_host_waits_only_for_guests_still_connected() {
    let (host, mut host_rx, addr) = host().await;
    let code = host.code().to_string();
    let (slow, _slow_rx) = join(addr, &code).await.unwrap();
    let (fast, mut fast_rx) = join(addr, &code).await.unwrap();
    wait_for_guests(&host, 2).await;
    host.report(false, 0, 1.0).unwrap();
    let PartyEvent::Prepare { seq, .. } = next_matching(&mut fast_rx, "prepare", |e| matches!(e, PartyEvent::Prepare { .. })).await else {
        unreachable!()
    };
    fast.ready(seq);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(host.status().waiting, "started without the slow guest");
    // The slow guest leaves instead of answering: play starts.
    slow.leave();
    next_matching(&mut host_rx, "host state", |e| matches!(e, PartyEvent::State(_))).await;
    next_matching(&mut fast_rx, "state", |e| matches!(e, PartyEvent::State(_))).await;
}

/// Frames as the wire carries them, for a hand-made peer.
async fn send_raw(stream: &mut TcpStream, bytes: &[u8]) {
    let mut buf = (bytes.len() as u16).to_be_bytes().to_vec();
    buf.extend_from_slice(bytes);
    stream.write_all(&buf).await.unwrap();
}

async fn read_raw(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut len = [0u8; 2];
    stream.read_exact(&mut len).await.ok()?;
    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
    stream.read_exact(&mut buf).await.ok()?;
    Some(buf)
}

/// Joins by hand and returns the stream and channel after the Welcome.
async fn raw_join(addr: SocketAddr, code: &str) -> (TcpStream, anix_lib::party::secure::Channel) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let (pake, outbound) = Pake::start(Side::Guest, code);
    send_raw(&mut stream, &outbound).await;
    let reply = read_raw(&mut stream).await.unwrap();
    assert_eq!(reply[0], 0);
    let mut channel = pake.finish(&reply[1..]).unwrap();
    send_raw(&mut stream, &channel.seal(&protocol::encode(&Message::Hello { proto: 1, t1: 0 }))).await;
    let welcome = channel.open(&read_raw(&mut stream).await.unwrap()).unwrap();
    assert!(matches!(protocol::decode(&welcome), Ok(Message::Welcome { .. })));
    (stream, channel)
}

async fn closed_by_peer(stream: &mut TcpStream) -> bool {
    let read = tokio::time::timeout(Duration::from_secs(3), async {
        let mut buf = [0u8; 256];
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return true,
                Ok(_) => continue,
            }
        }
    });
    read.await.unwrap_or(false)
}

#[tokio::test]
async fn a_malformed_message_from_a_guest_closes_its_connection() {
    let (host, _host_rx, addr) = host().await;
    let code = host.code().to_string();
    for bad in [&br#"{"kind":"chat","text":"hi"}"#[..], br#"{"kind":"state","seq":1,"paused":true,"position_ms":0,"rate":1,"at_ms":0}"#] {
        let (mut stream, mut channel) = raw_join(addr, &code).await;
        wait_for_guests(&host, 1).await;
        send_raw(&mut stream, &channel.seal(bad)).await;
        assert!(closed_by_peer(&mut stream).await);
        wait_for_guests(&host, 0).await;
    }
}

#[tokio::test]
async fn a_replayed_frame_closes_the_connection() {
    let (host, _host_rx, addr) = host().await;
    let (mut stream, mut channel) = raw_join(addr, host.code()).await;
    wait_for_guests(&host, 1).await;
    let ping = channel.seal(&protocol::encode(&Message::Ping { t1: 1 }));
    send_raw(&mut stream, &ping).await;
    send_raw(&mut stream, &ping).await;
    assert!(closed_by_peer(&mut stream).await);
    wait_for_guests(&host, 0).await;
}

#[tokio::test]
async fn an_oversize_frame_closes_the_connection() {
    let (host, _host_rx, addr) = host().await;
    let (mut stream, _channel) = raw_join(addr, host.code()).await;
    wait_for_guests(&host, 1).await;
    // Only the length: 60 000 bytes announced.
    stream.write_all(&60_000u16.to_be_bytes()).await.unwrap();
    assert!(closed_by_peer(&mut stream).await);
    wait_for_guests(&host, 0).await;
}

#[tokio::test]
async fn a_guest_flooding_messages_is_disconnected() {
    let (host, _host_rx, addr) = host().await;
    let (mut stream, mut channel) = raw_join(addr, host.code()).await;
    wait_for_guests(&host, 1).await;
    for seq in 0..40 {
        let frame = channel.seal(&protocol::encode(&Message::Ready { seq }));
        let mut buf = (frame.len() as u16).to_be_bytes().to_vec();
        buf.extend_from_slice(&frame);
        if stream.write_all(&buf).await.is_err() {
            break;
        }
    }
    assert!(closed_by_peer(&mut stream).await);
    wait_for_guests(&host, 0).await;
}

#[tokio::test]
async fn a_guest_refuses_a_host_that_sends_a_guest_message() {
    // A fake host: does the exchange, welcomes, then sends a Ready (a guest-only kind).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let fake = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let inbound = read_raw(&mut stream).await.unwrap();
        let (pake, outbound) = Pake::start(Side::Host, "424242");
        let mut reply = vec![0u8];
        reply.extend_from_slice(&outbound);
        send_raw(&mut stream, &reply).await;
        let mut channel = pake.finish(&inbound).unwrap();
        let hello = protocol::decode(&channel.open(&read_raw(&mut stream).await.unwrap()).unwrap()).unwrap();
        let Message::Hello { t1, .. } = hello else { panic!("no hello") };
        send_raw(&mut stream, &channel.seal(&protocol::encode(&Message::Welcome { t1, t2: 0 }))).await;
        send_raw(&mut stream, &channel.seal(&protocol::encode(&Message::Ready { seq: 1 }))).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    let (_guest, mut rx) = join(addr, "424242").await.unwrap();
    let ended = next_matching(&mut rx, "end", |e| matches!(e, PartyEvent::Ended { .. })).await;
    assert_eq!(ended, PartyEvent::Ended { reason: EndReason::Lost });
    fake.abort();
}

#[tokio::test]
async fn silent_connections_do_not_hold_up_a_join() {
    let (host, _host_rx, addr) = host().await;
    // Two peers that connect and say nothing: before, each held the join lock for 5 s.
    let _idle = [TcpStream::connect(addr).await.unwrap(), TcpStream::connect(addr).await.unwrap()];
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = Instant::now();
    assert!(join(addr, host.code()).await.is_ok());
    assert!(started.elapsed() < Duration::from_secs(2), "join took {:?}", started.elapsed());
    assert!(!host.status().locked);
}

#[tokio::test]
async fn one_address_may_hold_only_a_few_connections_in_the_join_exchange() {
    let (host, _host_rx, addr) = host().await;
    let mut idle = Vec::new();
    for _ in 0..4 {
        idle.push(TcpStream::connect(addr).await.unwrap());
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    // The next one is closed at once instead of waiting out the handshake timeout.
    let mut extra = TcpStream::connect(addr).await.unwrap();
    let started = Instant::now();
    assert!(closed_by_peer(&mut extra).await);
    assert!(started.elapsed() < Duration::from_secs(2));
    // Once they go, joining works again.
    drop(idle);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(join(addr, host.code()).await.is_ok());
}

#[tokio::test]
async fn a_run_of_quick_reports_is_merged_and_keeps_guests_connected() {
    let (host, _host_rx, addr) = host().await;
    let (_guest, mut rx) = join(addr, host.code()).await.unwrap();
    wait_for_guests(&host, 1).await;
    // A held arrow key while paused: far more than the guests' 10 a second.
    for step in 0..100u64 {
        host.report(true, step * 100, 1.0).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let mut states = 0;
    let last = loop {
        match tokio::time::timeout(Duration::from_millis(800), rx.recv()).await {
            Ok(Some(PartyEvent::Ended { reason })) => panic!("guest dropped: {reason:?}"),
            Ok(Some(PartyEvent::State(state))) => {
                states += 1;
                if state.position_ms == 9_900 {
                    break state;
                }
            }
            Ok(Some(_)) => continue,
            _ => panic!("the last report never arrived"),
        }
    };
    assert!(last.paused);
    assert!(states < 10, "{states} states sent");
    assert_eq!(host.status().guests, 1);
}

#[tokio::test]
async fn a_party_hosted_mid_episode_sends_where_the_host_is() {
    let (host, _host_rx, addr) = host().await;
    host.set_load(load()).unwrap();
    let (guest, mut rx) = join(addr, host.code()).await.unwrap();
    wait_for_guests(&host, 1).await;
    // Nothing reported yet: the first heartbeat is the state, playing or paused.
    host.heartbeat(false, 30_000, 1.0).unwrap();
    let PartyEvent::State(state) = next_matching(&mut rx, "state", |e| matches!(e, PartyEvent::State(_))).await else { unreachable!() };
    assert_eq!((state.paused, state.position_ms), (false, 30_000));
    assert!(matches!(guest.last_play(), Some(PartyEvent::State(s)) if !s.paused));
    // A new episode opened paused.
    host.set_load(Load { episode: 4.0, ..load() }).unwrap();
    host.heartbeat(true, 0, 1.0).unwrap();
    let PartyEvent::State(state) = next_matching(&mut rx, "paused state", |e| matches!(e, PartyEvent::State(s) if s.paused)).await else { unreachable!() };
    assert_eq!(state.position_ms, 0);
    // Once there is a state, a paused heartbeat changes nothing.
    host.heartbeat(true, 5_000, 1.0).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(matches!(guest.last_play(), Some(PartyEvent::State(s)) if s.position_ms == 0));
}

#[tokio::test]
async fn a_load_that_only_adds_the_length_keeps_the_guests_last_report() {
    let (host, _host_rx, addr) = host().await;
    host.set_load(Load { duration_ms: None, ..load() }).unwrap();
    let (guest, mut rx) = join(addr, host.code()).await.unwrap();
    wait_for_guests(&host, 1).await;
    host.report(true, 12_000, 1.0).unwrap();
    next_matching(&mut rx, "state", |e| matches!(e, PartyEvent::State(_))).await;
    host.set_load(load()).unwrap();
    next_matching(&mut rx, "load with length", |e| matches!(e, PartyEvent::Load(l) if l.duration_ms.is_some())).await;
    assert!(matches!(guest.last_play(), Some(PartyEvent::State(s)) if s.position_ms == 12_000));
    // Another episode does drop it.
    host.set_load(Load { episode: 4.0, ..load() }).unwrap();
    next_matching(&mut rx, "new episode", |e| matches!(e, PartyEvent::Load(l) if l.episode == 4.0)).await;
    assert!(guest.last_play().is_none());
}
