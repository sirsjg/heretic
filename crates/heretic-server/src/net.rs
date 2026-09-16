//! Which addresses this machine can be reached at.
//!
//! Used for the pairing screen: the desktop shows the phone a link, and the
//! link has to carry an address the phone can actually open. A Tailscale
//! address is preferred, since it works from anywhere and reaches nobody
//! else; a LAN address works from the sofa.

use std::net::{IpAddr, Ipv4Addr};

/// One way in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Interface {
    pub ip: IpAddr,
    /// What the address is, for the person choosing: `Tailscale`, or the
    /// interface's name.
    pub label: String,
    pub tailscale: bool,
}

/// Every address worth offering, best first.
pub fn interfaces() -> Vec<Interface> {
    let mut found: Vec<Interface> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|iface| match iface.ip() {
            IpAddr::V4(ip) if usable(ip) => Some(Interface {
                tailscale: is_tailscale(ip),
                label: if is_tailscale(ip) {
                    "Tailscale".into()
                } else {
                    iface.name.clone()
                },
                ip: IpAddr::V4(ip),
            }),
            _ => None,
        })
        .collect();
    found.sort_by_key(|iface| (!iface.tailscale, iface.ip));
    found.dedup_by_key(|iface| iface.ip);
    found
}

fn usable(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() && !ip.is_broadcast()
}

/// Tailscale hands out addresses from the carrier-grade NAT range,
/// 100.64.0.0/10, and nothing else on a normal machine does.
pub fn is_tailscale(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tailscale_range_is_recognised() {
        assert!(is_tailscale(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_tailscale(Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!is_tailscale(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_tailscale(Ipv4Addr::new(100, 63, 0, 1)));
        assert!(!is_tailscale(Ipv4Addr::new(192, 168, 1, 10)));
    }

    #[test]
    fn loopback_and_link_local_are_not_offered() {
        assert!(!usable(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!usable(Ipv4Addr::new(169, 254, 1, 1)));
        assert!(usable(Ipv4Addr::new(192, 168, 1, 10)));
    }
}
