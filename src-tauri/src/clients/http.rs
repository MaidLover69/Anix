//! Shared HTTP clients. Every outgoing request in the backend goes through one of these so the
//! DNS-over-HTTPS settings apply everywhere.

use crate::clients::app_settings::{self, DohMode, DohProvider};
use crate::error::AppError;
use moka::future::Cache;
use moka::Expiry;
use once_cell::sync::{Lazy, OnceCell};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, ClientBuilder};
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How the app names itself to the services it talks to as itself (not as a browser).
pub const APP_USER_AGENT: &str = concat!("Anix/", env!("CARGO_PKG_VERSION"));

pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:146.0) Gecko/20100101 Firefox/146.0";

const DOH_REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const MIN_DNS_TTL: Duration = Duration::from_secs(60);
const MAX_DNS_TTL: Duration = Duration::from_secs(3600);
const MISSING_NAME_TTL: Duration = Duration::from_secs(60);

const DNS_TYPE_A: u64 = 1;
const DNS_TYPE_AAAA: u64 = 28;

impl DohProvider {
    fn host(self) -> &'static str {
        match self {
            DohProvider::Cloudflare => "cloudflare-dns.com",
            DohProvider::Google => "dns.google",
            DohProvider::AdGuard => "dns.adguard-dns.com",
        }
    }

    fn query_url(self, name: &str, record_type: &str) -> String {
        let path = match self {
            DohProvider::Cloudflare => "dns-query",
            DohProvider::Google | DohProvider::AdGuard => "resolve",
        };
        format!(
            "https://{}/{}?name={}&type={}",
            self.host(),
            path,
            urlencoding::encode(name),
            record_type
        )
    }

    /// Well-known addresses of the resolver, so reaching it never depends on the system DNS.
    fn bootstrap_addrs(self) -> Vec<SocketAddr> {
        let ips: [IpAddr; 4] = match self {
            DohProvider::Cloudflare => [
                IpAddr::V4(Ipv4Addr::new(104, 16, 248, 249)),
                IpAddr::V4(Ipv4Addr::new(104, 16, 249, 249)),
                IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0x6810, 0xf8f9)),
                IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0x6810, 0xf9f9)),
            ],
            DohProvider::Google => [
                IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
                IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4)),
                IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888)),
                IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8844)),
            ],
            DohProvider::AdGuard => [
                IpAddr::V4(Ipv4Addr::new(94, 140, 14, 14)),
                IpAddr::V4(Ipv4Addr::new(94, 140, 15, 15)),
                IpAddr::V6(Ipv6Addr::new(0x2a10, 0x50c0, 0, 0, 0, 0, 0xad1, 0xff)),
                IpAddr::V6(Ipv6Addr::new(0x2a10, 0x50c0, 0, 0, 0, 0, 0xad2, 0xff)),
            ],
        };
        ips.iter().map(|ip| SocketAddr::new(*ip, 443)).collect()
    }
}

/// Addresses and the smallest TTL (seconds) found in one DoH JSON answer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DohRecords {
    pub addrs: Vec<IpAddr>,
    pub ttl_s: Option<u64>,
}

/// Parses the JSON form of a DoH answer (`application/dns-json`, as served by Cloudflare, Google
/// and AdGuard). Only A and AAAA records are kept; CNAME hops in the answer are skipped.
/// A response whose `Status` is not NOERROR/NXDOMAIN is an error; NXDOMAIN is an empty answer.
pub fn parse_doh_answer(json: &Value) -> Result<DohRecords, String> {
    let status = json.get("Status").and_then(Value::as_u64).unwrap_or(0);
    if status == 3 {
        return Ok(DohRecords::default());
    }
    if status != 0 {
        return Err(format!("resolver answered with DNS status {}", status));
    }

    let mut records = DohRecords::default();
    let Some(answers) = json.get("Answer").and_then(Value::as_array) else {
        return Ok(records);
    };
    for answer in answers {
        let record_type = answer.get("type").and_then(Value::as_u64).unwrap_or(0);
        if record_type != DNS_TYPE_A && record_type != DNS_TYPE_AAAA {
            continue;
        }
        let Some(ip) = answer
            .get("data")
            .and_then(Value::as_str)
            .and_then(|d| d.trim().parse::<IpAddr>().ok())
        else {
            continue;
        };
        if !records.addrs.contains(&ip) {
            records.addrs.push(ip);
        }
        if let Some(ttl) = answer.get("TTL").and_then(Value::as_u64) {
            records.ttl_s = Some(records.ttl_s.map_or(ttl, |current| current.min(ttl)));
        }
    }
    Ok(records)
}

#[derive(Clone)]
struct CachedAnswer {
    addrs: Arc<Vec<IpAddr>>,
    ttl: Duration,
}

struct AnswerExpiry;

impl Expiry<String, CachedAnswer> for AnswerExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &CachedAnswer,
        _created_at: Instant,
    ) -> Option<Duration> {
        Some(value.ttl)
    }
}

static DOH_CACHE: Lazy<Cache<String, CachedAnswer>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(2048)
        .expire_after(AnswerExpiry)
        .build()
});

/// Names the resolver recently reported as having no address, so a dead domain is not looked up
/// again on every request.
static DOH_MISSING: Lazy<Cache<String, ()>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(512)
        .time_to_live(MISSING_NAME_TTL)
        .build()
});

/// Client used only to talk to the DoH resolvers; their addresses are pinned.
static BOOTSTRAP_CLIENT: OnceCell<Client> = OnceCell::new();

fn bootstrap_client() -> Result<&'static Client, String> {
    BOOTSTRAP_CLIENT.get_or_try_init(|| {
        let mut builder = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(DOH_REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(3));
        for provider in [DohProvider::Cloudflare, DohProvider::Google, DohProvider::AdGuard] {
            builder = builder.resolve_to_addrs(provider.host(), &provider.bootstrap_addrs());
        }
        builder.build().map_err(|e| e.to_string())
    })
}

async fn doh_query(
    client: &Client,
    provider: DohProvider,
    host: &str,
    record_type: &str,
) -> Result<DohRecords, String> {
    let response = client
        .get(provider.query_url(host, record_type))
        .header("Accept", "application/dns-json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("resolver answered HTTP {}", response.status().as_u16()));
    }
    let json: Value = response.json().await.map_err(|e| e.to_string())?;
    parse_doh_answer(&json)
}

#[derive(Debug, Clone)]
enum DohFailure {
    /// The resolver answered, but has no address for this name.
    NoRecords,
    /// The resolver itself could not be used.
    Resolver(String),
}

/// While set and in the future, the DoH resolver is treated as down and lookups go straight to
/// the system resolver instead of waiting for another timeout per host name.
static DOH_DOWN_UNTIL: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));
const DOH_DOWN_FOR: Duration = Duration::from_secs(30);

fn doh_is_down() -> bool {
    let guard = DOH_DOWN_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
    matches!(*guard, Some(until) if Instant::now() < until)
}

fn mark_doh_down() {
    let mut guard = DOH_DOWN_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(Instant::now() + DOH_DOWN_FOR);
}

async fn doh_lookup(host: &str, provider: DohProvider) -> Result<CachedAnswer, DohFailure> {
    let client = bootstrap_client().map_err(DohFailure::Resolver)?;
    let (v4, v6) = tokio::join!(
        doh_query(client, provider, host, "A"),
        doh_query(client, provider, host, "AAAA")
    );
    // "No address" is only believed when both answers arrived; one failed query leaves it open.
    let failed_query = v4.as_ref().err().or(v6.as_ref().err()).cloned();

    // IPv6 first, as the system resolver orders them on a dual-stack host. A host without an
    // IPv6 route fails those connects at once and moves on to the IPv4 addresses.
    let mut addrs = Vec::new();
    let mut ttl_s: Option<u64> = None;
    for records in [v6, v4].into_iter().flatten() {
        addrs.extend(records.addrs);
        if let Some(ttl) = records.ttl_s {
            ttl_s = Some(ttl_s.map_or(ttl, |current| current.min(ttl)));
        }
    }
    if addrs.is_empty() {
        return Err(match failed_query {
            Some(reason) => DohFailure::Resolver(reason),
            None => DohFailure::NoRecords,
        });
    }

    let ttl = Duration::from_secs(ttl_s.unwrap_or(300)).clamp(MIN_DNS_TTL, MAX_DNS_TTL);
    Ok(CachedAnswer {
        addrs: Arc::new(addrs),
        ttl,
    })
}

/// Names that only the local resolver can answer.
fn is_local_name(host: &str) -> bool {
    const LOCAL_SUFFIXES: [&str; 8] = [
        ".localhost", ".local", ".lan", ".internal", ".intranet", ".home", ".home.arpa", ".test",
    ];
    !host.contains('.') || LOCAL_SUFFIXES.iter().any(|suffix| host.ends_with(suffix))
}

async fn system_lookup(host: &str) -> Result<Vec<IpAddr>, std::io::Error> {
    let addrs = tokio::net::lookup_host((host, 0)).await?;
    Ok(addrs.map(|addr| addr.ip()).collect())
}

/// Hosts that looked blocked through the system resolver; in Automatic mode they use DoH for
/// the rest of the session.
static SWITCHED_TO_DOH: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

fn switched_to_doh(host: &str) -> bool {
    SWITCHED_TO_DOH.lock().unwrap_or_else(|p| p.into_inner()).contains(host)
}

fn switch_to_doh(host: &str) {
    let mut hosts = SWITCHED_TO_DOH.lock().unwrap_or_else(|p| p.into_inner());
    if hosts.len() < 4096 && hosts.insert(host.to_string()) {
        tracing::info!("{} looks blocked by the system DNS; using DNS over HTTPS for it", host);
    }
}

/// Whether a host is looked up through DoH before the system resolver.
pub fn doh_first(mode: DohMode, switched: bool) -> bool {
    match mode {
        DohMode::On => true,
        DohMode::Off => false,
        DohMode::Auto => switched,
    }
}

/// True for a system DNS answer that cannot be the public site: no address, or only addresses
/// no public site has (a resolver that blocks a name often answers like this).
pub fn looks_blocked_answer(addrs: &[IpAddr]) -> bool {
    addrs.iter().all(|ip| match ip {
        IpAddr::V4(v4) => v4.is_unspecified() || v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_unspecified() || v6.is_loopback(),
    })
}

/// Whether a DoH lookup's result is final. In Automatic mode a name DoH has no address for
/// goes back to the system resolver, which may know it (split-horizon or router names).
pub fn use_doh_answer(mode: DohMode, found: bool) -> bool {
    found || mode == DohMode::On
}

/// A connection to `host` failed for `reason`. In Automatic mode a site that cannot be
/// connected to is looked up through DoH from now on, since a blocking resolver often hands
/// out an address that resets or refuses the connection.
pub fn note_unreachable_host(host: &str, reason: &str) {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || is_local_name(&host) || host.parse::<IpAddr>().is_ok() || reason.is_empty() {
        return;
    }
    if app_settings::current().doh_mode == DohMode::Auto {
        switch_to_doh(&host);
    }
}

/// The DoH answer for `host`, or `None` when the resolver could not be used and the system
/// resolver should answer instead.
async fn doh_resolve(
    host: &str,
    provider: DohProvider,
) -> Option<Result<Vec<IpAddr>, Box<dyn std::error::Error + Send + Sync>>> {
    let no_such_host = || format!("no such host: {}", host);
    if DOH_MISSING.contains_key(host) {
        return Some(Err(no_such_host().into()));
    }
    let lookup = DOH_CACHE
        .try_get_with(host.to_string(), doh_lookup(host, provider))
        .await;
    match lookup {
        Ok(answer) => Some(Ok(answer.addrs.as_ref().clone())),
        Err(failure) => match &*failure {
            // The resolver answered: the name has no address. Asking the system resolver
            // instead would only invite the made-up answers DoH is there to avoid.
            DohFailure::NoRecords => {
                DOH_MISSING.insert(host.to_string(), ()).await;
                Some(Err(no_such_host().into()))
            }
            DohFailure::Resolver(reason) => {
                mark_doh_down();
                tracing::warn!(
                    "DNS-over-HTTPS lookup of {} failed ({}); falling back to the system resolver",
                    host,
                    reason
                );
                None
            }
        },
    }
}

async fn resolve_host(host: &str) -> Result<Vec<IpAddr>, Box<dyn std::error::Error + Send + Sync>> {
    let settings = app_settings::current();
    let usable = !is_local_name(host) && !doh_is_down();
    if usable && doh_first(settings.doh_mode, switched_to_doh(host)) {
        match doh_resolve(host, settings.doh_provider).await {
            Some(answer) if use_doh_answer(settings.doh_mode, answer.is_ok()) => return answer,
            _ => return Ok(system_lookup(host).await?),
        }
    }
    let system = system_lookup(host).await;
    if usable && settings.doh_mode == DohMode::Auto {
        let blocked = match &system {
            Err(_) => true,
            Ok(addrs) => looks_blocked_answer(addrs),
        };
        if blocked {
            if let Some(Ok(addrs)) = doh_resolve(host, settings.doh_provider).await {
                switch_to_doh(host);
                return Ok(addrs);
            }
        }
    }
    Ok(system?)
}

/// reqwest resolver that honours the `doh_mode` / `doh_provider` settings.
#[derive(Debug, Default, Clone, Copy)]
pub struct DohResolver;

impl Resolve for DohResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().trim_end_matches('.').to_ascii_lowercase();
            let addrs = resolve_host(&host).await?;
            let addrs: Addrs = Box::new(addrs.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addrs)
        })
    }
}

fn base_builder() -> ClientBuilder {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .gzip(true)
        .brotli(true)
        .dns_resolver(Arc::new(DohResolver))
}

fn build_error(e: reqwest::Error) -> AppError {
    AppError::Other(format!("Could not initialise the HTTP client: {}", e))
}

static SHARED_CLIENT: OnceCell<Client> = OnceCell::new();
static DOWNLOAD_CLIENT: OnceCell<Client> = OnceCell::new();
static NO_REDIRECT_CLIENT: OnceCell<Client> = OnceCell::new();

/// The shared client: 30s total timeout, follows redirects, resolves through `DohResolver`.
/// Use `RequestBuilder::timeout` where a request needs a shorter limit.
pub fn http_client() -> Result<Client, AppError> {
    SHARED_CLIENT
        .get_or_try_init(|| {
            base_builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .map_err(build_error)
        })
        .cloned()
}

/// Like `http_client` but with no total-request timeout (connect and read timeouts only), for
/// long downloads and the stream proxy.
pub fn try_download_client() -> Result<Client, AppError> {
    DOWNLOAD_CLIENT
        .get_or_try_init(|| {
            base_builder()
                .read_timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .map_err(build_error)
        })
        .cloned()
}

/// Shared client that never follows redirects, for callers that must re-apply their own headers
/// on every hop.
pub fn no_redirect_client() -> Result<Client, AppError> {
    NO_REDIRECT_CLIENT
        .get_or_try_init(|| {
            base_builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(build_error)
        })
        .cloned()
}

/// Infallible form of `http_client` for callers that cannot report an error. The only way the
/// client can fail to build is the TLS backend failing to initialise, which nothing in the app
/// can work without.
pub fn build_http_client() -> Client {
    http_client().unwrap_or_else(|e| panic!("{}", e))
}

/// Infallible form of `try_download_client`; see `build_http_client`.
pub fn download_client() -> Client {
    try_download_client().unwrap_or_else(|e| panic!("{}", e))
}
