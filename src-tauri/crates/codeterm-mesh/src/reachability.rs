use std::net::{IpAddr, SocketAddr};

use crate::types::{PeerAddress, WebUiAnnouncement, WebUiEndpoint};

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

pub fn advertised_addresses_with_verified_forward(
    listeners: &[SocketAddr],
    candidates: &[PeerAddress],
    port: u16,
    verified_web_ui: Option<&WebUiAnnouncement>,
) -> Vec<PeerAddress> {
    let mut addresses = advertised_addresses(listeners, candidates, port);
    let loopback_ready = listeners
        .iter()
        .any(|listener| listener.port() == port && listener.ip().is_loopback());
    if loopback_ready {
        for candidate in candidates {
            let Ok(ip) = candidate.ip.parse::<IpAddr>() else {
                continue;
            };
            let verified = verified_web_ui.is_some_and(|announcement| {
                announcement.protocol_version == crate::types::RELAY_PROTOCOL_VERSION
                    && crate::types::is_tailscale_ip(&candidate.ip)
                    && announcement.endpoints.iter().any(|endpoint| {
                        matches!(
                            endpoint,
                            WebUiEndpoint::TailscaleHttp {
                                ip: endpoint_ip,
                                port: endpoint_port,
                            } if *endpoint_ip == ip && *endpoint_port == port
                        )
                    })
            });
            if verified && eligible(ip) {
                addresses.push(PeerAddress::from_ip(&candidate.ip));
            }
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

    #[test]
    fn mesh_reachability_loopback_accepts_only_matching_verified_forward() {
        let listeners = ["127.0.0.1:7690".parse().unwrap()];
        let candidates = [
            PeerAddress::from_ip("100.84.159.37"),
            PeerAddress::from_ip("192.168.1.2"),
        ];
        let verified_web_ui = WebUiAnnouncement {
            protocol_version: crate::types::RELAY_PROTOCOL_VERSION,
            endpoints: vec![WebUiEndpoint::TailscaleHttp {
                ip: "100.84.159.37".parse().unwrap(),
                port: 7690,
            }],
        };

        let result = advertised_addresses_with_verified_forward(
            &listeners,
            &candidates,
            7690,
            Some(&verified_web_ui),
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ip, "100.84.159.37");
        assert!(result[0].is_tailscale);
    }

    #[test]
    fn mesh_reachability_loopback_rejects_unverified_forward() {
        let listeners = ["127.0.0.1:7690".parse().unwrap()];
        let candidates = [PeerAddress::from_ip("100.84.159.37")];

        assert!(advertised_addresses_with_verified_forward(
            &listeners,
            &candidates,
            7690,
            None,
        )
        .is_empty());
    }
}
