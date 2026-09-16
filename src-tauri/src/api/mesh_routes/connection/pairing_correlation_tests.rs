use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const PEER_ID: &str = "31c7cff6-4bd2-43b0-a5cd-b5db9bfbb673";

fn mesh_state() -> crate::MeshStateHandle {
    let mut manager = crate::mesh::MeshManager::new("test".to_string(), 7685);
    manager.disk_persist = false;
    manager.paired_peers.clear();
    manager.pending_requests.clear();
    manager.pairing_targets.clear();
    manager.pairing_deliveries.clear();
    manager.ident.identity.public_key = Some("initiator-key".to_string());
    manager.discovered_peers.insert(
        PEER_ID.to_string(),
        crate::mesh::types::DiscoveredPeer {
            instance_id: PEER_ID.to_string(),
            instance_name: "peer".to_string(),
            addresses: vec![crate::mesh::types::PeerAddress::from_ip("127.0.0.1")],
            api_port: 7685,
            discovery_method: crate::mesh::types::DiscoveryMethod::Manual,
            last_seen: 0,
            dev: true,
            mesh_id: None,
            vouched_by: Vec::new(),
        },
    );
    Arc::new(parking_lot::Mutex::new(manager))
}

async fn retain_ambiguous(mesh: &crate::MeshStateHandle) -> String {
    let _ = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
        Err(ConnectPeerError::Delivery(
            "phase=response;source_depth=1".to_string(),
        ))
    })
    .await
    .expect_err("an ambiguous sender result is returned to the owner");
    mesh.lock()
        .pending_requests
        .keys()
        .next()
        .cloned()
        .expect("ambiguous request remains correlated")
}

fn paired_peer(public_key: &str) -> crate::mesh::types::PairedPeer {
    crate::mesh::types::PairedPeer {
        instance_id: PEER_ID.to_string(),
        instance_name: "peer".to_string(),
        addresses: vec![crate::mesh::types::PeerAddress::from_ip("127.0.0.1")],
        api_port: 7685,
        public_key: public_key.to_string(),
        paired_at: "now".to_string(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: crate::mesh::types::AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: 0,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[tokio::test]
async fn mesh_connect_peer_correlation_ambiguous_send_retains_identity_binding() {
    let mesh = mesh_state();
    let request_id = retain_ambiguous(&mesh).await;
    let manager = mesh.lock();
    let request = manager
        .pending_requests
        .get(&request_id)
        .expect("request remains pending");
    assert_eq!(request.public_key, "initiator-key");
    assert_eq!(manager.pairing_targets[&request_id].2, PEER_ID);
    let delivery = manager
        .pairing_deliveries
        .get(PEER_ID)
        .expect("callback delivery remains correlated");
    assert_eq!(delivery.request_id(), request_id);
    assert_eq!(delivery.target_id(), PEER_ID);
    assert!(delivery.is_retained());
}

#[tokio::test]
async fn mesh_connect_peer_correlation_late_approval_completes_without_resend() {
    let mesh = mesh_state();
    let sends = Arc::new(AtomicUsize::new(0));
    let initial_sends = sends.clone();
    let _ = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), move |_, _, _| {
        initial_sends.fetch_add(1, Ordering::SeqCst);
        async {
            Err(ConnectPeerError::Delivery(
                "phase=response;source_depth=1".to_string(),
            ))
        }
    })
    .await
    .expect_err("the sender did not receive a usable response");
    let request_id = mesh
        .lock()
        .pending_requests
        .keys()
        .next()
        .cloned()
        .expect("retained request id");
    assert!(!request_id.is_empty());

    {
        let mut manager = mesh.lock();
        let wrong_id = "01d1dc9d-fcf2-49a5-8a02-0a1fc681b29d";
        assert!(manager
            .validate_pairing_accept_identity(&request_id, wrong_id)
            .is_err());
        manager
            .validate_pairing_accept_identity(&request_id, PEER_ID)
            .expect("exact retained target identity is accepted");
        manager
            .complete_pairing_accept(&request_id, paired_peer("peer-key"))
            .expect("late approval releases the retained callback");
    }

    let result = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
        panic!("late approval must not resend the pair request")
    })
    .await
    .expect("late approval paired the peer");
    assert_eq!(result, ConnectPeerResult::Connected);
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(mesh.lock().paired_peers[PEER_ID].public_key, "peer-key");
}

#[tokio::test]
async fn mesh_connect_peer_correlation_definitely_before_send_cleans_state() {
    let mesh = mesh_state();
    let error = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
        Err(ConnectPeerError::BeforeSend(
            "phase=connection_establishment;source_depth=1".to_string(),
        ))
    })
    .await
    .expect_err("before-send failure is returned");
    assert!(error.to_string().contains("connection_establishment"));
    assert!(mesh.lock().pending_requests.is_empty());
    assert!(mesh.lock().pairing_targets.is_empty());
    assert!(mesh.lock().pairing_deliveries.is_empty());

    let retry = connect_peer_with_sender(mesh, PEER_ID.to_string(), |_, _, _| async {
        Ok(PEER_ID.to_string())
    })
    .await
    .expect("cleanup allows a fresh request");
    assert!(matches!(
        retry,
        ConnectPeerResult::Pending { existing: false, .. }
    ));
}

#[tokio::test]
async fn mesh_connect_peer_correlation_retained_duplicates_share_request() {
    let mesh = mesh_state();
    let request_id = retain_ambiguous(&mesh).await;
    let sends = Arc::new(AtomicUsize::new(0));
    let make_duplicate = |mesh: crate::MeshStateHandle, sends: Arc<AtomicUsize>| async move {
        connect_peer_with_sender(mesh, PEER_ID.to_string(), move |_, _, _| {
            sends.fetch_add(1, Ordering::SeqCst);
            async { panic!("retained request must not resend") }
        })
        .await
    };
    let (one, two, three) = tokio::join!(
        make_duplicate(mesh.clone(), sends.clone()),
        make_duplicate(mesh.clone(), sends.clone()),
        make_duplicate(mesh.clone(), sends.clone()),
    );
    for result in [one, two, three] {
        assert_eq!(
            result.expect("retained duplicate observes the request"),
            ConnectPeerResult::Pending {
                request_id: request_id.clone(),
                existing: true,
            }
        );
    }
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(mesh.lock().pending_requests.len(), 1);
    assert_eq!(mesh.lock().pairing_deliveries.len(), 1);
    assert!(mesh.lock().cancel_outgoing_pairing(&request_id));
    assert!(mesh.lock().pending_requests.is_empty());
    assert!(mesh.lock().pairing_targets.is_empty());
    assert!(mesh.lock().pairing_deliveries.is_empty());
}

#[tokio::test]
async fn mesh_connect_peer_correlation_terminal_refusal_cleans_state() {
    let mesh = mesh_state();
    let error = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
        Err(ConnectPeerError::Refused(
            "Pair request rejected with HTTP 403".to_string(),
        ))
    })
    .await
    .expect_err("terminal peer refusal is returned");
    assert!(error.to_string().contains("HTTP 403"));
    assert!(mesh.lock().pending_requests.is_empty());
    assert!(mesh.lock().pairing_targets.is_empty());
    assert!(mesh.lock().pairing_deliveries.is_empty());
}

#[tokio::test]
async fn mesh_connect_peer_correlation_retained_expiry_and_capacity_are_bounded() {
    let mesh = mesh_state();
    let request_id = retain_ambiguous(&mesh).await;
    let expires_at = mesh.lock().pending_requests[&request_id].timestamp + 301;
    mesh.lock().cleanup_expired_requests_at(expires_at);
    assert!(mesh.lock().pending_requests.is_empty());
    assert!(mesh.lock().pairing_targets.is_empty());
    assert!(mesh.lock().pairing_deliveries.is_empty());
    assert!(mesh
        .lock()
        .validate_pairing_accept_identity(&request_id, PEER_ID)
        .is_err());

    {
        let mut manager = mesh.lock();
        for index in 0..crate::mesh::MAX_PAIRING_DELIVERIES {
            let target_id = format!("retained-{index}");
            manager.pairing_deliveries.insert(
                target_id.clone(),
                Arc::new(crate::mesh::PairingDelivery::new(
                    target_id,
                    format!("request-{index}"),
                )),
            );
            manager.pairing_targets.insert(
                format!("request-{index}"),
                ("127.0.0.1".to_string(), 7685, format!("retained-{index}")),
            );
        }
        assert!(matches!(
            prepare_connect(&mut manager, PEER_ID),
            Err(ConnectPeerError::Conflict(_))
        ));
        manager.pairing_deliveries.remove("retained-0");
        manager.pairing_targets.remove("request-0");
        assert!(matches!(
            prepare_connect(&mut manager, PEER_ID),
            Ok(ConnectPreparation::Send { .. })
        ));
    }
}
