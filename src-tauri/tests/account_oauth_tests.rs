//! Authorize URLs, PKCE values and redirect parsing.

use anix_lib::accounts;

use accounts::oauth::{
    anilist_authorize_url, denial_message, is_registered_redirect, mal_authorize_url,
    new_pkce_verifier, new_state, parse_redirect, Redirect, Service,
};
use std::collections::{HashMap, HashSet};
use url::Url;

fn url(text: &str) -> Url {
    Url::parse(text).expect("valid url")
}

fn query(text: &str) -> HashMap<String, String> {
    url(text).query_pairs().into_owned().collect()
}

#[test]
fn test_account_pkce_verifier_follows_rfc_7636() {
    let mut seen = HashSet::new();
    for _ in 0..50 {
        let verifier = new_pkce_verifier().expect("random bytes");
        assert!((43..=128).contains(&verifier.len()), "length {}", verifier.len());
        assert!(
            verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')),
            "unreserved characters only: {}",
            verifier
        );
        assert!(seen.insert(verifier), "verifiers must not repeat");
    }
}

#[test]
fn test_account_state_is_random_and_url_safe() {
    let first = new_state().unwrap();
    let second = new_state().unwrap();
    assert_ne!(first, second);
    assert!(first.len() >= 32);
    assert!(first.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
}

#[test]
fn test_account_anilist_authorize_url() {
    let authorize = anilist_authorize_url("st4te");
    assert!(authorize.starts_with("https://anilist.co/api/v2/oauth/authorize?"));
    let params = query(&authorize);
    assert_eq!(params.get("client_id").map(String::as_str), Some("45552"));
    assert_eq!(params.get("response_type").map(String::as_str), Some("token"));
    assert_eq!(params.get("state").map(String::as_str), Some("st4te"));
    // AniList's grant step fails when the registered redirect is passed explicitly.
    assert!(!params.contains_key("redirect_uri"));
}

#[test]
fn test_account_mal_authorize_url() {
    let authorize = mal_authorize_url("st4te", "the-verifier");
    assert!(authorize.starts_with("https://myanimelist.net/v1/oauth2/authorize?"));
    let params = query(&authorize);
    assert_eq!(params.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(
        params.get("client_id").map(String::as_str),
        Some("4ae5f8056db821737a4f40f8816177cd")
    );
    assert_eq!(params.get("state").map(String::as_str), Some("st4te"));
    assert_eq!(params.get("code_challenge").map(String::as_str), Some("the-verifier"));
    assert_eq!(params.get("code_challenge_method").map(String::as_str), Some("plain"));
    assert!(!params.contains_key("redirect_uri"));
}

#[test]
fn test_account_anilist_token_is_read_from_the_fragment() {
    let redirect = parse_redirect(
        Service::AniList,
        &url("http://localhost/#access_token=eyJ.abc.def&token_type=Bearer&expires_in=31536000&state=s1"),
    );
    assert_eq!(
        redirect,
        Redirect::Token {
            access_token: "eyJ.abc.def".to_string(),
            expires_in: Some(31_536_000),
            state: Some("s1".to_string()),
        }
    );

    // Without a trailing slash, over https, and percent-encoded.
    for address in [
        "http://localhost#access_token=tok%2B1&state=s1",
        "https://localhost/#access_token=tok%2B1&state=s1",
        "http://LOCALHOST:80/#access_token=tok%2B1&state=s1",
    ] {
        match parse_redirect(Service::AniList, &url(address)) {
            Redirect::Token { access_token, expires_in, state } => {
                assert_eq!(access_token, "tok+1", "{}", address);
                assert_eq!(expires_in, None);
                assert_eq!(state.as_deref(), Some("s1"));
            }
            other => panic!("{} parsed as {:?}", address, other),
        }
    }

    // A token in the query is not the implicit grant's answer.
    assert_eq!(
        parse_redirect(Service::AniList, &url("http://localhost/?access_token=tok")),
        Redirect::Empty
    );
    assert_eq!(
        parse_redirect(Service::AniList, &url("http://localhost/#access_token=&state=s1")),
        Redirect::Empty
    );
}

#[test]
fn test_account_mal_code_is_read_from_the_query() {
    assert_eq!(
        parse_redirect(Service::Mal, &url("https://localhost/?code=def50200abc&state=s2")),
        Redirect::Code {
            code: "def50200abc".to_string(),
            state: Some("s2".to_string()),
        }
    );
    assert_eq!(
        parse_redirect(Service::Mal, &url("http://localhost?code=c%2F1")),
        Redirect::Code {
            code: "c/1".to_string(),
            state: None,
        }
    );
    // A code in the fragment, or an empty one, is not an answer.
    assert_eq!(parse_redirect(Service::Mal, &url("https://localhost/#code=abc")), Redirect::Empty);
    assert_eq!(parse_redirect(Service::Mal, &url("https://localhost/?code=&state=s2")), Redirect::Empty);
    assert_eq!(parse_redirect(Service::Mal, &url("https://localhost/")), Redirect::Empty);
}

#[test]
fn test_account_error_redirects() {
    assert_eq!(
        parse_redirect(
            Service::Mal,
            &url("https://localhost/?error=access_denied&message=The+resource+owner+denied+the+request.&state=s2"),
        ),
        Redirect::Denied {
            error: "access_denied".to_string(),
            description: Some("The resource owner denied the request.".to_string()),
        }
    );
    assert_eq!(
        parse_redirect(Service::AniList, &url("http://localhost/#error=access_denied&state=s1")),
        Redirect::Denied {
            error: "access_denied".to_string(),
            description: None,
        }
    );
    assert_eq!(
        parse_redirect(
            Service::AniList,
            &url("http://localhost/?error=invalid_request&error_description=Bad+client"),
        ),
        Redirect::Denied {
            error: "invalid_request".to_string(),
            description: Some("Bad client".to_string()),
        }
    );

    assert_eq!(
        denial_message(Service::AniList, "access_denied", None),
        "AniList sign-in was not approved."
    );
    assert_eq!(
        denial_message(Service::Mal, "invalid_request", Some("Bad client")),
        "MyAnimeList sign-in failed: Bad client (invalid_request)"
    );
}

#[test]
fn test_account_look_alike_hosts_are_not_the_redirect() {
    let look_alikes = [
        "http://localhost.evil.example/#access_token=tok&state=s1",
        "http://evil.example/localhost/#access_token=tok&state=s1",
        "http://localhost@evil.example/#access_token=tok&state=s1",
        "http://user@localhost/#access_token=tok&state=s1",
        "https://localhost:8443/?code=abc&state=s2",
        "http://localhost:1420/#access_token=tok&state=s1",
        "http://127.0.0.1/#access_token=tok&state=s1",
        "http://localhost./#access_token=tok&state=s1",
        "https://anilist.co/api/v2/oauth/authorize?redirect=http://localhost/#access_token=tok",
        "https://myanimelist.net/v1/oauth2/authorize?code=abc&state=s2",
        "tauri://localhost/#access_token=tok&state=s1",
        "ftp://localhost/?code=abc",
    ];
    for address in look_alikes {
        let parsed = url(address);
        assert!(!is_registered_redirect(&parsed), "{}", address);
        for service in Service::ALL {
            assert_eq!(parse_redirect(service, &parsed), Redirect::Elsewhere, "{}", address);
        }
    }

    for address in ["http://localhost", "http://localhost/", "https://localhost/?x=1", "https://localhost:443/"] {
        assert!(is_registered_redirect(&url(address)), "{}", address);
    }
}

#[test]
fn test_account_service_names() {
    assert_eq!(Service::from_key("anilist"), Some(Service::AniList));
    assert_eq!(Service::from_key("mal"), Some(Service::Mal));
    assert_eq!(Service::from_key("kitsu"), None);
    assert_eq!(serde_json::to_string(&Service::Mal).unwrap(), "\"mal\"");
    assert_eq!(serde_json::from_str::<Service>("\"anilist\"").unwrap(), Service::AniList);
    assert_eq!(Service::Mal.label(), "MyAnimeList");
}
