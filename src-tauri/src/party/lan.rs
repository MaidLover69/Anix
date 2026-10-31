//! Which network interfaces a watch party may use, the address a guest types, and mDNS
//! discovery as `_anix-party._tcp`.
//!
//! A party lives on the local network only. The host listens and announces only on private
//! IPv4 addresses (10/8, 172.16/12, 192.168/16) of real interfaces, never on a VPN or a
//! container bridge, so it cannot be reached from the internet or another machine's tunnel. A
//! guest connects only to such an address too.

use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

/// The port shown in the firewall help; the next free one is used when it is taken.
pub const DEFAULT_PORT: u16 = 47830;
/// Ports tried after the default one.
pub const PORT_TRIES: u16 = 10;
/// The mDNS service type.
pub const SERVICE_TYPE: &str = "_anix-party._tcp.local.";

/// Name prefixes of VPN, tunnel, container and virtual-machine interfaces.
const SKIPPED_PREFIXES: &[&str] = &[
    "lo", "tun", "tap", "wg", "utun", "ppp", "ipsec", "tailscale", "zt", "nordlynx", "proton", "mullvad", "docker",
    "br-", "virbr", "veth", "podman", "cni", "flannel", "cali", "vmnet", "vboxnet", "lxc", "lxd", "incus",
    "waydroid", "anbox", "vpn",
];

/// True for an interface the party never uses.
pub fn is_skipped_interface(name: &str) -> bool {
    SKIPPED_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// True for a private IPv4 address, the kind home and office networks hand out.
pub fn is_lan_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

/// The machine's usable local addresses with their interface names, sorted.
pub fn lan_addresses() -> Vec<(String, Ipv4Addr)> {
    let mut found: Vec<(String, Ipv4Addr)> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|iface| iface.is_oper_up() && !iface.is_p2p() && !is_skipped_interface(&iface.name))
        .filter_map(|iface| match iface.ip() {
            IpAddr::V4(ip) if is_lan_ipv4(ip) => Some((iface.name, ip)),
            _ => None,
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Reads the host address a guest typed: `192.168.1.20` or `192.168.1.20:47831`. Only a
/// private IPv4 address is accepted.
pub fn parse_host_address(input: &str) -> Option<SocketAddr> {
    let input = input.trim();
    let addr = input
        .parse::<SocketAddr>()
        .ok()
        .or_else(|| input.parse::<Ipv4Addr>().ok().map(|ip| SocketAddr::new(IpAddr::V4(ip), DEFAULT_PORT)))?;
    match addr.ip() {
        IpAddr::V4(ip) if is_lan_ipv4(ip) && addr.port() != 0 => Some(addr),
        _ => None,
    }
}

/// This machine's name, for the party's mDNS entry.
fn host_name() -> String {
    let name = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let clean: String = name.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(40).collect();
    if clean.is_empty() {
        "anix".to_string()
    } else {
        clean
    }
}

/// Announces a hosted party until dropped.
pub struct Advertiser {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Advertiser {
    /// Announces `port` on `addresses`, through their interfaces only.
    pub fn start(port: u16, addresses: &[(String, Ipv4Addr)]) -> Result<Self, String> {
        let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
        limit_interfaces(&daemon, addresses);
        let name = host_name();
        let ips: Vec<IpAddr> = addresses.iter().map(|(_, ip)| IpAddr::V4(*ip)).collect();
        let info = ServiceInfo::new(SERVICE_TYPE, &name, &format!("{name}.local."), &ips[..], port, None::<std::collections::HashMap<String, String>>)
            .map_err(|e| e.to_string())?;
        let fullname = info.get_fullname().to_string();
        daemon.register(info).map_err(|e| e.to_string())?;
        Ok(Self { daemon, fullname })
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

fn limit_interfaces(daemon: &ServiceDaemon, addresses: &[(String, Ipv4Addr)]) {
    let _ = daemon.disable_interface(IfKind::All);
    for (name, _) in addresses {
        let _ = daemon.enable_interface(IfKind::Name(name.clone()));
    }
}

/// A party found on the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundParty {
    /// The host machine's name as it announced it (shown as text only).
    pub name: String,
    /// `ip:port`, ready for joining.
    pub address: String,
}

/// Looks for parties for `wait`. Entries whose address is not a private IPv4 are ignored.
pub fn discover(wait: Duration) -> Result<Vec<FoundParty>, String> {
    let addresses = lan_addresses();
    if addresses.is_empty() {
        return Ok(Vec::new());
    }
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    limit_interfaces(&daemon, &addresses);
    let events = daemon.browse(SERVICE_TYPE).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + wait;
    let mut found: Vec<FoundParty> = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else { break };
        let ServiceEvent::ServiceResolved(service) = event else { continue };
        let name: String = service
            .fullname
            .trim_end_matches(SERVICE_TYPE)
            .trim_end_matches('.')
            .chars()
            .filter(|c| !c.is_control())
            .take(63)
            .collect();
        for ip in service.get_addresses_v4() {
            if !is_lan_ipv4(ip) || service.port == 0 {
                continue;
            }
            let address = SocketAddr::new(IpAddr::V4(ip), service.port).to_string();
            if !found.iter().any(|f| f.address == address) {
                found.push(FoundParty { name: name.clone(), address });
            }
        }
    }
    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();
    found.sort_by(|a, b| a.address.cmp(&b.address));
    Ok(found)
}
