use super::support::*;
use crate::account_mesh::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;

const UID: &str = "owner";

#[tokio::test]
async fn account_mesh_rejoin_registry_advances_peer_key_without_downgrade() {
    let mut harness = Harness::new().await;
    harness.sign_in(UID).await;
    let old_key = InstallationKey::generate();
    let new_key = InstallationKey::generate();
    harness.registry.script.lock().unwrap().peer_key = Some((old_key.key_id(), 1));
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let membership = |key: &InstallationKey, generation| {
        let mut payload = Issuer::membership_payload(UID, &key.key_id(), generation, 1, START);
        payload["installation_id"] = json!(PEER);
        membership_token(&Issuer::new().sign(&payload))
    };
    harness.mesh.list().await.unwrap();
    harness
        .mesh
        .revoke(wire::MeshTarget {
            installation_id: PEER.into(),
            key_id: old_key.key_id(),
            key_generation: 1,
            slot_revision: 1,
        })
        .await
        .unwrap();
    harness.mesh.list().await.unwrap();
    assert!(harness.state().revoked_installations.contains(PEER));
    harness.registry.script.lock().unwrap().peer_active = Some(false);
    harness.mesh.list().await.unwrap();
    assert!(harness.state().revoked_installations.contains(PEER));
    harness.registry.script.lock().unwrap().peer_key = Some((new_key.key_id(), 2));
    harness.registry.script.lock().unwrap().peer_active = Some(true);
    harness.mesh.list().await.unwrap();
    assert!(!harness.state().revoked_installations.contains(PEER));
    assert!(!harness
        .transport
        .last
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .revoked_installations
        .contains(PEER));
    assert!(harness
        .mesh
        .verify_peer_membership(&membership(&new_key, 2), START)
        .is_ok());
    assert!(harness
        .mesh
        .verify_peer_membership(&membership(&old_key, 1), START)
        .is_err());
    assert!(harness
        .mesh
        .verify_peer_membership(&membership(&old_key, 2), START)
        .is_err());
    harness.registry.script.lock().unwrap().peer_key = Some((old_key.key_id(), 1));
    harness.mesh.list().await.unwrap();
    harness.restart().await;
    harness.sign_in(UID).await;
    assert!(!harness.state().revoked_installations.contains(PEER));
    assert!(harness
        .mesh
        .verify_peer_membership(&membership(&new_key, 2), START)
        .is_ok());
    assert!(harness
        .mesh
        .verify_peer_membership(&membership(&old_key, 1), START)
        .is_err());
}

#[tokio::test]
async fn account_mesh_rejoin_registry_reconciles_persisted_revocation_without_key_observation() {
    let mut harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness
        .mesh
        .revoke(row(PEER, 0, None).target())
        .await
        .unwrap();
    {
        let mut record = harness.store.record.lock().unwrap();
        record.peer_keys.clear();
        record.peer_presence.clear();
    }
    harness.restart().await;
    harness.sign_in(UID).await;
    assert!(harness.state().revoked_installations.contains(PEER));
    harness.registry.script.lock().unwrap().peer_active = Some(false);
    harness.mesh.list().await.unwrap();
    assert!(harness.state().revoked_installations.contains(PEER));
    harness.registry.script.lock().unwrap().peer_key = Some(("d".repeat(64), 2));
    harness.registry.script.lock().unwrap().peer_active = Some(true);
    harness.mesh.list().await.unwrap();
    assert!(!harness.state().revoked_installations.contains(PEER));
    assert!(!harness
        .store
        .record
        .lock()
        .unwrap()
        .revoked_installations
        .contains(PEER));
}

#[tokio::test]
async fn account_mesh_rejoin_in_process_refreshes_key_endpoint_and_revocation() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let old = harness.state().self_installation.clone().unwrap();
    let digest = [19; 32];
    let old_signature = harness.mesh.sign_possession(&digest).unwrap();
    harness
        .mesh
        .revoke(wire::MeshTarget {
            installation_id: old.installation_id.clone(),
            key_id: old.key_id.clone(),
            key_generation: old.key_generation,
            slot_revision: old.slot_revision,
        })
        .await
        .unwrap();
    harness.registry.revoke_self(UID);
    harness.logout().await;
    harness.sign_in(UID).await;
    harness.mesh.enrollment_signed_in();
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
    let state = harness.state();
    let own = state.self_installation.as_ref().unwrap();
    assert_eq!(own.key_generation, old.key_generation + 1);
    let signature = harness.mesh.sign_possession(&digest).unwrap();
    let registry_key = &state
        .registry
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .find(|row| row.installation_id == INSTALLATION)
        .unwrap()
        .key_id;
    assert!(verify_possession(registry_key, &digest, &signature));
    assert!(!verify_possession(registry_key, &digest, &old_signature));
    assert_eq!(state.publication, Publication::Published);
    assert!(harness.registry.endpoint(UID).is_some());
    assert!(!state.revoked_installations.contains(INSTALLATION));
    assert!(!harness
        .store
        .record
        .lock()
        .unwrap()
        .revoked_installations
        .contains(INSTALLATION));
}

fn auth_time(harness: &Harness, time: u64) {
    harness
        .sessions
        .0
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .id_token = crate::Secret::new(format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(json!({"auth_time": time}).to_string())
    ));
}

async fn removed() -> Harness {
    let mut harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.registry.revoke_self(UID);
    harness.restart().await;
    harness.sign_in(UID).await;
    auth_time(&harness, START);
    harness.log.lock().unwrap().clear();
    harness.mesh.enrollment_signed_in();
    harness
}

#[tokio::test]
async fn account_mesh_rejoin_rotates_then_enrolls_then_syncs() {
    let harness = removed().await;
    auth_time(&harness, START - 86400);
    let old = harness.store.record.lock().unwrap().keys[&account_id(UID)].clone();
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("recover"), 1);
    assert_eq!(harness.calls("sync"), 1);
    assert!(
        position(&harness.log, "registry:recover_key:")
            < position(&harness.log, "registry:recover:")
    );
    assert!(position(&harness.log, "registry:recover:") < position(&harness.log, "registry:sync:"));
    let state = harness.state();
    let own = state.self_installation.as_ref().unwrap();
    assert_eq!(own.installation_id, INSTALLATION);
    assert_ne!(own.key_id, old.key_id);
    assert_eq!(own.key_generation, old.generation + 1);
    assert_eq!(state.enrollment, Enrollment::Enrolled);
    assert!(state.current_membership(START).is_some());
    assert!(state.registry.as_ref().unwrap().rows[0].active);
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 1);
}

#[tokio::test]
async fn account_mesh_rejoin_old_auth_is_decided_by_server() {
    let harness = removed().await;
    auth_time(&harness, START - 86400);
    harness.registry.script.lock().unwrap().enrollment =
        Some(wire::RegistryRefusal::ReauthRequired);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Explicit).await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    assert_eq!(harness.calls("recover_key"), 1);
    harness.registry.script.lock().unwrap().enrollment = None;
    auth_time(&harness, START);
    harness.mesh.enrollment_signed_in();
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 2);
}

#[tokio::test]
async fn account_mesh_rejoin_failed_rotation_does_not_loop_in_a_sign_in_epoch() {
    let harness = removed().await;
    harness.registry.script.lock().unwrap().enrollment = Some(wire::RegistryRefusal::ProofInvalid);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Explicit).await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    harness.clock.set(START + 60);
    for trigger in [
        EnrollTrigger::Explicit,
        EnrollTrigger::Network,
        EnrollTrigger::Automatic,
    ] {
        let _ = harness.mesh.refresh(trigger).await;
    }
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("sync"), 0);
}

#[tokio::test]
async fn account_mesh_rejoin_same_generation_is_refused() {
    let harness = removed().await;
    let signed = harness
        .mesh
        .signed(Some(harness.state().account_epoch))
        .await
        .unwrap();
    assert_eq!(
        harness
            .mesh
            .admission(
                &signed,
                wire::MeshOperation::Enroll {
                    expected_slot_revision: 0,
                }
            )
            .await,
        Err(AccountMeshRefusal::Forbidden)
    );
    assert_eq!(harness.calls("recover_key"), 0);
    assert_eq!(harness.calls("sync"), 0);
    assert!(harness.registry.slot(UID).is_some());
}

#[tokio::test]
async fn account_mesh_rejoin_retries_sync_without_another_rotation() {
    let harness = removed().await;
    harness.registry.script.lock().unwrap().publication = Some(wire::RegistryRefusal::Offline);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Explicit).await,
        Err(AccountMeshRefusal::Offline)
    );
    harness.registry.script.lock().unwrap().publication = None;
    harness.clock.set(START + 30);
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("recover"), 1);
    assert_eq!(harness.calls("sync"), 2);
}

#[tokio::test]
async fn account_mesh_fresh_signin_observes_current_short_lease_once_without_rotation() {
    for offline in [false, true] {
        let harness = Harness::new().await;
        harness.sign_in(UID).await;
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        let membership = harness.state().membership.clone().unwrap();
        let lists = harness.calls("list");
        harness.mesh.enrollment_signed_in();
        if offline {
            harness.registry.script.lock().unwrap().list = Some(wire::RegistryRefusal::Offline);
        }
        harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
        assert_eq!(harness.calls("list"), lists + 1);
        assert_eq!(harness.state().membership.as_ref(), Some(&membership));
        assert_eq!(harness.calls("recover_key"), 0);
        harness.registry.script.lock().unwrap().list = None;
        harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
        assert_eq!(harness.calls("list"), lists + 1 + usize::from(offline));
        harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
        assert_eq!(harness.calls("list"), lists + 1 + usize::from(offline));
        assert_eq!(harness.calls("recover_key"), 0);
    }
}
