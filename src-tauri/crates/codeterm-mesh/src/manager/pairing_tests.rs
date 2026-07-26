use super::*;
use crate::types::{PairingRequest, PairingStatus};

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
