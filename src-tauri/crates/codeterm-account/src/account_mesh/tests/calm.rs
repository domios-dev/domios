use super::support::*;
use crate::account_mesh::{wire::RegistryRefusal, *};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Notify;

#[tokio::test]
async fn account_mesh_fresh_sign_in_cancels_the_previous_enrollment_epoch() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness.registry.script.lock().unwrap().gate = Some(Arc::new(Notify::new()));
    let old = tokio::spawn({
        let mesh = harness.mesh.clone();
        async move { mesh.refresh(EnrollTrigger::Automatic).await }
    });
    harness.registry.entered.notified().await;
    harness.mesh.enrollment_signed_in();
    harness.registry.script.lock().unwrap().gate = None;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    assert_eq!(old.await.unwrap(), Err(AccountMeshRefusal::Cancelled));
    assert_eq!(harness.calls("enroll"), 2);
    assert_eq!(harness.state().last_refusal, None);
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("enroll"), 2);
}

#[tokio::test]
async fn account_mesh_fresh_sign_in_assigns_a_stale_generation_and_recovers_own_slot_once() {
    let mut harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.store.record.lock().unwrap().keys.get_mut(&account_id("owner")).unwrap().generation = 999;
    harness.restart().await;
    harness.sign_in("owner").await;
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Err(AccountMeshRefusal::GenerationMismatch));
    let before = count(&harness.log, "registry:");
    harness.clock.set(START + 86400);
    for trigger in [EnrollTrigger::Network, EnrollTrigger::Lease, EnrollTrigger::Explicit] {
        assert_eq!(harness.mesh.refresh(trigger).await, Err(AccountMeshRefusal::GenerationMismatch));
    }
    assert_eq!(count(&harness.log, "registry:"), before);
    harness.mesh.enrollment_signed_in();
    assert_eq!(harness.state().last_refusal, None);
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    let state = harness.state();
    assert_eq!(state.enrollment, Enrollment::Enrolled);
    assert_eq!(state.self_installation.as_ref().unwrap().key_generation, 2);
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("recover"), 1);
    let script = harness.registry.script.lock().unwrap();
    let key = script.key_requests.last().unwrap();
    let body = serde_json::to_value(&key.body).unwrap();
    assert_eq!(body["generation_policy"], "assign");
    assert_eq!(body["expected_generation"], 0);
    assert_eq!(key.proof.generation, 0);
    let recovery = serde_json::to_value(script.recovery_bodies.last().unwrap()).unwrap();
    assert_eq!(recovery["target_policy"], "own");
    assert_eq!(recovery["target"], json!({"installation_id": INSTALLATION}));
    assert!(state.registry.as_ref().unwrap().rows.iter().any(|row| row.installation_id == INSTALLATION && row.active));
    assert!(harness.transport.revoked.lock().unwrap().contains(&INSTALLATION.to_owned()));
}

#[tokio::test]
async fn account_mesh_rejoin_refusal_parks_until_another_fresh_sign_in() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.registry.revoke_self("owner");
    harness.clock.set(START + 3600);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Err(AccountMeshRefusal::ReauthRequired));
    harness.mesh.enrollment_signed_in();
    harness.registry.script.lock().unwrap().enrollment = Some(RegistryRefusal::RateLimited { retry_after_seconds: 900 });
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Err(AccountMeshRefusal::RateLimited { retry_after_seconds: 900 }));
    let before = count(&harness.log, "registry:");
    for offset in [30, 900, 86400] {
        harness.clock.set(START + 3600 + offset);
        let _ = harness.mesh.refresh(EnrollTrigger::Network).await;
    }
    assert_eq!(count(&harness.log, "registry:"), before);
    assert_eq!(harness.calls("recover_key"), 1);
}

#[tokio::test]
async fn account_mesh_fresh_sign_in_replaces_cached_admission_once() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.mesh.with(|inner| inner.update(&harness.mesh.watch, |state| {
        state.enrollment = Enrollment::Refused;
        state.last_refusal = Some(AccountMeshRefusal::ReauthRequired);
    }));
    harness.mesh.enrollment_signed_in();
    assert_eq!(harness.state().last_refusal, None);
    assert!(harness.state().membership.is_none());
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
}
