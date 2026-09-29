use super::*;
use crate::discovered::DiscoverySighting;
use crate::types::{AccessLevel, DiscoveryMethod, GossipPeerInfo, PairedPeer, PeerAddress};

fn gossip_peer(id: &str) -> GossipPeerInfo {
    GossipPeerInfo {
        instance_id: id.into(),
        instance_name: id.into(),
        addresses: vec![PeerAddress::from_ip("100.64.1.9")],
        api_port: 7685,
        dev: false,
        last_active_ms: None,
    }
}

fn paired_peer(id: &str) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: id.into(),
        addresses: vec![PeerAddress::from_ip("100.64.1.9")],
        api_port: 7685,
        public_key: "pub".into(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: 0,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[test]
fn gossip_snapshot_reconciles_voucher_claims_and_stale_replay_cannot_resurrect() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    let snapshot_a = vec![gossip_peer("old-id")];

    mesh.apply_gossip_snapshot("voucher", 1, &snapshot_a, 100);
    assert_eq!(mesh.discovered_peers["old-id"].vouched_by, ["voucher"]);
    mesh.apply_gossip_snapshot("voucher", 2, &[], 101);
    assert!(!mesh.discovered_peers.contains_key("old-id"));

    mesh.apply_gossip_snapshot("voucher", 1, &snapshot_a, 102);
    assert!(!mesh.discovered_peers.contains_key("old-id"));
    assert!(mesh.client_for("old-id").is_none());
}

#[test]
fn gossip_snapshot_omission_keeps_direct_discovery_and_its_addresses() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    mesh.upsert_discovered(
        DiscoverySighting {
            instance_id: "old-id".into(),
            instance_name: "old".into(),
            addresses: vec![PeerAddress::from_ip("192.168.1.9")],
            api_port: 7685,
            method: DiscoveryMethod::Udp,
            dev: false,
            mesh_id: None,
            voucher: None,
        },
        99,
    );
    let snapshot_a = vec![gossip_peer("old-id")];
    mesh.apply_gossip_snapshot("voucher", 1, &snapshot_a, 100);
    mesh.apply_gossip_snapshot("voucher", 2, &[], 101);

    let peer = &mesh.discovered_peers["old-id"];
    assert!(peer.vouched_by.is_empty());
    assert_eq!(peer.discovery_method, DiscoveryMethod::Udp);
    assert!(peer
        .addresses
        .iter()
        .any(|address| address.ip == "192.168.1.9"));
}

#[test]
fn upsert_discovered_rejects_blocked_or_revoked_identity() {
    for blocked in [true, false] {
        let mut mesh = MeshManager::new("local".into(), 7685);
        mesh.disk_persist = false;
        if blocked {
            mesh.revocation_list.blocked.push("old-id".into());
        } else {
            mesh.revocation_list.revoked.push("old-id".into());
        }
        assert!(!mesh.upsert_discovered(
            DiscoverySighting {
                instance_id: "old-id".into(),
                instance_name: "old".into(),
                addresses: vec![PeerAddress::from_ip("100.64.1.9")],
                api_port: 7685,
                method: DiscoveryMethod::Gossip,
                dev: false,
                mesh_id: None,
                voucher: Some("voucher".into()),
            },
            100,
        ));
        assert!(!mesh.discovered_peers.contains_key("old-id"));
    }
}

#[test]
fn blocked_or_revoked_paired_peer_cannot_route() {
    for blocked in [true, false] {
        let mut mesh = MeshManager::new("local".into(), 7685);
        mesh.disk_persist = false;
        mesh.paired_peers
            .insert("old-id".into(), paired_peer("old-id"));
        if blocked {
            mesh.revocation_list.blocked.push("old-id".into());
        } else {
            mesh.revocation_list.revoked.push("old-id".into());
        }
        assert!(mesh.client_for("old-id").is_none());
    }
}

#[test]
fn unpair_tombstone_survives_restart_and_rejects_stale_gossip() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    mesh.paired_peers
        .insert("old-id".into(), paired_peer("old-id"));
    assert!(mesh.unpair("old-id").is_some());
    let generation = mesh.revocation_list.tombstones["old-id"];

    let persisted = serde_json::to_string(&mesh.revocation_list).unwrap();
    let mut restarted = MeshManager::new("restarted".into(), 7685);
    restarted.disk_persist = false;
    restarted.revocation_list = serde_json::from_str(&persisted).unwrap();
    assert_eq!(restarted.revocation_list.tombstones["old-id"], generation);

    restarted.apply_gossip_snapshot("voucher", 1, &[gossip_peer("old-id")], 100);
    assert!(!restarted.discovered_peers.contains_key("old-id"));
    assert!(restarted.client_for("old-id").is_none());

    let re_pair_id = "31c7cff6-4bd2-43b0-a5cd-b5db9bfbb673";
    restarted.pairing_targets.insert(
        "re-pair".into(),
        ("100.64.1.9".into(), 7685, re_pair_id.into()),
    );
    restarted.tombstone_peer(re_pair_id);
    assert!(
        restarted
            .validate_pairing_accept_identity("re-pair", re_pair_id)
            .is_err(),
        "a tombstoned identity re-pairs only after an explicit owner unrevoke"
    );
    restarted.unrevoke(re_pair_id).unwrap();
    assert!(restarted
        .validate_pairing_accept_identity("re-pair", re_pair_id)
        .is_ok());
    restarted.complete_explicit_pairing(paired_peer(re_pair_id));
    assert!(restarted.client_for(re_pair_id).is_some());

    assert!(
        restarted.client_for("old-id").is_none(),
        "an identity nobody unrevoked stays tombstoned"
    );
}

#[test]
fn gossip_tombstones_are_bounded_to_the_current_voucher_generation() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    for generation in 1..=100 {
        let peer = gossip_peer(&format!("peer-{generation}"));
        mesh.apply_gossip_snapshot("voucher", generation * 2, &[peer], generation);
        mesh.apply_gossip_snapshot("voucher", generation * 2 + 1, &[], generation);
        assert!(
            mesh.gossip_tombstones.len() <= 1,
            "old generations must not accumulate tombstones"
        );
    }
}
