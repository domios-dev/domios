use super::*;
use crate::types::{PairedPeer, PeerAddress, PeerAnnounce, WebUiAnnouncement, WebUiEndpoint};

fn make_mesh_with_peer(peer_id: &str, addrs: Vec<&str>) -> MeshManager {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    let peer = PairedPeer {
        instance_id: peer_id.into(),
        instance_name: "TestPeer".into(),
        addresses: addrs.into_iter().map(PeerAddress::from_ip).collect(),
        api_port: 7685,
        public_key: String::new(),
        paired_at: chrono::Utc::now().to_rfc3339(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: types::AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    };
    m.paired_peers.insert(peer_id.into(), peer);
    m
}

#[test]
fn apply_announce_replaces_address_list_wholesale() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5", "100.64.0.5"]);
    let ann = PeerAnnounce {
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.99.99.99")],
        api_port: 7777,
        online: true,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let change = m.apply_announce("peer-A", &ann).expect("ok");
    assert!(change.addresses_changed);
    assert_eq!(
        change.previous_addresses.len(),
        2,
        "old list returned for logging"
    );
    let after = &m.paired_peers["peer-A"];
    assert_eq!(after.addresses.len(), 1, "list fully replaced, not merged");
    assert_eq!(after.addresses[0].ip, "100.99.99.99");
    assert_eq!(after.api_port, 7777);
}

#[test]
fn apply_announce_stores_and_clears_web_ui() {
    let mut m = make_mesh_with_peer("peer-A", vec!["100.64.0.5"]);
    let with_https = PeerAnnounce {
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.5")],
        api_port: 7685,
        online: true,
        web_ui: Some(WebUiAnnouncement {
            protocol_version: types::RELAY_PROTOCOL_VERSION,
            endpoints: vec![WebUiEndpoint::TailscaleHttps {
                host: "peer-a.tail.ts.net".into(),
                port: 443,
            }],
        }),
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let added = m.apply_announce("peer-A", &with_https).expect("ok");
    assert!(added.web_ui_changed);
    assert_eq!(m.paired_peers["peer-A"].web_ui, with_https.web_ui);

    let without = PeerAnnounce {
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
        ..with_https
    };
    let cleared = m.apply_announce("peer-A", &without).expect("ok");
    assert!(cleared.web_ui_changed);
    assert_eq!(m.paired_peers["peer-A"].web_ui, None);
}

#[test]
fn apply_announce_rejects_spoofed_sender() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5"]);
    let ann = PeerAnnounce {
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.99.99.99")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let err = m.apply_announce("peer-B", &ann).unwrap_err();
    assert!(err.contains("cannot announce"), "got: {}", err);
    assert_eq!(
        m.paired_peers["peer-A"].addresses[0].ip, "192.168.1.5",
        "untouched"
    );
}

#[test]
fn apply_announce_empty_list_is_noop() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5"]);
    let ann = PeerAnnounce {
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let res = m.apply_announce("peer-A", &ann).expect("ok");
    assert!(!res.changed(), "empty list does not clear cache");
    assert_eq!(m.paired_peers["peer-A"].addresses.len(), 1);
}

#[test]
fn apply_announce_same_addrs_returns_no_change() {
    let mut m = make_mesh_with_peer("peer-A", vec!["100.64.0.5"]);
    let ann = PeerAnnounce {
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.5")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
    };
    let res = m.apply_announce("peer-A", &ann).expect("ok");
    assert!(!res.changed(), "identical list reports no change");
}

#[test]
fn set_peer_addr_override_replaces_with_single_entry() {
    let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5", "100.64.0.5"]);
    let prev = m
        .set_peer_addr_override("peer-A", "10.0.0.99", Some(8888))
        .expect("ok");
    assert_eq!(prev.len(), 2);
    let after = &m.paired_peers["peer-A"];
    assert_eq!(after.addresses.len(), 1);
    assert_eq!(after.addresses[0].ip, "10.0.0.99");
    assert_eq!(after.api_port, 8888);
}

#[test]
fn set_peer_addr_override_unknown_peer_errors() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    let err = m
        .set_peer_addr_override("ghost", "10.0.0.1", None)
        .unwrap_err();
    assert!(err.contains("not paired"), "got: {}", err);
}

#[test]
fn build_announce_includes_tailscale_and_lan() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.ready_listeners = vec!["0.0.0.0:7685".parse().unwrap()];
    m.disk_persist = false;
    m.tailscale_ip = Some("100.65.0.30".into());
    m.local_ip = Some("192.168.1.5".into());
    let ann = m.build_announce();
    assert_eq!(ann.addresses.len(), 2);
    assert!(ann.addresses[0].is_tailscale, "tailscale first");
    assert_eq!(ann.addresses[0].ip, "100.65.0.30");
    assert_eq!(ann.addresses[1].ip, "192.168.1.5");
    assert!(!ann.addresses[1].is_tailscale);
    assert_eq!(ann.api_port, 7685);
}

#[test]
fn build_announce_skips_public_local_ip() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    m.tailscale_ip = None;
    m.local_ip = Some("8.8.8.8".into());
    let ann = m.build_announce();
    assert!(ann.addresses.is_empty(), "public IPs are not advertised");
}

#[test]
fn loopback_listener_with_verified_serve_announces_online() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    m.local_http_healthy = true;
    m.ready_listeners = vec!["127.0.0.1:7685".parse().unwrap()];
    m.tailscale_ip = Some("100.65.0.30".into());
    m.local_web_ui = Some(WebUiAnnouncement {
        protocol_version: types::RELAY_PROTOCOL_VERSION,
        endpoints: vec![WebUiEndpoint::TailscaleHttp {
            ip: "100.65.0.30".parse().unwrap(),
            port: 7685,
        }],
    });

    let announcement = m.build_announce();

    assert!(announcement.online);
    assert_eq!(announcement.addresses.len(), 1);
    assert_eq!(announcement.addresses[0].ip, "100.65.0.30");
    assert!(announcement.addresses[0].is_tailscale);
    assert_eq!(announcement.web_ui, m.local_web_ui);
}

#[test]
fn loopback_listener_without_verified_serve_stays_offline() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    m.local_http_healthy = true;
    m.ready_listeners = vec!["127.0.0.1:7685".parse().unwrap()];
    m.tailscale_ip = Some("100.65.0.30".into());

    let announcement = m.build_announce();

    assert!(!announcement.online);
    assert!(announcement.addresses.is_empty());
    assert!(announcement.web_ui.is_none());
}

#[test]
fn mesh_reachability_remote_loss_withdraws_paired_cached_addresses() {
    let mut sender = MeshManager::new("sender".into(), 7690);
    sender.disk_persist = false;
    sender.local_http_healthy = true;
    sender.ready_listeners = vec!["127.0.0.1:7690".parse().unwrap()];
    let id = sender.ident.identity.instance_id.clone();
    let mut receiver = make_mesh_with_peer(&id, vec!["100.64.0.1"]);
    let announcement = sender.build_announce();
    assert!(!announcement.online);
    assert!(announcement.addresses.is_empty());
    receiver.apply_announce(&id, &announcement).unwrap();
    assert!(receiver.paired_peers[&id].addresses.is_empty());
    assert!(!receiver.paired_peers[&id].online);
}

#[test]
fn client_for_picks_tailscale_via_tier_ranking() {
    let mut m = MeshManager::new("test-host".into(), 7685);
    m.disk_persist = false;
    let peer = PairedPeer {
        instance_id: "peer-A".into(),
        instance_name: "TestPeer".into(),
        addresses: vec![
            PeerAddress {
                ip: "8.8.8.8".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "192.168.1.2".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "100.65.0.30".into(),
                is_tailscale: true,
            },
        ],
        api_port: 7685,
        public_key: String::new(),
        paired_at: chrono::Utc::now().to_rfc3339(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: types::AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: 0,
        consecutive_misses: 0,
        last_active_ms: None,
    };
    m.paired_peers.insert("peer-A".into(), peer);

    let picked = m.paired_peers["peer-A"]
        .addresses
        .iter()
        .min_by_key(|a| types::address_tier(a))
        .unwrap();
    assert_eq!(picked.ip, "100.65.0.30");
}

#[test]
fn mesh_rename_announce_stays_queued_until_the_peer_acknowledges() {
    let mut m = make_mesh_with_peer("peer-1", vec!["100.64.0.9"]);
    assert!(!m.has_unacked_announce("peer-1"));
    m.record_announce_outcome("peer-1", false);
    assert!(m.has_unacked_announce("peer-1"));
    m.record_announce_outcome("peer-1", false);
    assert!(m.has_unacked_announce("peer-1"), "a repeat failure keeps one entry");
    m.record_announce_outcome("peer-1", true);
    assert!(!m.has_unacked_announce("peer-1"));
}
