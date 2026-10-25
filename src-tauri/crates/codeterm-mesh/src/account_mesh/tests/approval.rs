use std::sync::{atomic::Ordering, Arc};

use super::support::*;
use crate::account_mesh::approval::{LinkDecision, APPROVAL_TTL_SECS};
use crate::account_mesh::link::link_installation_with;
use crate::account_mesh::protocol::LinkConfirm;
use crate::account_mesh::AdmissionRefusal;

struct ForgedApproval(Loopback);

impl crate::account_mesh::link::LinkTransport for ForgedApproval {
    fn post(
        &self,
        ip: &str,
        port: u16,
        path: &'static str,
        bearer: Option<String>,
        mut body: serde_json::Value,
    ) -> crate::account_mesh::BoxFuture<'_, Result<serde_json::Value, AdmissionRefusal>> {
        if path == crate::account_mesh::protocol::LINK_APPROVAL_PATH {
            body["mesh_signature"] = serde_json::json!("forged");
        }
        self.0.post(ip, port, path, bearer, body)
    }
}

fn participants() -> (Machine, Machine) {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    (a, b)
}

async fn request(
    a: &Machine,
    b: &Machine,
) -> tokio::task::JoinHandle<Result<crate::account_mesh::protocol::AccountMeshPair, AdmissionRefusal>>
{
    let entered = Arc::new(tokio::sync::Notify::new());
    let transport = Loopback::new(&[a, b]).hold_approval(entered.clone());
    let mesh = a.mesh.clone();
    let installation = b.installation_id.clone();
    let task =
        tokio::spawn(async move { link_installation_with(&mesh, &transport, &installation).await });
    tokio::time::timeout(std::time::Duration::from_secs(1), entered.notified())
        .await
        .unwrap();
    task
}

#[tokio::test]
async fn account_mesh_approval_blocks_admission_and_both_permits_until_receiver_approves() {
    let (a, b) = participants();
    let task = request(&a, &b).await;
    let pending = b.mesh.lock().link_requests()[0].clone();
    assert!(pending.incoming);
    assert_eq!(pending.installation_id, a.installation_id);
    assert_eq!(pending.name, "machine-1");
    assert_eq!(pending.decision, LinkDecision::Waiting);
    assert!(a.mesh.lock().paired_peers.is_empty());
    assert!(b.mesh.lock().paired_peers.is_empty());
    assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 0);
    assert_eq!(b.authority.authorize_calls.load(Ordering::SeqCst), 0);
    let premature = LinkConfirm {
        link_id: pending.request_id.clone(),
        installation_signature: String::new(),
        mesh_signature: String::new(),
        permit: String::new(),
    };
    assert!(b.mesh.lock().begin_link_confirm(&premature).is_err());
    b.mesh
        .lock()
        .decide_link(&pending.request_id, true)
        .unwrap();
    assert!(task.await.unwrap().is_ok());
    assert!(a.mesh.lock().paired_peers.contains_key(&b.id()));
    assert!(b.mesh.lock().paired_peers.contains_key(&a.id()));
}

#[tokio::test]
async fn account_mesh_approval_decline_never_creates_a_pair() {
    let (a, b) = participants();
    let task = request(&a, &b).await;
    let id = b.mesh.lock().link_requests()[0].request_id.clone();
    assert_eq!(
        b.mesh.lock().decide_link(&id, false).unwrap(),
        LinkDecision::Declined
    );
    assert_eq!(
        b.mesh.lock().decide_link(&id, true).unwrap(),
        LinkDecision::Declined
    );
    assert_eq!(task.await.unwrap().unwrap_err(), AdmissionRefusal::Declined);
    assert!(a.mesh.lock().paired_peers.is_empty());
    assert!(b.mesh.lock().paired_peers.is_empty());
}

#[tokio::test]
async fn account_mesh_approval_expiry_refuses_late_approval() {
    let (a, b) = participants();
    let task = request(&a, &b).await;
    let id = b.mesh.lock().link_requests()[0].request_id.clone();
    b.mesh
        .lock()
        .set_clock(Arc::new(|| START + APPROVAL_TTL_SECS));
    assert_eq!(
        b.mesh.lock().decide_link(&id, true).unwrap_err(),
        AdmissionRefusal::Expired
    );
    assert_eq!(task.await.unwrap().unwrap_err(), AdmissionRefusal::Expired);
    assert!(a.mesh.lock().paired_peers.is_empty());
    assert!(b.mesh.lock().paired_peers.is_empty());
}

#[tokio::test]
async fn account_mesh_approval_duplicate_request_keeps_one_receiver_question() {
    let (a, b) = participants();
    let task = request(&a, &b).await;
    let transport = Loopback::new(&[&a, &b]);
    assert!(
        link_installation_with(&a.mesh, &transport, &b.installation_id)
            .await
            .is_err()
    );
    assert_eq!(b.mesh.lock().link_requests().len(), 1);
    let id = b.mesh.lock().link_requests()[0].request_id.clone();
    b.mesh.lock().decide_link(&id, false).unwrap();
    assert_eq!(task.await.unwrap().unwrap_err(), AdmissionRefusal::Declined);
}

#[tokio::test]
async fn account_mesh_approval_account_change_cancels_waiting_request() {
    let (a, b) = participants();
    let task = request(&a, &b).await;
    b.sign_out(2);
    assert_eq!(
        task.await.unwrap().unwrap_err(),
        AdmissionRefusal::SignedOut
    );
    assert!(b.mesh.lock().paired_peers.is_empty());
}

#[tokio::test]
async fn account_mesh_approval_requires_possession_before_asking_the_receiver() {
    let (a, b) = participants();
    let transport = ForgedApproval(Loopback::new(&[&a, &b]));
    assert!(
        link_installation_with(&a.mesh, &transport, &b.installation_id)
            .await
            .is_err()
    );
    assert!(b.mesh.lock().link_requests().is_empty());
    assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 0);
    assert_eq!(b.authority.authorize_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn account_mesh_approval_wait_has_its_own_window_before_permits() {
    let (a, b) = participants();
    let task = request(&a, &b).await;
    let id = b.mesh.lock().link_requests()[0].request_id.clone();
    b.mesh
        .lock()
        .set_clock(Arc::new(|| START + APPROVAL_TTL_SECS - 1));
    b.mesh.lock().decide_link(&id, true).unwrap();
    assert!(task.await.unwrap().is_ok());
}

#[tokio::test]
async fn account_mesh_approval_does_not_extend_the_confirmed_commit_window() {
    let (a, b) = participants();
    let responder = b.mesh.clone();
    let transport = Loopback::new(&[&a, &b]).after_each(Arc::new(move |path| {
        if path == crate::account_mesh::protocol::LINK_CONFIRM_PATH {
            responder.lock().set_clock(Arc::new(|| START + 61));
        }
    }));
    assert!(
        link_installation_with(&a.mesh, &transport, &b.installation_id)
            .await
            .is_err()
    );
    assert!(a.mesh.lock().paired_peers.is_empty());
    assert!(b.mesh.lock().paired_peers.is_empty());
}
