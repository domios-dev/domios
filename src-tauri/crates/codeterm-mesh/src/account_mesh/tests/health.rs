use super::*;
use crate::health::check_peer_with;
use crate::manager::ConnectionPhase;
use crate::transport::SyncFailure;
use crate::types::{MeshSyncPayload, PeerAddress};

fn payload() -> MeshSyncPayload {
    MeshSyncPayload {
        endpoints: Vec::new(),
        peers: Vec::new(),
        generation: 0,
    }
}

async fn connected() -> (Machine, Machine) {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    link_installation_with(&a.mesh, &Loopback::new(&[&a, &b]), &b.installation_id)
        .await
        .unwrap();
    (a, b)
}

#[tokio::test]
async fn health_public_ping_waits_for_authenticated_sync_before_restoring_online() {
    let (a, b) = connected().await;
    a.mesh.lock().paired_peers.get_mut(&b.id()).unwrap().online = false;
    let ping_mesh = a.mesh.clone();
    let sync_mesh = a.mesh.clone();
    let result = check_peer_with(
        &a.mesh,
        &b.id(),
        Some(START),
        move |_| async move {
            assert!(!ping_mesh.lock().account_mesh_pairs()[0].online);
            Ok(())
        },
        move |(), _| async move {
            assert!(!sync_mesh.lock().account_mesh_pairs()[0].online);
            Ok(payload())
        },
    )
    .await;
    assert_eq!(result, Some(true));
    assert!(a.mesh.lock().account_mesh_pairs()[0].online);
    assert_eq!(
        a.mesh.lock().connection_phases()[0].phase,
        ConnectionPhase::Ready
    );
}

#[tokio::test]
async fn health_every_failed_protected_sync_clears_usable_state() {
    for failure in [
        SyncFailure::Request,
        SyncFailure::InvalidReply,
        SyncFailure::Refused(401),
        SyncFailure::Refused(403),
        SyncFailure::Refused(426),
        SyncFailure::Refused(500),
    ] {
        let (a, b) = connected().await;
        let result = check_peer_with(
            &a.mesh,
            &b.id(),
            None,
            |_| async { Ok(()) },
            move |(), _| async move { Err(failure) },
        )
        .await;
        assert_eq!(result, Some(false));
        assert!(!a.mesh.lock().paired_peers[&b.id()].online);
        assert!(!a.mesh.lock().account_mesh_pairs()[0].online);
        assert_ne!(
            a.mesh.lock().connection_phases()[0].phase,
            ConnectionPhase::Ready
        );
    }
}

#[tokio::test]
async fn health_unreachable_ping_clears_usable_state_without_sync() {
    let (a, b) = connected().await;
    let result = check_peer_with(
        &a.mesh,
        &b.id(),
        None,
        |_| async { Err::<(), _>(()) },
        |(), _| async { panic!("An unreachable peer cannot be synced") },
    )
    .await;
    assert_eq!(result, Some(false));
    assert!(!a.mesh.lock().account_mesh_pairs()[0].online);
}

#[tokio::test]
async fn health_relink_during_production_ping_discards_old_endpoint_results() {
    for (old_result, new_refused) in [
        (Err(SyncFailure::Refused(401)), false),
        (Err(SyncFailure::Refused(426)), false),
        (Err(SyncFailure::Request), false),
        (Ok(payload()), true),
    ] {
        let (a, b) = connected().await;
        let old_ip = b.ip.clone();
        let expected_ip = old_ip.clone();
        let old_nonce = a
            .mesh
            .lock()
            .link_record(&b.id())
            .unwrap()
            .link_nonce
            .clone();
        let transport = Loopback::new(&[&a, &b]);
        let peer_id = b.id();
        let target_id = peer_id.clone();
        let a_ref = &a;
        let b_ref = &b;
        let result = check_peer_with(
            &a.mesh,
            &peer_id,
            None,
            |endpoint| async move {
                assert_eq!(endpoint.addresses[0].ip, old_ip);
                assert_eq!(endpoint.port, b_ref.port);
                assert!(!endpoint.token.is_empty());
                link_installation_with(&a_ref.mesh, &transport, &b_ref.installation_id)
                    .await
                    .unwrap();
                let mut manager = a_ref.mesh.lock();
                let nonce = manager.link_record(&target_id).unwrap().link_nonce.clone();
                assert_ne!(nonce, old_nonce);
                manager.paired_peers.get_mut(&target_id).unwrap().addresses =
                    vec![PeerAddress::from_ip("192.0.2.99")];
                if new_refused {
                    manager.record_peer_http_status(&target_id, nonce.as_deref(), 401);
                }
                Ok(endpoint.addresses[0].ip.clone())
            },
            move |client_ip, _| async move {
                assert_eq!(client_ip, expected_ip);
                old_result
            },
        )
        .await;
        assert_eq!(result, None);
        assert_eq!(a.mesh.lock().account_mesh_pairs()[0].online, !new_refused);
        assert_eq!(
            a.mesh.lock().connection_phases()[0].phase,
            if new_refused {
                ConnectionPhase::AuthRefused
            } else {
                ConnectionPhase::Ready
            }
        );
    }
}

#[tokio::test]
async fn health_account_epoch_change_during_ping_discards_old_success_with_same_nonce() {
    let (a, b) = connected().await;
    let original_nonce = a
        .mesh
        .lock()
        .link_record(&b.id())
        .unwrap()
        .link_nonce
        .clone();
    let result = check_peer_with(
        &a.mesh,
        &b.id(),
        None,
        |_| async {
            let mut manager = a.mesh.lock();
            let mut state = manager.account_state().clone();
            state.account_epoch += 1;
            manager.apply_account_state(state);
            assert_eq!(
                manager.link_record(&b.id()).unwrap().link_nonce,
                original_nonce
            );
            manager.record_peer_http_status(&b.id(), original_nonce.as_deref(), 401);
            Ok(())
        },
        |(), _| async { Ok(payload()) },
    )
    .await;
    assert_eq!(result, None);
    assert!(!a.mesh.lock().account_mesh_pairs()[0].online);
    assert_eq!(
        a.mesh.lock().connection_phases()[0].phase,
        ConnectionPhase::AuthRefused
    );
}
