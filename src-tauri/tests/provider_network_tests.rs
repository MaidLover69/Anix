//! The network the extensions see: automatic DNS over HTTPS and repeated requests after a
//! connection reset. Loopback only.

use anix_lib::clients::app_settings::{DohMode, NetworkSettings};
use anix_lib::clients::http::{doh_first, looks_blocked_answer, use_doh_answer};
use anix_lib::plugins::net::{is_connection_reset, NetContext, WireRequest};
use anix_lib::web_session::{app_session, SessionUse};
use serde_json::json;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn the_doh_mode_keeps_an_older_switch_and_is_automatic_for_new_settings() {
    assert_eq!(DohMode::from_json(&json!({})), DohMode::Auto);
    assert_eq!(DohMode::from_json(&json!({ "enable_doh": true })), DohMode::On);
    assert_eq!(DohMode::from_json(&json!({ "enable_doh": false })), DohMode::Off);
    assert_eq!(DohMode::from_json(&json!({ "enable_doh": true, "doh_mode": "auto" })), DohMode::Auto);
    assert_eq!(DohMode::from_json(&json!({ "enable_doh": false, "doh_mode": "on" })), DohMode::On);
    assert_eq!(DohMode::from_json(&json!({ "doh_mode": "nonsense", "enable_doh": false })), DohMode::Off);
    assert_eq!(NetworkSettings::default().doh_mode, DohMode::Auto);
}

#[test]
fn a_blocked_looking_answer_with_no_doh_records_keeps_working() {
    // Automatic mode: DoH knowing no address sends the lookup back to the system resolver.
    assert!(!use_doh_answer(DohMode::Auto, false));
    assert!(use_doh_answer(DohMode::Auto, true));
    // Always-on DoH keeps its "no such host".
    assert!(use_doh_answer(DohMode::On, false));
}

#[test]
fn automatic_mode_uses_doh_only_for_hosts_that_looked_blocked() {
    assert!(doh_first(DohMode::On, false));
    assert!(!doh_first(DohMode::Off, true));
    assert!(!doh_first(DohMode::Auto, false));
    assert!(doh_first(DohMode::Auto, true));

    let ips = |list: &[&str]| -> Vec<IpAddr> { list.iter().map(|ip| ip.parse().unwrap()).collect() };
    assert!(looks_blocked_answer(&[]));
    assert!(looks_blocked_answer(&ips(&["0.0.0.0"])));
    assert!(looks_blocked_answer(&ips(&["127.0.0.1", "::1"])));
    assert!(looks_blocked_answer(&ips(&["10.10.34.35"])));
    assert!(!looks_blocked_answer(&ips(&["104.21.3.4"])));
    assert!(!looks_blocked_answer(&ips(&["0.0.0.0", "104.21.3.4"])));
}

#[test]
fn resets_are_told_apart_from_other_failures() {
    assert!(is_connection_reset("error sending request: Connection reset by peer (os error 104)"));
    assert!(is_connection_reset("connection closed before message completed"));
    assert!(!is_connection_reset("operation timed out"));
    assert!(!is_connection_reset("Connection refused (os error 111)"));
}

/// Answers the first `drop_first` connections by closing them unanswered, then with 200.
async fn flaky_server(drop_first: usize) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let number = counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                let _ = socket.read(&mut buffer).await;
                if number < drop_first {
                    return;
                }
                let body = "{\"ok\":true}";
                let answer = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(answer.as_bytes()).await;
            });
        }
    });
    (base, seen)
}

fn get(url: &str) -> WireRequest {
    serde_json::from_value(json!({ "url": url, "method": "GET" })).unwrap()
}

#[tokio::test]
async fn a_connection_the_network_cuts_off_is_tried_again() {
    let (base, seen) = flaky_server(2).await;
    let net = NetContext::new("flaky", app_session(), true);
    let answer = net.perform(get(&format!("{}/search", base)), vec![], SessionUse::Existing).await.unwrap();
    assert_eq!(answer.meta.status, 200);
    assert_eq!(seen.load(Ordering::SeqCst), 3, "two cut-off connections, then the answer");

    // Three retries at most: a site that never answers fails with the reset.
    let (base, seen) = flaky_server(usize::MAX).await;
    let net = NetContext::new("flaky", app_session(), true);
    let error = net.perform(get(&format!("{}/search", base)), vec![], SessionUse::Existing).await.unwrap_err();
    assert!(is_connection_reset(&error), "{}", error);
    assert_eq!(seen.load(Ordering::SeqCst), 4);
}
