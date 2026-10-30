use super::*;

pub(super) fn paired_peer(id: &str) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: "peer".into(),
        addresses: vec![types::PeerAddress::from_ip("100.64.1.9")],
        api_port: 7685,
        public_key: "pub".into(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: types::AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[test]
fn build_sync_payload_includes_self_last_active() {
    let activity = crate::ports::RecordingLastActivity::new(cfg!(debug_assertions));
    let mut mesh = MeshManager::with_last_activity("local".into(), 7685, activity.clone());
    activity.mark();
    let payload = mesh.build_sync_payload();
    let self_entry = payload
        .peers
        .iter()
        .find(|p| p.instance_id == mesh.ident.identity.instance_id)
        .expect("self in gossip");
    assert!(!self_entry.dev || cfg!(debug_assertions));
    if cfg!(debug_assertions) {
        assert_eq!(self_entry.last_active_ms, None);
    } else {
        assert!(self_entry.last_active_ms.is_some());
    }
}

#[test]
fn apply_gossip_peer_info_updates_last_active() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    mesh.paired_peers
        .insert("peer-1".into(), paired_peer("peer-1"));
    mesh.apply_gossip_peer_info(&types::GossipPeerInfo {
        instance_id: "peer-1".into(),
        instance_name: "peer".into(),
        addresses: vec![types::PeerAddress::from_ip("100.64.1.9")],
        api_port: 7685,
        dev: false,
        last_active_ms: Some(42_000),
    });
    assert_eq!(mesh.paired_peers["peer-1"].last_active_ms, Some(42_000));
}

#[test]
fn apply_offline_announce_clears_addresses_and_marks_peer_offline() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    mesh.paired_peers
        .insert("peer-1".into(), paired_peer("peer-1"));

    let change = mesh
        .apply_announce(
            "peer-1",
            &types::PeerAnnounce {
                registry: None,
                instance_id: "peer-1".into(),
                instance_name: "peer".into(),
                addresses: Vec::new(),
                api_port: 7685,
                online: false,
                web_ui: None,
                relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
            },
        )
        .unwrap();
    let peer = mesh.paired_peers.get("peer-1").unwrap();

    assert!(change.changed());
    assert!(change.addresses_changed);
    assert!(change.online_changed);
    assert!(peer.addresses.is_empty());
    assert!(!peer.online);
}

#[test]
fn apply_online_empty_announce_keeps_existing_addresses() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    mesh.paired_peers
        .insert("peer-1".into(), paired_peer("peer-1"));

    let change = mesh
        .apply_announce(
            "peer-1",
            &types::PeerAnnounce {
                registry: None,
                instance_id: "peer-1".into(),
                instance_name: "peer".into(),
                addresses: Vec::new(),
                api_port: 7685,
                online: true,
                web_ui: None,
                relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
            },
        )
        .unwrap();
    let peer = mesh.paired_peers.get("peer-1").unwrap();

    assert!(!change.changed());
    assert_eq!(peer.addresses.len(), 1);
    assert!(peer.online);
}

#[test]
fn apply_offline_announce_reports_change_when_addresses_already_empty() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    let mut peer = paired_peer("peer-1");
    peer.addresses.clear();
    mesh.insert_account_peer_for_test(peer);

    let change = mesh
        .apply_announce(
            "peer-1",
            &types::PeerAnnounce {
                registry: None,
                instance_id: "peer-1".into(),
                instance_name: "peer".into(),
                addresses: Vec::new(),
                api_port: 7685,
                online: false,
                web_ui: None,
                relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
            },
        )
        .unwrap();

    assert!(change.changed());
    assert!(change.online_changed);
    assert!(!mesh.paired_peers.get("peer-1").unwrap().online);
}
