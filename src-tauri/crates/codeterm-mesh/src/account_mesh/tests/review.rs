use std::sync::atomic::Ordering;

use super::support::*;
use crate::account_mesh::link::{adopt_peer_with, link_installation_with};
use crate::account_mesh::protocol::{LINK_COMMIT_PATH, LINK_CONFIRM_PATH};
use crate::account_mesh::AdmissionRefusal;

fn linked_pair() -> (Machine, Machine) {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    (a, b)
}

#[tokio::test]
async fn account_mesh_unpaired_peer_cannot_relink_itself_from_the_other_side() {
    let (a, b) = linked_pair();
    let transport = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    b.mesh.lock().unpair(&a.id());

    let refused = link_installation_with(&a.mesh, &transport, &b.installation_id).await;
    assert!(matches!(
        refused.unwrap_err(),
        AdmissionRefusal::HandshakeFailed { .. }
    ));
    let manager = b.mesh.lock();
    assert!(
        manager.is_revoked(&a.id()),
        "the removal survives a remote re-link"
    );
    assert!(!manager.paired_peers.contains_key(&a.id()));
    drop(manager);

    link_installation_with(&b.mesh, &transport, &a.installation_id)
        .await
        .expect("a local Add on the removing machine may link again");
    let manager = b.mesh.lock();
    assert!(!manager.is_revoked(&a.id()));
    assert!(manager.traffic_open(&a.id()));
}

#[tokio::test]
async fn account_mesh_lost_confirm_reply_leaves_no_pair_on_the_responder() {
    let (a, b) = linked_pair();
    let transport = Loopback::new(&[&a, &b]).lose_reply_of(LINK_CONFIRM_PATH);

    let lost = link_installation_with(&a.mesh, &transport, &b.installation_id).await;
    assert_eq!(lost.unwrap_err(), AdmissionRefusal::PeerUnreachable);
    let manager = b.mesh.lock();
    assert!(
        manager.paired_peers.is_empty(),
        "nothing recorded before the commit"
    );
    assert!(manager.link_record(&a.id()).is_none());
    assert!(!manager.traffic_open(&a.id()));
    drop(manager);
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

#[tokio::test]
async fn account_mesh_refused_adoption_is_not_offered_again_until_the_peer_changes() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    a.mesh.lock().insert_legacy_pair_for_test(b.as_peer());
    b.mesh.lock().insert_legacy_pair_for_test(a.as_peer());
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);

    assert!(adopt_peer_with(&a.mesh, &transport, &b.id()).await.is_err());
    assert_eq!(transport.posts.load(Ordering::SeqCst), 1);
    adopt_peer_with(&a.mesh, &transport, &b.id()).await.unwrap();
    assert_eq!(
        transport.posts.load(Ordering::SeqCst),
        1,
        "a refusing peer is not offered again on the next trigger"
    );

    a.mesh
        .lock()
        .note_advertised_membership(&b.id(), "renewed-membership");
    let _ = adopt_peer_with(&a.mesh, &transport, &b.id()).await;
    assert_eq!(transport.posts.load(Ordering::SeqCst), 2);
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
