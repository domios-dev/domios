use super::support::*;
use crate::account_mesh::*;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn endpoint_gossip_publishes_only_for_cold_start_and_never_refreshes_on_expiry() {
    let harness = Harness::new().await;
    harness.registry.script.lock().unwrap().membership_seconds = Some(86400);
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let signed = harness.mesh.signed(None).await.unwrap();
    harness.log.lock().unwrap().clear();
    harness
        .transport
        .peers_reachable
        .store(true, Ordering::SeqCst);
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.reconcile_publication(&signed).await.unwrap();
    assert_eq!(harness.calls("publish"), 0);
    let sequence = harness.store.record.lock().unwrap().endpoint_sequence;
    assert!(sequence > 0);
    assert_eq!(
        harness
            .transport
            .endpoints
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1,
        sequence
    );
    harness
        .transport
        .peers_reachable
        .store(false, Ordering::SeqCst);
    harness.mesh.reconcile_publication(&signed).await.unwrap();
    assert_eq!(harness.calls("publish"), 1);
    harness.log.lock().unwrap().clear();
    harness.clock.set(START + 3600);
    harness.mesh.reconcile_publication(&signed).await.unwrap();
    assert_eq!(harness.calls("publish"), 0);
    *harness.endpoint.0.lock().unwrap() = None;
    harness.mesh.reconcile_publication(&signed).await.unwrap();
    assert_eq!(harness.calls("withdraw"), 0);
    assert_eq!(
        harness
            .transport
            .endpoints
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .0,
        None
    );
    assert!(harness.store.record.lock().unwrap().endpoint_sequence > sequence);
}

#[tokio::test]
async fn endpoint_gossip_sender_sequence_survives_restart_and_failed_storage_stops_signing() {
    let mut harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let before = harness.store.record.lock().unwrap().endpoint_sequence;
    harness.restart().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert!(harness.store.record.lock().unwrap().endpoint_sequence > before);
    let signed = harness.mesh.signed(None).await.unwrap();
    let sent = harness.transport.endpoints.lock().unwrap().len();
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    *harness.store.fail_save.lock().unwrap() = true;
    assert!(harness.mesh.reconcile_publication(&signed).await.is_err());
    assert_eq!(harness.transport.endpoints.lock().unwrap().len(), sent);
}

#[tokio::test]
async fn endpoint_gossip_receiver_high_water_survives_restart_and_fences_equivocation() {
    let mut harness = Harness::new().await;
    harness.sign_in("owner").await;
    let account = account_id("owner");
    assert!(harness
        .mesh
        .accept_endpoint_sequence(&account, PEER, 7, [7; 32])
        .await
        .unwrap());
    assert!(!harness
        .mesh
        .accept_endpoint_sequence(&account, PEER, 6, [6; 32])
        .await
        .unwrap());
    harness.restart().await;
    harness.sign_in("owner").await;
    assert!(harness
        .mesh
        .accept_endpoint_sequence(&account, PEER, 7, [7; 32])
        .await
        .unwrap());
    assert!(!harness
        .mesh
        .accept_endpoint_sequence(&account, PEER, 7, [8; 32])
        .await
        .unwrap());
    assert!(!harness
        .mesh
        .accept_endpoint_sequence(&account, PEER, 1, [1; 32])
        .await
        .unwrap());
    *harness.store.fail_save.lock().unwrap() = true;
    assert!(harness
        .mesh
        .accept_endpoint_sequence(&account, PEER, 8, [8; 32])
        .await
        .is_err());
    assert!(harness
        .mesh
        .accept_endpoint_sequence("other-account", PEER, 99, [9; 32])
        .await
        .is_err());
}
