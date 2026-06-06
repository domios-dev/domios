//! Merge-on-ingest for discovered peers.
//!
//! Every path that learns about a peer (mDNS, UDP beacon, gossip sync inbound,
//! background sync-poll) must funnel through `merge_sighting` instead of
//! blind-inserting a fresh `DiscoveredPeer`. Direct inserts clobbered
//! gossip-merged `vouched_by` (and cached addresses), which silently disabled
//! auto-pair for any LAN-visible peer (#126).

use super::types::{sanitize_addresses, DiscoveredPeer, DiscoveryMethod, PeerAddress};

/// One observation of a peer, from any discovery source.
pub struct DiscoverySighting {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    pub method: DiscoveryMethod,
    pub dev: bool,
    pub mesh_id: Option<String>,
    /// Paired peer that vouched for this sighting (gossip paths only).
    pub voucher: Option<String>,
}

/// Merge a fresh sighting into the existing record (if any).
///
/// - addresses: sanitized new first, then existing ones not re-observed —
///   a direct UDP beacon (single source ip) must not erase the Tailscale
///   address learned via gossip.
/// - vouched_by: union — direct discovery must not erase gossip vouchers.
/// - method: a direct observation (mDNS/UDP/manual) outranks Gossip; Gossip
///   never downgrades a previously direct method.
/// - mesh_id: fresh Some wins, otherwise keep what we had.
pub fn merge_sighting(
    existing: Option<&DiscoveredPeer>,
    sighting: DiscoverySighting,
    now: u64,
) -> DiscoveredPeer {
    let mut addresses = sanitize_addresses(&sighting.addresses);
    let mut vouched_by: Vec<String> = sighting.voucher.into_iter().collect();
    let mut method = sighting.method;
    let mut mesh_id = sighting.mesh_id;
    let mut instance_name = sighting.instance_name;

    if let Some(prev) = existing {
        for addr in &prev.addresses {
            if !addresses.iter().any(|a| a.ip == addr.ip) {
                addresses.push(addr.clone());
            }
        }
        vouched_by.extend(prev.vouched_by.iter().cloned());
        if method == DiscoveryMethod::Gossip && prev.discovery_method != DiscoveryMethod::Gossip {
            method = prev.discovery_method.clone();
        }
        if mesh_id.is_none() {
            mesh_id = prev.mesh_id.clone();
        }
        if instance_name.is_empty() {
            instance_name = prev.instance_name.clone();
        }
    }
    vouched_by.sort();
    vouched_by.dedup();

    DiscoveredPeer {
        instance_id: sighting.instance_id,
        instance_name,
        addresses,
        api_port: sighting.api_port,
        discovery_method: method,
        last_seen: now,
        dev: sighting.dev,
        mesh_id,
        vouched_by,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gossip_sighting(id: &str, voucher: &str, ips: &[&str]) -> DiscoverySighting {
        DiscoverySighting {
            instance_id: id.to_string(),
            instance_name: "peer".to_string(),
            addresses: ips.iter().map(|ip| PeerAddress::from_ip(ip)).collect(),
            api_port: 7685,
            method: DiscoveryMethod::Gossip,
            dev: false,
            mesh_id: None,
            voucher: Some(voucher.to_string()),
        }
    }

    fn udp_sighting(id: &str, ip: &str) -> DiscoverySighting {
        DiscoverySighting {
            instance_id: id.to_string(),
            instance_name: "peer".to_string(),
            addresses: vec![PeerAddress::from_ip(ip)],
            api_port: 7685,
            method: DiscoveryMethod::Udp,
            dev: false,
            mesh_id: None,
            voucher: None,
        }
    }

    /// #126 regression: a direct UDP beacon arriving after gossip must not
    /// erase the vouchers — that erasure kept auto-approve from ever firing.
    #[test]
    fn udp_sighting_preserves_gossip_vouchers() {
        let after_gossip =
            merge_sighting(None, gossip_sighting("p1", "mediator", &["100.64.0.9"]), 100);
        assert_eq!(after_gossip.vouched_by, vec!["mediator".to_string()]);

        let after_udp = merge_sighting(Some(&after_gossip), udp_sighting("p1", "192.168.1.50"), 110);
        assert_eq!(
            after_udp.vouched_by,
            vec!["mediator".to_string()],
            "direct discovery clobbered gossip vouchers"
        );
    }

    /// #126 regression: the UDP beacon carries only its source ip; the
    /// Tailscale address learned via gossip must survive the merge.
    #[test]
    fn udp_sighting_preserves_known_addresses() {
        let after_gossip =
            merge_sighting(None, gossip_sighting("p1", "mediator", &["100.64.0.9"]), 100);
        let after_udp = merge_sighting(Some(&after_gossip), udp_sighting("p1", "192.168.1.50"), 110);
        let ips: Vec<&str> = after_udp.addresses.iter().map(|a| a.ip.as_str()).collect();
        assert!(ips.contains(&"192.168.1.50"));
        assert!(ips.contains(&"100.64.0.9"), "gossip address dropped");
        assert_eq!(after_udp.last_seen, 110);
    }

    #[test]
    fn gossip_does_not_downgrade_direct_method() {
        let direct = merge_sighting(None, udp_sighting("p1", "192.168.1.50"), 100);
        let merged = merge_sighting(Some(&direct), gossip_sighting("p1", "m", &["100.64.0.9"]), 110);
        assert_eq!(merged.discovery_method, DiscoveryMethod::Udp);

        let upgraded = merge_sighting(Some(&merged), udp_sighting("p1", "192.168.1.50"), 120);
        assert_eq!(upgraded.discovery_method, DiscoveryMethod::Udp);
    }

    #[test]
    fn vouchers_union_sorted_dedup() {
        let a = merge_sighting(None, gossip_sighting("p1", "zeta", &["100.64.0.9"]), 100);
        let b = merge_sighting(Some(&a), gossip_sighting("p1", "alpha", &["100.64.0.9"]), 110);
        let c = merge_sighting(Some(&b), gossip_sighting("p1", "alpha", &["100.64.0.9"]), 120);
        assert_eq!(c.vouched_by, vec!["alpha".to_string(), "zeta".to_string()]);
    }

    #[test]
    fn addresses_are_sanitized_and_re_observed_ip_not_duplicated() {
        let poisoned = DiscoverySighting {
            addresses: vec![
                PeerAddress::from_ip("not-an-ip"),
                PeerAddress::from_ip("192.168.1.50"),
            ],
            ..udp_sighting("p1", "ignored")
        };
        let first = merge_sighting(None, poisoned, 100);
        let ips: Vec<&str> = first.addresses.iter().map(|a| a.ip.as_str()).collect();
        assert_eq!(ips, vec!["192.168.1.50"]);

        let again = merge_sighting(Some(&first), udp_sighting("p1", "192.168.1.50"), 110);
        assert_eq!(again.addresses.len(), 1);
    }

    #[test]
    fn mesh_id_fresh_some_wins_none_keeps_previous() {
        let mut s = udp_sighting("p1", "192.168.1.50");
        s.mesh_id = Some("mesh-a".to_string());
        let first = merge_sighting(None, s, 100);
        assert_eq!(first.mesh_id.as_deref(), Some("mesh-a"));

        let keep = merge_sighting(Some(&first), udp_sighting("p1", "192.168.1.50"), 110);
        assert_eq!(keep.mesh_id.as_deref(), Some("mesh-a"));

        let mut replace = udp_sighting("p1", "192.168.1.50");
        replace.mesh_id = Some("mesh-b".to_string());
        let replaced = merge_sighting(Some(&keep), replace, 120);
        assert_eq!(replaced.mesh_id.as_deref(), Some("mesh-b"));
    }
}
