use std::net::{IpAddr, SocketAddr};

use crate::types::PeerAddress;

pub fn advertised_addresses(
    listeners: &[SocketAddr],
    candidates: &[PeerAddress],
    port: u16,
) -> Vec<PeerAddress> {
    let mut addresses = Vec::new();
    for listener in listeners.iter().filter(|address| address.port() == port) {
        if listener.ip().is_loopback() {
            continue;
        }
        if listener.ip().is_unspecified() {
            for candidate in candidates {
                if let Ok(ip) = candidate.ip.parse::<IpAddr>() {
                    if ip.is_ipv4() == listener.is_ipv4() && eligible(ip) {
                        addresses.push(PeerAddress::from_ip(&ip.to_string()));
                    }
                }
            }
        } else if eligible(listener.ip()) {
            addresses.push(PeerAddress::from_ip(&listener.ip().to_string()));
        }
    }
    addresses.sort_by(|left, right| left.ip.cmp(&right.ip));
    addresses.dedup_by(|left, right| left.ip == right.ip);
    addresses
}

pub(crate) fn eligible(ip: IpAddr) -> bool {
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && match ip {
            IpAddr::V4(ip) => !ip.is_link_local(),
            IpAddr::V6(ip) => !ip.is_unicast_link_local(),
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_reachability_loopback_and_unbound_are_not_advertised() {
        let candidates = [PeerAddress::from_ip("192.168.1.2")];
        assert!(advertised_addresses(&[], &candidates, 7690).is_empty());
        assert!(
            advertised_addresses(&["127.0.0.1:7690".parse().unwrap()], &candidates, 7690)
                .is_empty()
        );
    }

    #[test]
    fn mesh_reachability_concrete_tailnet_excludes_other_interfaces() {
        let candidates = [
            PeerAddress::from_ip("192.168.1.2"),
            PeerAddress::from_ip("fe80::1"),
        ];
        let live = [
            "127.0.0.1:7690".parse().unwrap(),
            "100.84.159.37:7690".parse().unwrap(),
        ];
        let result = advertised_addresses(&live, &candidates, 7690);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ip, "100.84.159.37");
        assert!(result[0].is_tailscale);
        assert!(advertised_addresses(&live, &candidates, 7686).is_empty());
    }

    #[test]
    fn mesh_reachability_wildcard_filters_family_and_link_local() {
        let candidates = [
            PeerAddress::from_ip("192.168.1.2"),
            PeerAddress::from_ip("169.254.1.2"),
            PeerAddress::from_ip("::1"),
        ];
        let result = advertised_addresses(&["0.0.0.0:7690".parse().unwrap()], &candidates, 7690);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ip, "192.168.1.2");
    }
}
