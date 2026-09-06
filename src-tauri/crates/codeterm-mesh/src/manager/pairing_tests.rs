use super::*;
use crate::types::{AccessLevel, PairedPeer, PairingRequest, PairingStatus, PeerAddress};

fn paired_peer_named(id: &str, name: &str, addresses: Vec<&str>) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: name.into(),
        addresses: addresses.into_iter().map(PeerAddress::from_ip).collect(),
        api_port: 7685,
        public_key: "pub".into(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: 1,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

/// codeterm#434: a peer that re-minted its instance_id and re-pairs under the
/// same name + host must not leave the old id behind as a dead resolution
/// candidate — `complete_explicit_pairing` supersedes (removes) it.
#[test]
fn re_pairing_under_the_same_name_and_host_supersedes_the_old_record() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    // `MeshManager::new` loads real disk state — start from empty so this
    // test's exact counts are never polluted by the running machine's own peers.
    mesh.paired_peers.clear();
    mesh.complete_explicit_pairing(paired_peer_named(
        "a4db6f3d-dead",
        "BLACKSTAR",
        vec!["100.65.0.30"],
    ));
    assert!(mesh.paired_peers.contains_key("a4db6f3d-dead"));

    mesh.complete_explicit_pairing(paired_peer_named(
        "2556b3f9-live",
        "BLACKSTAR",
        vec!["100.65.0.30"],
    ));

    assert!(
        !mesh.paired_peers.contains_key("a4db6f3d-dead"),
        "the old identity must stop being a resolution candidate"
    );
    assert!(mesh.paired_peers.contains_key("2556b3f9-live"));
    assert_eq!(mesh.paired_peers.len(), 1);
}

/// A same-named peer at a DIFFERENT host is a distinct machine, not a
/// re-mint — must never be silently deleted.
#[test]
fn same_name_different_host_is_not_superseded() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    mesh.paired_peers.clear();
    mesh.complete_explicit_pairing(paired_peer_named(
        "peer-a",
        "BLACKSTAR",
        vec!["100.65.0.30"],
    ));
    mesh.complete_explicit_pairing(paired_peer_named(
        "peer-b",
        "BLACKSTAR",
        vec!["100.65.0.99"],
    ));

    assert!(mesh.paired_peers.contains_key("peer-a"));
    assert!(mesh.paired_peers.contains_key("peer-b"));
    assert_eq!(mesh.paired_peers.len(), 2);
}

/// A different-named peer at the same host (e.g. two VMs behind one NAT IP)
/// must never be superseded by name+host matching alone.
#[test]
fn different_name_same_host_is_not_superseded() {
    let mut mesh = MeshManager::new("local".into(), 7685);
    mesh.disk_persist = false;
    mesh.paired_peers.clear();
    mesh.complete_explicit_pairing(paired_peer_named(
        "peer-a",
        "BLACKSTAR",
        vec!["100.65.0.30"],
    ));
    mesh.complete_explicit_pairing(paired_peer_named("peer-b", "ROG", vec!["100.65.0.30"]));

    assert!(mesh.paired_peers.contains_key("peer-a"));
    assert!(mesh.paired_peers.contains_key("peer-b"));
    assert_eq!(mesh.paired_peers.len(), 2);
}

#[test]
fn check_and_record_nonce_rejects_replay() {
    let mut mgr = MeshManager::new("test-node".to_string(), 7777);
    mgr.disk_persist = false;

    let (priv_key, pub_key) = crate::crown::generate_peer_keypair();
    mgr.ident.identity.private_key = Some(priv_key.clone());
    mgr.ident.identity.public_key = Some(pub_key.clone());
    let instance_id = mgr.ident.identity.instance_id.clone();

    let token = crate::crown::sign_request(&priv_key, &instance_id).unwrap();

    assert!(
        mgr.check_and_record_nonce(&token).is_ok(),
        "first use should succeed"
    );
    assert!(
        mgr.check_and_record_nonce(&token).is_err(),
        "replay should be rejected"
    );
}

fn pending_request(id: &str, from: &str, timestamp: u64) -> PairingRequest {
    PairingRequest {
        request_id: id.into(),
        from_instance_id: from.into(),
        from_instance_name: from.into(),
        from_address: "192.168.1.2".into(),
        from_port: 7685,
        from_addresses: vec![],
        public_key: "pk".into(),
        timestamp,
        status: PairingStatus::Pending,
        introduced_by: None,
    }
}

#[test]
fn cleanup_marks_expired_then_drops_with_pairing_targets() {
    let mut manager = MeshManager::new("test-host".into(), 7685);
    manager.disk_persist = false;
    manager
        .pending_requests
        .insert("r1".into(), pending_request("r1", "peer-a", 1000));
    manager
        .pairing_targets
        .insert("r1".into(), ("192.168.1.2".into(), 7685, "peer-a".into()));

    manager.cleanup_expired_requests_at(1000 + 301);
    let request = manager
        .pending_requests
        .get("r1")
        .expect("retained for visibility");
    assert_eq!(request.status, PairingStatus::Expired);
    assert!(manager.pairing_targets.contains_key("r1"));
    assert_eq!(manager.pending_request_count(), 0, "expired must not count");

    manager.cleanup_expired_requests_at(1000 + 3601);
    assert!(manager.pending_requests.is_empty());
    assert!(manager.pairing_targets.is_empty(), "targets must not leak");
}

#[test]
fn cleanup_expired_request_completes_its_inflight_delivery() {
    let mut manager = MeshManager::new("test-host".into(), 7685);
    manager.disk_persist = false;
    manager
        .pending_requests
        .insert("r1".into(), pending_request("r1", "peer-a", 1000));
    manager
        .pairing_targets
        .insert("r1".into(), ("192.168.1.2".into(), 7685, "peer-a".into()));
    let delivery = Arc::new(PairingDelivery::new("peer-a".into(), "r1".into()));
    manager
        .pairing_deliveries
        .insert("peer-a".into(), delivery.clone());

    manager.cleanup_expired_requests_at(1000 + 301);

    assert!(manager.pairing_deliveries.is_empty());
    assert!(manager.pending_requests.is_empty());
    assert!(manager.pairing_targets.is_empty());
    assert_eq!(
        delivery.outcome.lock().clone().unwrap().unwrap_err(),
        "Pairing request expired"
    );
}

#[test]
fn pairing_delivery_expiry_wins_over_owner_completion() {
    let delivery = PairingDelivery::new("peer-a".into(), "r1".into());

    assert!(delivery.complete(Err("Pairing request expired".to_string())));
    assert!(!delivery.complete(Ok(())));
    assert_eq!(
        delivery.outcome.lock().clone().unwrap().unwrap_err(),
        "Pairing request expired"
    );
}

#[test]
fn pairing_delivery_cancellation_wins_over_owner_completion() {
    let delivery = PairingDelivery::new("peer-a".into(), "r1".into());

    assert!(delivery.complete(Err("Pairing request delivery was cancelled".to_string())));
    assert!(!delivery.complete(Ok(())));
    assert_eq!(
        delivery.outcome.lock().clone().unwrap().unwrap_err(),
        "Pairing request delivery was cancelled"
    );
}

#[test]
fn expired_request_does_not_block_resend() {
    let mut manager = MeshManager::new("test-host".into(), 7685);
    manager.disk_persist = false;
    manager
        .pending_requests
        .insert("r1".into(), pending_request("r1", "peer-a", 1000));
    manager.cleanup_expired_requests_at(1000 + 301);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let fresh = pending_request("r2", "peer-a", now);
    assert!(
        manager.receive_pairing_request(fresh).is_ok(),
        "re-send after expiry must be accepted"
    );
}
