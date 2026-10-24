use super::support::*;
use crate::account_mesh::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;

const UID: &str = "owner";

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
    harness
}

#[tokio::test]
async fn account_mesh_rejoin_rotates_then_enrolls_then_syncs() {
    let harness = removed().await;
    let old = harness.store.record.lock().unwrap().keys[&account_id(UID)].clone();
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.calls("sync"), 1);
    assert!(
        position(&harness.log, "registry:recover_key:")
            < position(&harness.log, "registry:enroll:")
    );
    assert!(position(&harness.log, "registry:enroll:") < position(&harness.log, "registry:sync:"));
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
async fn account_mesh_rejoin_stale_auth_requires_sign_in_without_rotation() {
    let harness = removed().await;
    auth_time(&harness, START - 301);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Explicit).await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    assert_eq!(harness.calls("recover_key"), 0);
    auth_time(&harness, START);
    harness.mesh.enrollment_signed_in();
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 1);
}

#[tokio::test]
async fn account_mesh_rejoin_failed_rotation_does_not_loop_in_a_sign_in_epoch() {
    let harness = removed().await;
    harness.registry.script.lock().unwrap().enrollment = Some(wire::RegistryRefusal::Unavailable);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Explicit).await,
        Err(AccountMeshRefusal::Unavailable)
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
    assert!(harness.registry.slot(UID).is_none());
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
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.calls("sync"), 2);
}
