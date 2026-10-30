//! Which addresses content from extensions may reach: public internet hosts only.
//!
//! Extensions are code the user adds from a link. The URLs they hand the app (their own
//! `fetch`, the stream and subtitle links they return, the playlists behind those) must not
//! reach the user's machine or home network. Every such request goes through the clients here:
//! the URL's literal host is checked before sending, host names are resolved through
//! `PublicResolver`, which never hands out a private address (so the connection is made only
//! to an address that was checked, and DNS rebinding cannot swap it), and every redirect hop is
//! checked again.

use crate::clients::http::{base_builder, build_error, DohResolver};
use crate::error::AppError;
use once_cell::sync::OnceCell;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use reqwest::{Client, ClientBuilder};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// Redirect hops a guarded client follows, as many as the shared clients.
const MAX_REDIRECTS: usize = 10;

/// True for an address that is not on the public internet: loopback, private, link-local
/// (including the cloud metadata address 169.254.169.254), unspecified, multicast, broadcast,
/// reserved, and IPv6 forms that carry such an IPv4 address (`::ffff:127.0.0.1`, NAT64).
///
/// 100.64.0.0/10 (carrier-grade NAT) is refused as well: no public site is served from it, and
/// VPNs such as Tailscale put the user's own devices there.
pub fn is_private_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            // 0.0.0.0/8 is "this network"; Linux connects 0.0.0.0 to the machine itself.
            a == 0
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || (a == 100 && (b & 0xc0) == 64)
                // 240.0.0.0/4 is reserved, and holds the broadcast address.
                || a >= 240
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_address(IpAddr::V4(v4));
            }
            let s = v6.segments();
            // NAT64 gateways connect to the IPv4 address in the last 32 bits.
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                let v4 = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
                return is_private_address(IpAddr::V4(v4));
            }
            // ::/96 holds ::, ::1 and the old IPv4-compatible form; nothing public lives there.
            s[..6] == [0; 6]
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (s[0] & 0xffc0) == 0xfec0 // old site-local fec0::/10
        }
    }
}

/// True when the URL's host, as written, is the local machine or a private address. A host
/// name is resolved later, by `PublicResolver`.
pub fn private_target(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(ip)) => is_private_address(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_private_address(IpAddr::V6(ip)),
        None => true,
    }
}

/// Ok for an `http(s)` URL with a host that is not written as a private address. Checked before
/// every request a guarded client sends, since a connection to an IP literal never asks the
/// resolver.
pub fn check_public_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("only http and https URLs can be fetched, not {}:", url.scheme()));
    }
    if url.host_str().map_or(true, str::is_empty) {
        return Err("the URL has no host".to_string());
    }
    if private_target(url) {
        return Err(format!("private network access denied: {}", url.host_str().unwrap_or_default()));
    }
    Ok(())
}

/// The app's resolver (DoH settings and all) with every private address taken out of the
/// answer. A name with only private addresses fails before any connection is opened.
#[derive(Debug, Default, Clone, Copy)]
pub struct PublicResolver;

/// Host names of the proxies the user set in the environment (`HTTP_PROXY`, `HTTPS_PROXY`,
/// `ALL_PROXY`, either case), lowercased. reqwest resolves a proxy's name through the client's
/// resolver, so a local proxy such as `http://localhost:7890` must be let through; the names
/// come from the user's own environment, never from an extension.
pub fn proxy_hosts<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| {
            let value = value.trim();
            // `clash:7890` parses as a scheme with no host, so a value without one gets `http://`.
            let url = Url::parse(value)
                .ok()
                .filter(|url| url.host().is_some())
                .or_else(|| Url::parse(&format!("http://{value}")).ok())?;
            match url.host()? {
                url::Host::Domain(name) => Some(name.trim_end_matches('.').to_ascii_lowercase()),
                _ => None,
            }
        })
        .collect()
}

fn env_proxy_hosts() -> &'static [String] {
    static HOSTS: OnceCell<Vec<String>> = OnceCell::new();
    HOSTS.get_or_init(|| {
        let values: Vec<String> = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"]
            .iter()
            .filter_map(|key| std::env::var(key).ok())
            .collect();
        proxy_hosts(values.iter().map(String::as_str))
    })
}

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let is_proxy = env_proxy_hosts().contains(&host.trim_end_matches('.').to_ascii_lowercase());
        let lookup = DohResolver.resolve(name);
        Box::pin(async move {
            if is_proxy {
                // The user's own proxy: may well be on this machine or the LAN.
                return lookup.await;
            }
            let public: Vec<SocketAddr> = lookup.await?.filter(|addr| !is_private_address(addr.ip())).collect();
            if public.is_empty() {
                return Err(format!("private network access denied: {} resolves to a private address", host).into());
            }
            let addrs: Addrs = Box::new(public.into_iter());
            Ok(addrs)
        })
    }
}

/// Follows a redirect only to an `http(s)` URL whose host is not a private address; a host name
/// there is checked by `PublicResolver` when the hop connects.
fn public_redirects() -> Policy {
    Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        match check_public_url(attempt.url()) {
            Ok(()) => attempt.follow(),
            Err(reason) => attempt.error(reason),
        }
    })
}

fn public_builder() -> ClientBuilder {
    base_builder().dns_resolver(Arc::new(PublicResolver))
}

static PUBLIC_CLIENT: OnceCell<Client> = OnceCell::new();
static PUBLIC_DOWNLOAD_CLIENT: OnceCell<Client> = OnceCell::new();
static PUBLIC_NO_REDIRECT_CLIENT: OnceCell<Client> = OnceCell::new();

/// `http_client` that reaches public hosts only. Callers still check the first URL with
/// `check_public_url`.
pub fn public_http_client() -> Result<Client, AppError> {
    PUBLIC_CLIENT
        .get_or_try_init(|| {
            public_builder()
                .timeout(Duration::from_secs(30))
                .redirect(public_redirects())
                .build()
                .map_err(build_error)
        })
        .cloned()
}

/// `try_download_client` that reaches public hosts only.
pub fn public_download_client() -> Result<Client, AppError> {
    PUBLIC_DOWNLOAD_CLIENT
        .get_or_try_init(|| {
            public_builder()
                .read_timeout(Duration::from_secs(30))
                .redirect(public_redirects())
                .build()
                .map_err(build_error)
        })
        .cloned()
}

/// `no_redirect_client` that reaches public hosts only. Callers that follow redirects by hand
/// check each hop with `check_public_url`.
pub fn public_no_redirect_client() -> Result<Client, AppError> {
    PUBLIC_NO_REDIRECT_CLIENT
        .get_or_try_init(|| {
            public_builder()
                .timeout(Duration::from_secs(30))
                .redirect(Policy::none())
                .build()
                .map_err(build_error)
        })
        .cloned()
}
