use super::*;
use crate::account_mesh::protocol::{LINK_COMMIT_PATH, LINK_CONFIRM_PATH};

fn linked_pair() -> (Machine, Machine) {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    (a, b)
}

#[tokio::test]
async fn account_mesh_lost_confirm_reply_leaves_no_pair_on_the_responder() {
    let (a, b) = linked_pair();
    let transport = Loopback::new(&[&a, &b]).lose_reply_of(LINK_CONFIRM_PATH);

    let lost = link_installation_with(&a.mesh, &transport, &b.installation_id).await;
    assert_eq!(lost.unwrap_err(), AdmissionRefusal::PeerUnreachable);
    {
        let manager = b.mesh.lock();
        assert!(
            manager.paired_peers.is_empty(),
            "nothing recorded before the commit"
        );
        assert!(manager.link_record(&a.id()).is_none());
        assert!(!manager.traffic_open(&a.id()));
    }
    assert!(a.mesh.lock().paired_peers.is_empty());

    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .expect("a fresh Add completes after the lost attempt");
    assert!(b.mesh.lock().traffic_open(&a.id()));
    assert_eq!(
        transport.posts.load(Ordering::SeqCst),
        3,
        "hello, confirm, commit"
    );
}

#[tokio::test]
async fn account_mesh_lost_commit_reply_is_repaired_by_the_next_add() {
    let (a, b) = linked_pair();
    let lossy = Loopback::new(&[&a, &b]).lose_reply_of(LINK_COMMIT_PATH);
    assert!(link_installation_with(&a.mesh, &lossy, &b.installation_id)
        .await
        .is_err());
    assert!(a.mesh.lock().paired_peers.is_empty());

    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .expect("re-Add is idempotent on the side that already committed");
    assert!(a.mesh.lock().traffic_open(&b.id()));
    assert_eq!(b.mesh.lock().paired_peers.len(), 1);
}

#[test]
fn account_mesh_membership_tolerates_issuer_skew_but_never_extends_expiry() {
    use crate::account_mesh::NOT_BEFORE_SKEW_SECONDS;
    assert_eq!(
        NOT_BEFORE_SKEW_SECONDS, 120,
        "must equal the account verifier's tolerance"
    );
    let mut issued_ahead = membership(ACCOUNT, "inst-9", START + 600).verified;
    issued_ahead.issued_at = START + NOT_BEFORE_SKEW_SECONDS;
    assert!(issued_ahead.is_current(START));
    issued_ahead.issued_at = START + NOT_BEFORE_SKEW_SECONDS + 1;
    assert!(!issued_ahead.is_current(START));
    issued_ahead.issued_at = 0;
    assert!(!issued_ahead.is_current(START + 600), "no grace at expiry");
}

#[tokio::test]
async fn account_mesh_registry_revocation_closes_an_open_account_stream() {
    let (a, b) = linked_pair();
    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    assert!(a.mesh.lock().traffic_open(&b.id()));
    let blocked = crate::account_mesh::wait_until_blocked(a.mesh.clone(), b.id());
    tokio::pin!(blocked);
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(blocked.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let mut state = a.mesh.lock().account_state().clone();
    state
        .revoked_installations
        .insert(b.installation_id.clone());
    a.mesh.lock().apply_account_state(state);
    blocked.await;
    assert!(!a.mesh.lock().traffic_open(&b.id()));
    assert!(matches!(
        a.mesh.lock().link_record(&b.id()).unwrap().admission,
        crate::account_mesh::ledger::LinkAdmission::Account { .. }
    ));
}

#[tokio::test]
async fn account_mesh_signed_out_peer_is_unavailable_and_returns_on_the_same_account_edge() {
    use crate::account_mesh::{
        gate::{BlockReason, TrafficDecision},
        AccountMeshAuthority,
    };
    let (a, b) = linked_pair();
    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    let original = serde_json::to_value(a.mesh.lock().link_record(&b.id()).unwrap()).unwrap();
    let token = a
        .mesh
        .lock()
        .link_record(&b.id())
        .unwrap()
        .remote_membership
        .as_ref()
        .unwrap()
        .token
        .clone();
    let mut state = a.mesh.lock().account_state().clone();
    state
        .unavailable_installations
        .insert(b.installation_id.clone());
    a.mesh.lock().apply_account_state(state.clone());
    assert_eq!(
        a.mesh.lock().traffic_decision(&b.id()),
        TrafficDecision::Blocked(BlockReason::PeerUnavailable)
    );
    assert!(a.authority.verify_membership(&token, START).is_ok());
    assert!(a
        .mesh
        .lock()
        .account_state()
        .revoked_installations
        .is_empty());
    assert!(!a.mesh.lock().account_mesh_pairs()[0].online);
    state.unavailable_installations.clear();
    a.mesh.lock().apply_account_state(state);
    assert!(a.mesh.lock().traffic_open(&b.id()));
    assert_eq!(
        serde_json::to_value(a.mesh.lock().link_record(&b.id()).unwrap()).unwrap(),
        original
    );
    assert_eq!(a.mesh.lock().paired_peers.len(), 1);
    assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 1);
}
