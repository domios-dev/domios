use super::*;
use codeterm_entitlements::DetachedGrant;

fn snapshot(revision: &str) -> DetachedGrant {
    DetachedGrant {
        payload: revision.into(),
        signature: "opaque-signature".into(),
        key_id: "issuer".into(),
        alg: "Ed25519".into(),
    }
}

#[tokio::test]
async fn every_handshake_exchanges_latest_envelopes_in_both_directions() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    a.mesh.lock().signed_registry = Some(snapshot("older"));
    b.mesh.lock().signed_registry = Some(snapshot("newer"));
    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    assert_eq!(
        *a.authority.snapshots.lock(),
        vec![snapshot("newer"), snapshot("newer")]
    );
    assert_eq!(
        *b.authority.snapshots.lock(),
        vec![snapshot("older"), snapshot("older")]
    );
    b.mesh.lock().signed_registry = Some(snapshot("after-offline"));
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    assert_eq!(
        *a.authority.snapshots.lock(),
        vec![
            snapshot("newer"),
            snapshot("newer"),
            snapshot("after-offline"),
            snapshot("after-offline")
        ]
    );
    assert_eq!(transport.posts.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn signed_registry_slot_change_closes_open_stream_and_blocks_old_membership() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    let mut blocked = Box::pin(super::super::wait_until_blocked(a.mesh.clone(), b.id()));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(blocked.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let mut state = a.mesh.lock().account_state().clone();
    state.registry_members = Some(
        [
            (
                a.installation_id.clone(),
                (format!("key-{}", a.installation_id), 1, 1),
            ),
            (
                b.installation_id.clone(),
                (format!("key-{}", b.installation_id), 1, 2),
            ),
        ]
        .into(),
    );
    a.mesh.lock().apply_account_state(state);
    blocked.await;
    assert_eq!(
        a.mesh.lock().traffic_decision(&b.id()),
        TrafficDecision::Blocked(BlockReason::Revoked)
    );
    assert!(a.mesh.lock().client_for(&b.id()).is_none());
    assert!(a.mesh.lock().sign_request_for(&b.id()).is_err());
    assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 0);
}
