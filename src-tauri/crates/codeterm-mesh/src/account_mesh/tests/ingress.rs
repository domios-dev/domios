use super::support::*;
use crate::account_mesh::link::link_installation_with;
use crate::cloud_ingress::admission::{EnrolledProof, IngressRole};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn enrolled_ingress_requires_both_proofs_on_the_same_live_channel() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    for m in [&a, &b] {
        m.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    }
    let binding = [17; 32];
    let target = b.mesh.lock().admission_context().unwrap().party;
    let proof = a
        .mesh
        .lock()
        .sign_ingress_proof(&target, IngressRole::Initiator, &binding)
        .unwrap();
    let verify = |proof: &EnrolledProof, binding: &[u8; 32], role| {
        b.mesh
            .lock()
            .verify_ingress_proof(&a.installation_id, role, binding, proof)
    };
    for field in ["mesh_signature", "installation_signature"] {
        let mut damaged = serde_json::to_value(&proof).unwrap();
        damaged[field] = "invalid".into();
        let damaged = serde_json::from_value(damaged).unwrap();
        assert!(verify(&damaged, &binding, IngressRole::Initiator).is_err());
    }
    assert!(verify(&proof, &[18; 32], IngressRole::Initiator).is_err());
    assert!(verify(&proof, &binding, IngressRole::Responder).is_err());
    let evidence = verify(&proof, &binding, IngressRole::Initiator).unwrap();
    assert!(evidence
        .peer_for_dispatch(&b.mesh.lock(), &binding)
        .is_err());
    let network = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &network, &b.installation_id)
        .await
        .unwrap();
    let evidence = verify(&proof, &binding, IngressRole::Initiator).unwrap();
    assert!(evidence.peer_for_dispatch(&b.mesh.lock(), &binding).is_ok());
    assert!(evidence
        .peer_for_dispatch(&b.mesh.lock(), &[18; 32])
        .is_err());
    let snapshot = codeterm_entitlements::DetachedGrant {
        payload: "registry".into(),
        signature: "signature".into(),
        key_id: "issuer".into(),
        alg: "Ed25519".into(),
    };
    assert!(evidence
        .receive_registry_snapshot(&b.mesh, &[18; 32], &snapshot)
        .await
        .is_err());
    assert!(b.authority.snapshots.lock().is_empty());
    assert_eq!(
        evidence
            .receive_registry_snapshot(&b.mesh, &binding, &snapshot)
            .await,
        Ok(true)
    );
    assert_eq!(b.authority.snapshots.lock().as_slice(), &[snapshot]);
    clock.store(START + MEMBERSHIP_TTL, Ordering::SeqCst);
    assert!(evidence
        .peer_for_dispatch(&b.mesh.lock(), &binding)
        .is_err());
}
