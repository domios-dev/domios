use super::support::*;
use crate::account_mesh::wire::ProofEndpoint;
use crate::account_mesh::{AccountMeshRefusal, EnrollTrigger, InstallationKey, ProofContext};

#[tokio::test]
async fn entitlements_proof_reuses_persisted_key_and_generation_across_restart() {
    let mut harness = Harness::new().await;
    let id = crate::InstallationId::parse(INSTALLATION.into()).unwrap();
    harness.sign_in("user-1").await;
    let proof = harness
        .mesh
        .entitlements_proof(&session("user-1"), &id)
        .await
        .unwrap();
    let secret = harness.keys.0.lock().unwrap().clone().unwrap();
    let key = InstallationKey::from_secret(&secret).unwrap();
    let context = ProofContext {
        project: PROJECT.into(),
        mode: "test",
        uid: "user-1".into(),
        installation_id: INSTALLATION.into(),
        policy_epoch: EPOCH.into(),
        method: "POST",
        endpoint: ProofEndpoint::GetEntitlements,
        body_sha256: crate::account_mesh::key::jcs_sha256_hex(
            &serde_json::json!({"installation_id": INSTALLATION}),
        )
        .unwrap(),
    };
    assert_eq!(
        proof,
        key.prove(&context, 1, START, proof.nonce.clone()).unwrap()
    );
    assert_eq!(harness.calls("enroll_key"), 1);
    harness.restart().await;
    harness.sign_in("user-1").await;
    let restored = harness
        .mesh
        .entitlements_proof(&session("user-1"), &id)
        .await
        .unwrap();
    assert_eq!(
        (restored.key_id, restored.generation),
        (proof.key_id, proof.generation)
    );
    assert_eq!(harness.calls("enroll_key"), 1);
}

#[tokio::test]
async fn entitlements_proof_refuses_foreign_installation_account_and_session_generation() {
    let harness = Harness::new().await;
    harness.sign_in("user-1").await;
    let id = crate::InstallationId::parse(INSTALLATION.into()).unwrap();
    let foreign = crate::InstallationId::parse("foreign".into()).unwrap();
    assert_eq!(
        harness
            .mesh
            .entitlements_proof(&session("user-1"), &foreign)
            .await,
        Err(AccountMeshRefusal::Invalid)
    );
    assert_eq!(
        harness
            .mesh
            .entitlements_proof(&session("user-2"), &id)
            .await,
        Err(AccountMeshRefusal::Cancelled)
    );
    let mut old = session("user-1");
    old.login_generation += 1;
    assert_eq!(
        harness.mesh.entitlements_proof(&old, &id).await,
        Err(AccountMeshRefusal::Cancelled)
    );
    assert_eq!(harness.calls("enroll_key"), 0);
}

#[tokio::test]
async fn account_switch_restores_mesh_and_entitlement_proofs_when_challenge_issuer_is_ahead() {
    let harness = Harness::new().await;
    let id = crate::InstallationId::parse(INSTALLATION.into()).unwrap();
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .challenge_issuer_ahead = 2;
    for (index, uid) in ["user-1", "user-2", "user-1"].into_iter().enumerate() {
        if index != 0 {
            harness.logout().await;
        }
        harness.sign_in(uid).await;
        let mut session = session(uid);
        session.login_generation += index as u64;
        *harness.sessions.0.lock().unwrap() = Some(session.clone());
        let proof = harness
            .mesh
            .entitlements_proof(&session, &id)
            .await
            .unwrap();
        assert_eq!(proof.timestamp, START + 2);
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        let state = harness.state();
        assert_eq!(
            state.current_membership(START).unwrap().verified.account_id,
            crate::account_mesh::membership::account_id(uid)
        );
        assert_eq!(
            state.self_installation.as_ref().unwrap().key_id,
            proof.key_id
        );
    }
    assert_eq!(harness.calls("enroll_key"), 2);
}
