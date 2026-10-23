use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::support::*;
use crate::account_mesh::link::{adopt_peer_with, link_installation_with, LinkTransport};
use crate::account_mesh::protocol::{LINK_COMMIT_PATH, LINK_CONFIRM_PATH};
use crate::account_mesh::{AdmissionRefusal, BoxFuture};

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
    b.mesh.lock().unpair(&a.id()).unwrap();

    let refused = link_installation_with(&a.mesh, &transport, &b.installation_id).await;
    assert!(matches!(
        refused.unwrap_err(),
        AdmissionRefusal::HandshakeFailed { .. }
    ));
    {
        let manager = b.mesh.lock();
        assert!(
            manager.is_revoked(&a.id()),
            "the removal survives a remote re-link"
        );
        assert!(!manager.paired_peers.contains_key(&a.id()));
    }

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

struct HeldCommitReply {
    inner: Loopback,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl LinkTransport for HeldCommitReply {
    fn post(
        &self,
        ip: &str,
        port: u16,
        path: &'static str,
        bearer: Option<String>,
        body: serde_json::Value,
    ) -> BoxFuture<'_, Result<serde_json::Value, AdmissionRefusal>> {
        let inner = &self.inner;
        let ip = ip.to_string();
        let entered = self.entered.clone();
        let release = self.release.clone();
        Box::pin(async move {
            let result = inner.post(&ip, port, path, bearer, body).await;
            if path == LINK_COMMIT_PATH {
                entered.notify_one();
                release.notified().await;
            }
            result
        })
    }
}

#[tokio::test]
async fn account_mesh_add_started_before_unpair_cannot_clear_its_removal() {
    let (a, b) = linked_pair();
    a.mesh.lock().insert_legacy_pair_for_test(b.as_peer());
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let transport = HeldCommitReply {
        inner: Loopback::new(&[&a, &b]),
        entered: entered.clone(),
        release: release.clone(),
    };
    let mesh = a.mesh.clone();
    let installation_id = b.installation_id.clone();
    let add =
        tokio::spawn(
            async move { link_installation_with(&mesh, &transport, &installation_id).await },
        );

    entered.notified().await;
    assert!(a.mesh.lock().unpair(&b.id()).unwrap().is_some());
    release.notify_one();

    assert_eq!(add.await.unwrap().unwrap_err(), AdmissionRefusal::Cancelled);
    let manager = a.mesh.lock();
    assert!(manager.is_revoked(&b.id()));
    assert!(!manager.paired_peers.contains_key(&b.id()));
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
