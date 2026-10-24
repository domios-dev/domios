use super::support::*;
use crate::account_mesh::link::link_installation_with;

async fn linked() -> (Machine, Machine) {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    link_installation_with(&a.mesh, &Loopback::new(&[&a, &b]), &b.installation_id).await.unwrap();
    (a, b)
}

#[tokio::test]
async fn account_mesh_disconnect_signed_notice_removes_both_sides() {
    let (a, b) = linked().await;
    a.mesh.lock().unpair(&b.id()).unwrap();
    let notice = a.mesh.lock().saved_disconnect(&b.id()).unwrap().notice.clone();
    assert!(b.mesh.lock().apply_disconnect_notice(&notice).unwrap());
    assert!(a.mesh.lock().paired_peers.is_empty());
    assert!(b.mesh.lock().paired_peers.is_empty());
    assert!(!b.mesh.lock().apply_disconnect_notice(&notice).unwrap());
}

#[tokio::test]
async fn account_mesh_disconnect_offline_peer_learns_removal_on_next_signed_ping() {
    let (a, b) = linked().await;
    a.mesh.lock().unpair(&b.id()).unwrap();
    let token = b.mesh.lock().sign_request().unwrap();
    let notice = a.mesh.lock().disconnect_notice_for(&token).unwrap();
    assert!(b.mesh.lock().paired_peers.contains_key(&a.id()));
    assert!(b.mesh.lock().apply_disconnect_notice(&notice).unwrap());
    assert!(b.mesh.lock().paired_peers.is_empty());
    let serialized = serde_json::to_value(a.mesh.lock().saved_disconnect(&b.id()).unwrap()).unwrap();
    assert!(serialized.get("notice").is_some(), "removal proof can survive daemon restart");
}

#[tokio::test]
async fn account_mesh_disconnect_old_or_forged_notice_cannot_remove_a_new_link() {
    let (a, b) = linked().await;
    a.mesh.lock().unpair(&b.id()).unwrap();
    let notice = a.mesh.lock().saved_disconnect(&b.id()).unwrap().notice.clone();
    let mut forged = notice.clone();
    forged.signature = "forged".into();
    assert!(b.mesh.lock().apply_disconnect_notice(&forged).is_err());
    b.mesh.lock().apply_disconnect_notice(&notice).unwrap();
    link_installation_with(&b.mesh, &Loopback::new(&[&a, &b]), &a.installation_id).await.unwrap();
    assert!(b.mesh.lock().apply_disconnect_notice(&notice).is_err());
    assert!(b.mesh.lock().paired_peers.contains_key(&a.id()));
}
