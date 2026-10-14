use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const PEER_ID: &str = "31c7cff6-4bd2-43b0-a5cd-b5db9bfbb673";
const PEER_NAME: &str = "peer";

#[test]
fn mesh_reject_http_route_returns_not_found_without_blocking_processed_ids() {
    let mut mesh = crate::mesh::MeshManager::new("receiver".into(), 7685);
    mesh.disk_persist = false;
    mesh.revocation_list.blocked.clear();
    mesh.pending_requests.insert(
        "accepted".into(),
        crate::mesh::types::PairingRequest {
            request_id: "accepted".into(),
            from_instance_id: "paired-peer".into(),
            from_instance_name: "paired-peer".into(),
            from_address: "100.64.0.2".into(),
            from_port: 7685,
            from_addresses: vec![crate::mesh::types::PeerAddress::from_ip("100.64.0.2")],
            public_key: "key".into(),
            timestamp: 0,
            status: crate::mesh::types::PairingStatus::Accepted,
            introduced_by: None,
        },
    );

    for request_id in ["never-existed", "accepted"] {
        let error = reject_pending_request_http(&mut mesh, request_id)
            .expect_err("only a pending request can be rejected");
        assert_eq!(error.0, StatusCode::NOT_FOUND);
    }

    assert!(mesh.revocation_list.blocked.is_empty());
    assert_eq!(
        mesh.pending_requests["accepted"].status,
        crate::mesh::types::PairingStatus::Accepted
    );
}

fn mesh_state() -> crate::MeshStateHandle {
    let mut manager = crate::mesh::MeshManager::new("test".to_string(), 7685);
    manager.ready_listeners = vec!["100.64.0.1:7685".parse().unwrap()];
    manager.disk_persist = false;
    manager.ident.identity.public_key = Some("key".to_string());
    manager.discovered_peers.insert(
        PEER_ID.to_string(),
        crate::mesh::types::DiscoveredPeer {
            instance_id: PEER_ID.to_string(),
            instance_name: PEER_NAME.to_string(),
            addresses: vec![crate::mesh::types::PeerAddress::from_ip("127.0.0.1")],
            api_port: 7685,
            discovery_method: crate::mesh::types::DiscoveryMethod::Manual,
            last_seen: 0,
            dev: true,
            mesh_id: None,
            vouched_by: vec![],
        },
    );
    Arc::new(parking_lot::Mutex::new(manager))
}

#[tokio::test]
async fn mesh_connect_peer_first_send_and_duplicate_return_the_same_pending_request() {
    let mesh = mesh_state();
    let first = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
        Ok(PEER_ID.to_string())
    })
    .await
    .unwrap();
    let ConnectPeerResult::Pending {
        request_id,
        existing,
    } = first
    else {
        panic!("pending result expected")
    };
    assert!(!existing);

    let duplicate = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
        panic!("duplicate must not send")
    })
    .await
    .unwrap();
    assert_eq!(
        duplicate,
        ConnectPeerResult::Pending {
            request_id: request_id.clone(),
            existing: true
        }
    );
    assert_eq!(mesh.lock().pending_requests.len(), 1);
}

async fn assert_aliases_share_one_delivery<M>(
    first_target: &str,
    duplicate_target: &str,
    mutate_while_waiting: M,
) where
    M: FnOnce(&crate::MeshStateHandle),
{
    let mesh = mesh_state();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let first_mesh = mesh.clone();
    let first_target = first_target.to_string();
    let first = tokio::spawn(async move {
        connect_peer_with_sender(first_mesh, first_target, |_, _, _| async move {
            started_tx.send(()).expect("first delivery starts");
            release_rx.await.expect("test releases first delivery");
            Ok(PEER_ID.to_string())
        })
        .await
    });
    started_rx.await.expect("first delivery starts");

    let duplicate_mesh = mesh.clone();
    let duplicate_target = duplicate_target.to_string();
    let (wait_tx, wait_rx) = tokio::sync::oneshot::channel();
    let duplicate = tokio::spawn(async move {
        connect_peer_with_sender_and_waiter(
            duplicate_mesh,
            duplicate_target,
            |_, _, _| async { panic!("alias duplicate must not send") },
            || wait_tx.send(()).expect("alias duplicate waits"),
        )
        .await
    });
    wait_rx.await.expect("alias duplicate waits");
    mutate_while_waiting(&mesh);
    release_tx.send(()).expect("release first delivery");

    let first = first.await.unwrap().unwrap();
    let duplicate = duplicate.await.unwrap().unwrap();
    let ConnectPeerResult::Pending {
        request_id,
        existing: false,
    } = first
    else {
        panic!("first request creates pending pairing")
    };
    assert_eq!(
        duplicate,
        ConnectPeerResult::Pending {
            request_id,
            existing: true,
        }
    );
}

#[tokio::test]
async fn mesh_connect_peer_name_then_uuid_share_the_same_delivery() {
    assert_aliases_share_one_delivery(PEER_NAME, PEER_ID, |_| {}).await;
}

#[tokio::test]
async fn mesh_connect_peer_uuid_then_name_share_the_same_delivery() {
    assert_aliases_share_one_delivery(PEER_ID, PEER_NAME, |_| {}).await;
}

#[tokio::test]
async fn mesh_connect_peer_waiter_survives_discovery_removal() {
    assert_aliases_share_one_delivery(PEER_ID, PEER_NAME, |mesh| {
        mesh.lock().discovered_peers.remove(PEER_ID);
    })
    .await;
}

#[tokio::test]
async fn mesh_connect_peer_waiter_survives_discovery_identity_rekey() {
    assert_aliases_share_one_delivery(PEER_ID, PEER_NAME, |mesh| {
        let mut manager = mesh.lock();
        let mut peer = manager
            .discovered_peers
            .remove(PEER_ID)
            .expect("test peer exists");
        peer.instance_id = "bffac0f8-3d21-4e8a-aa52-c59ef82eb80f".to_string();
        peer.instance_name = "renamed peer".to_string();
        manager
            .discovered_peers
            .insert(peer.instance_id.clone(), peer);
    })
    .await;
}

#[tokio::test]
async fn mesh_connect_peer_same_name_success_returns_existing_pending() {
    let mesh = mesh_state();
    let first = connect_peer_with_sender(mesh.clone(), PEER_NAME.to_string(), |_, _, _| async {
        Ok(PEER_ID.to_string())
    })
    .await
    .unwrap();
    let duplicate = connect_peer_with_sender(mesh, PEER_NAME.to_string(), |_, _, _| async {
        panic!("same-name duplicate must not send")
    })
    .await
    .unwrap();
    assert!(matches!(
        first,
        ConnectPeerResult::Pending {
            existing: false,
            ..
        }
    ));
    assert!(matches!(
        duplicate,
        ConnectPeerResult::Pending { existing: true, .. }
    ));
}

#[tokio::test]
async fn mesh_connect_peer_already_connected_returns_connected_without_sending() {
    let mesh = mesh_state();
    {
        let mut manager = mesh.lock();
        manager.paired_peers.insert(
            PEER_ID.to_string(),
            crate::mesh::types::PairedPeer {
                instance_id: PEER_ID.to_string(),
                instance_name: "peer".to_string(),
                addresses: vec![],
                api_port: 7685,
                public_key: "key".to_string(),
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
            },
        );
    }
    let result = connect_peer_with_sender(mesh, PEER_ID.to_string(), |_, _, _| async {
        panic!("paired peer must not send")
    })
    .await
    .unwrap();
    assert_eq!(result, ConnectPeerResult::Connected);
}

#[tokio::test]
async fn mesh_connect_peer_definitely_before_send_rolls_back_so_retry_can_send() {
    let mesh = mesh_state();
    let error = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
        Err(ConnectPeerError::BeforeSend("offline".to_string()))
    })
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "offline");
    assert!(mesh.lock().pending_requests.is_empty());
    assert!(mesh.lock().pairing_targets.is_empty());

    let retry = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), |_, _, _| async {
        Ok(PEER_ID.to_string())
    })
    .await
    .unwrap();
    assert!(matches!(
        retry,
        ConnectPeerResult::Pending {
            existing: false,
            ..
        }
    ));
}

#[tokio::test]
async fn mesh_connect_peer_duplicate_waits_for_before_send_failure_and_retry_is_fresh() {
    let mesh = mesh_state();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let first_mesh = mesh.clone();
    let first = tokio::spawn(async move {
        connect_peer_with_sender(first_mesh, PEER_ID.to_string(), |_, _, _| async move {
            started_tx
                .send(())
                .expect("test waits for the first delivery");
            release_rx.await.expect("test releases the first delivery");
            Err(ConnectPeerError::BeforeSend("offline".to_string()))
        })
        .await
    });
    started_rx.await.expect("first delivery starts");

    let duplicate_mesh = mesh.clone();
    let (duplicate_wait_tx, duplicate_wait_rx) = tokio::sync::oneshot::channel();
    let duplicate = tokio::spawn(async move {
        connect_peer_with_sender_and_waiter(
            duplicate_mesh,
            PEER_ID.to_string(),
            |_, _, _| async { panic!("duplicate must wait for the in-flight delivery") },
            || {
                duplicate_wait_tx
                    .send(())
                    .expect("duplicate starts waiting")
            },
        )
        .await
    });
    duplicate_wait_rx
        .await
        .expect("duplicate waits for first delivery");
    release_tx.send(()).expect("release first delivery");

    assert_eq!(first.await.unwrap().unwrap_err().to_string(), "offline");
    assert_eq!(duplicate.await.unwrap().unwrap_err().to_string(), "offline");
    assert!(mesh.lock().pending_requests.is_empty());
    assert!(mesh.lock().pairing_targets.is_empty());

    let sends = Arc::new(AtomicUsize::new(0));
    let retry_sends = sends.clone();
    let retry = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), move |_, _, _| {
        retry_sends.fetch_add(1, Ordering::SeqCst);
        async { Ok(PEER_ID.to_string()) }
    })
    .await
    .unwrap();
    assert!(matches!(
        retry,
        ConnectPeerResult::Pending {
            existing: false,
            ..
        }
    ));
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(mesh.lock().pending_requests.len(), 1);
}

#[tokio::test]
async fn mesh_connect_peer_duplicate_waits_for_identity_mismatch_and_retry_is_fresh() {
    let mesh = mesh_state();
    mesh.lock()
        .discovered_peers
        .get_mut(PEER_ID)
        .expect("test peer exists")
        .discovery_method = crate::mesh::types::DiscoveryMethod::Udp;
    let wrong_id = "01d1dc9d-fcf2-49a5-8a02-0a1fc681b29d".to_string();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let first_mesh = mesh.clone();
    let first = tokio::spawn(async move {
        connect_peer_with_sender(first_mesh, PEER_ID.to_string(), move |_, _, _| async move {
            started_tx
                .send(())
                .expect("test waits for the first delivery");
            release_rx.await.expect("test releases the first delivery");
            Ok(wrong_id)
        })
        .await
    });
    started_rx.await.expect("first delivery starts");

    let duplicate_mesh = mesh.clone();
    let (duplicate_wait_tx, duplicate_wait_rx) = tokio::sync::oneshot::channel();
    let duplicate = tokio::spawn(async move {
        connect_peer_with_sender_and_waiter(
            duplicate_mesh,
            PEER_ID.to_string(),
            |_, _, _| async { panic!("duplicate must wait for identity confirmation") },
            || {
                duplicate_wait_tx
                    .send(())
                    .expect("duplicate starts waiting")
            },
        )
        .await
    });
    duplicate_wait_rx
        .await
        .expect("duplicate waits for identity confirmation");
    release_tx.send(()).expect("release first delivery");

    let expected = format!(
        "Peer identity mismatch: expected {PEER_ID}, got 01d1dc9d-fcf2-49a5-8a02-0a1fc681b29d"
    );
    assert_eq!(first.await.unwrap().unwrap_err().to_string(), expected);
    assert_eq!(duplicate.await.unwrap().unwrap_err().to_string(), expected);
    assert!(mesh.lock().pending_requests.is_empty());
    assert!(mesh.lock().pairing_targets.is_empty());

    let sends = Arc::new(AtomicUsize::new(0));
    let retry_sends = sends.clone();
    let retry = connect_peer_with_sender(mesh.clone(), PEER_ID.to_string(), move |_, _, _| {
        retry_sends.fetch_add(1, Ordering::SeqCst);
        async { Ok(PEER_ID.to_string()) }
    })
    .await
    .unwrap();
    assert!(matches!(
        retry,
        ConnectPeerResult::Pending {
            existing: false,
            ..
        }
    ));
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(mesh.lock().pending_requests.len(), 1);
}

#[tokio::test]
async fn mesh_connect_peer_owner_cancellation_wakes_waiter_and_allows_one_fresh_retry() {
    let mesh = mesh_state();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let first_mesh = mesh.clone();
    let owner = tokio::spawn(async move {
        connect_peer_with_sender(first_mesh, PEER_NAME.to_string(), |_, _, _| async move {
            started_tx.send(()).expect("owner delivery starts");
            std::future::pending::<Result<String, ConnectPeerError>>().await
        })
        .await
    });
    started_rx.await.expect("owner delivery starts");

    let duplicate_mesh = mesh.clone();
    let (wait_tx, wait_rx) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn(async move {
        connect_peer_with_sender_and_waiter(
            duplicate_mesh,
            PEER_ID.to_string(),
            |_, _, _| async { panic!("waiter must not send") },
            || wait_tx.send(()).expect("waiter starts waiting"),
        )
        .await
    });
    wait_rx.await.expect("waiter starts waiting");
    owner.abort();
    assert!(owner.await.expect_err("owner was aborted").is_cancelled());
    assert!(matches!(
        waiter.await.unwrap().unwrap(),
        ConnectPeerResult::Pending { existing: true, .. }
    ));
    let request_id = mesh
        .lock()
        .pending_requests
        .keys()
        .next()
        .cloned()
        .expect("cancellation retains correlation until disposition");
    assert!(mesh.lock().cancel_outgoing_pairing(&request_id));
    assert!(mesh.lock().pairing_deliveries.is_empty());
    assert!(mesh.lock().pending_requests.is_empty());
    assert!(mesh.lock().pairing_targets.is_empty());

    let sends = Arc::new(AtomicUsize::new(0));
    let retry_sends = sends.clone();
    let retry = connect_peer_with_sender(mesh, PEER_NAME.to_string(), move |_, _, _| {
        retry_sends.fetch_add(1, Ordering::SeqCst);
        async { Ok(PEER_ID.to_string()) }
    })
    .await
    .unwrap();
    assert!(matches!(
        retry,
        ConnectPeerResult::Pending {
            existing: false,
            ..
        }
    ));
    assert_eq!(sends.load(Ordering::SeqCst), 1);
}

#[test]
fn mesh_connect_peer_rest_and_tauri_share_the_same_result_mapping() {
    let result = ConnectPeerResult::Pending {
        request_id: "request-1".to_string(),
        existing: true,
    };
    let rest = result.as_json();
    let tauri = result.as_json();
    assert_eq!(rest, tauri);
    assert_eq!(rest["status"], "pending");
    assert_eq!(rest["existing"], true);
}
