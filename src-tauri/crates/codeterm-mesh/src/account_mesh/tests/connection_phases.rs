use super::*;
use crate::manager::ConnectionPhase;
use parking_lot::Mutex;

fn record(machine: &Machine) -> Arc<Mutex<Vec<ConnectionPhase>>> {
    let phases = Arc::new(Mutex::new(Vec::new()));
    let events = phases.clone();
    machine
        .mesh
        .lock()
        .bind_connection_phases(Arc::new(move |peer| {
            events.lock().push(peer.phase);
        }));
    phases
}

#[tokio::test]
async fn inventory_failures_preserve_health_without_relinks_or_cloud_permits() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    let phases = record(&a);
    let mut gate = a.mesh.lock().gate_watch();
    let nonce = a
        .mesh
        .lock()
        .link_record(&b.id())
        .unwrap()
        .link_nonce
        .clone();
    let mut relinks = 0;
    for attempt in 0..12 {
        clock.store(START + attempt * 61, Ordering::SeqCst);
        for status in [403, 500, 0] {
            a.mesh
                .lock()
                .record_peer_inventory_status(&b.id(), nonce.as_deref(), status);
            assert!(a.mesh.lock().account_mesh_pairs()[0].online);
            assert!(!gate.has_changed().unwrap());
            if !a.mesh.lock().has_healthy_connection(&b.installation_id) {
                relinks += 1;
                link_installation_with(&a.mesh, &transport, &b.installation_id)
                    .await
                    .unwrap();
            }
        }
        assert_eq!(
            crate::health::check_peer_with(
                &a.mesh,
                &b.id(),
                None,
                |_| async { Ok(()) },
                |(), payload| async { Ok(payload) },
            )
            .await,
            Some(true)
        );
        gate.borrow_and_update();
    }
    assert_eq!(relinks, 0);
    assert_eq!(transport.posts.load(Ordering::SeqCst), 3);
    assert!(phases.lock().is_empty());
    assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 0);
    assert_eq!(b.authority.authorize_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn inventory_refusals_keep_typed_suppression_without_liveness_events() {
    for (status, phase) in [
        (401, ConnectionPhase::AuthRefused),
        (426, ConnectionPhase::PeerNeedsUpdate),
    ] {
        let clock = clock();
        let (a, b) = (machine(1, &clock), machine(2, &clock));
        enroll_all(&[&a, &b]);
        a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
        b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
        link_installation_with(&a.mesh, &Loopback::new(&[&a, &b]), &b.installation_id)
            .await
            .unwrap();
        let phases = record(&a);
        let gate = a.mesh.lock().gate_watch();
        let nonce = a
            .mesh
            .lock()
            .link_record(&b.id())
            .unwrap()
            .link_nonce
            .clone();
        {
            let mut manager = a.mesh.lock();
            manager.record_peer_inventory_status(&b.id(), Some("stale-link"), status);
            assert!(manager.peer_requests_allowed(&b.id()));
            manager.record_peer_inventory_status(&b.id(), nonce.as_deref(), status);
            assert!(!manager.peer_requests_allowed(&b.id()));
            assert_eq!(manager.connection_phases()[0].phase, phase);
            assert!(manager.paired_peers[&b.id()].online);
            assert!(manager.has_healthy_connection(&b.installation_id));
        }
        clock.store(START + 60, Ordering::SeqCst);
        assert!(a.mesh.lock().peer_requests_allowed(&b.id()));
        assert!(a.mesh.lock().has_healthy_connection(&b.installation_id));
        assert!(!gate.has_changed().unwrap());
        assert!(phases.lock().is_empty());
        assert_eq!(
            crate::health::check_peer_with(
                &a.mesh,
                &b.id(),
                None,
                |_| async { Ok(()) },
                |(), payload| async { Ok(payload) },
            )
            .await,
            Some(true)
        );
        assert!(a.mesh.lock().account_mesh_pairs()[0].online);
        assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 0);
        assert_eq!(b.authority.authorize_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn connection_phases_refusal_blocks_requests_and_ping_cannot_restore_ready() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    let nonce = a
        .mesh
        .lock()
        .link_record(&b.id())
        .unwrap()
        .link_nonce
        .clone();
    a.mesh
        .lock()
        .record_peer_http_status(&b.id(), nonce.as_deref(), 401);
    {
        let mut manager = a.mesh.lock();
        manager.paired_peers.get_mut(&b.id()).unwrap().online = true;
        assert!(manager.sign_request_for(&b.id()).is_err());
        assert!(!manager.account_mesh_pairs()[0].online);
        assert_eq!(
            manager.connection_phases()[0].phase,
            ConnectionPhase::AuthRefused
        );
    }
    clock.store(START + 60, Ordering::SeqCst);
    assert!(a.mesh.lock().peer_requests_allowed(&b.id()));
    assert!(!a.mesh.lock().account_mesh_pairs()[0].online);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    a.mesh
        .lock()
        .record_peer_http_status(&b.id(), nonce.as_deref(), 401);
    assert!(
        a.mesh.lock().account_mesh_pairs()[0].online,
        "old responses cannot invalidate a new handshake"
    );
    assert_eq!(
        a.mesh.lock().connection_phases()[0].phase,
        ConnectionPhase::Ready
    );
    assert_eq!(
        a.mesh.lock().connection_deadline(),
        Some(START + MEMBERSHIP_TTL)
    );
    clock.store(START + MEMBERSHIP_TTL, Ordering::SeqCst);
    assert_eq!(
        a.mesh.lock().connection_phases()[0].phase,
        ConnectionPhase::MembershipExpired
    );
    assert!(!a.mesh.lock().account_mesh_pairs()[0].online);
}

#[tokio::test]
async fn connection_phases_publish_success_in_protocol_order_on_both_sides() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let (initiator, responder) = (record(&a), record(&b));
    link_installation_with(&a.mesh, &Loopback::new(&[&a, &b]), &b.installation_id)
        .await
        .unwrap();
    assert_eq!(
        *initiator.lock(),
        [
            ConnectionPhase::Discovering,
            ConnectionPhase::Connecting,
            ConnectionPhase::Verifying,
            ConnectionPhase::Committing,
            ConnectionPhase::Ready
        ]
    );
    assert_eq!(
        *responder.lock(),
        [
            ConnectionPhase::Verifying,
            ConnectionPhase::Committing,
            ConnectionPhase::Ready
        ]
    );
    assert!(decision(&a, &b).is_open());
    assert!(decision(&b, &a).is_open());
}

#[tokio::test]
async fn connection_phases_publish_failure_and_expire_abandoned_responder() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let (initiator, responder) = (record(&a), record(&b));
    let transport = Loopback::new(&[&a, &b]).lose_reply_of(super::super::protocol::LINK_HELLO_PATH);
    assert!(
        link_installation_with(&a.mesh, &transport, &b.installation_id)
            .await
            .is_err()
    );
    assert_eq!(
        *initiator.lock(),
        [
            ConnectionPhase::Discovering,
            ConnectionPhase::Connecting,
            ConnectionPhase::Failed
        ]
    );
    assert_eq!(*responder.lock(), [ConnectionPhase::Verifying]);
    let deadline = b.mesh.lock().connection_deadline().unwrap();
    clock.store(deadline, Ordering::SeqCst);
    b.mesh.lock().expire_pending_connections();
    assert_eq!(
        *responder.lock(),
        [ConnectionPhase::Verifying, ConnectionPhase::Failed]
    );
    assert!(a.mesh.lock().paired_peers.is_empty());
    assert!(b.mesh.lock().paired_peers.is_empty());
    assert_eq!(b.mesh.lock().connection_deadline(), None);
}

#[tokio::test]
async fn connection_phases_do_not_resurrect_after_account_change() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let mesh = a.mesh.clone();
    let transport = Loopback::new(&[&a, &b]).after_each(Arc::new(move |_| {
        let mut manager = mesh.lock();
        let mut state = manager.account_state().clone();
        state.account_epoch += 1;
        state.presence = super::super::AccountPresence::SignedOut;
        manager.apply_account_state(state);
    }));
    assert!(
        link_installation_with(&a.mesh, &transport, &b.installation_id)
            .await
            .is_err()
    );
    assert!(a.mesh.lock().connection_phases().is_empty());
}

#[tokio::test]
async fn connection_phases_publish_failed_identity_authorization_without_ready() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let phases = record(&a);
    let local = a.mesh.clone();
    let transport = Loopback::new(&[&a, &b]).after_each(Arc::new(move |_| {
        let mut state = local.lock().account_state().clone();
        state.registry_members = Some(Default::default());
        local.lock().apply_account_state(state);
    }));
    assert!(
        link_installation_with(&a.mesh, &transport, &b.installation_id)
            .await
            .is_err()
    );
    assert_eq!(
        *phases.lock(),
        [
            ConnectionPhase::Discovering,
            ConnectionPhase::Connecting,
            ConnectionPhase::Verifying,
            ConnectionPhase::Failed
        ]
    );
    assert!(a.mesh.lock().paired_peers.is_empty());
}
