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
        assert_eq!(harness.mesh.refresh(trigger).await, Err(AccountMeshRefusal::ReauthRequired));
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
    let token = harness.state().membership.as_ref().unwrap().token.clone();
    let cached = harness.mesh.with(|inner| inner.record.admission_cache.clone());
    let calls = count(&harness.log, "registry:");
    harness.mesh.enrollment_signed_in();
    assert_eq!(harness.state().last_refusal, None);
    assert_eq!(harness.state().membership.as_ref().unwrap().token, token);
    assert_eq!(harness.mesh.with(|inner| inner.record.admission_cache.clone()), cached);
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    assert_eq!(count(&harness.log, "registry:"), calls);
    assert_eq!(harness.calls("renew"), 0);
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
}

async fn fresh_rejoin() -> Harness {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.registry.revoke_self("owner");
    harness.clock.set(START + 3600);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Err(AccountMeshRefusal::ReauthRequired));
    harness.mesh.enrollment_signed_in();
    harness
}

#[tokio::test]
async fn account_mesh_rejoin_retries_only_slot_recovery_after_transient_failure() {
    for (refusal, delay) in [
        (RegistryRefusal::RateLimited { retry_after_seconds: 120 }, 120),
        (RegistryRefusal::Unavailable, 30), (RegistryRefusal::Offline, 30),
    ] {
        let harness = fresh_rejoin().await;
        harness.registry.script.lock().unwrap().membership_once = Some(refusal);
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Err(AccountMeshRefusal::from(refusal)));
        let binding = harness.store.record.lock().unwrap().keys[&account_id("owner")].clone();
        let calls = count(&harness.log, "registry:");
        harness.clock.set(START + 3600 + delay - 1);
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Err(AccountMeshRefusal::from(refusal)));
        assert_eq!(count(&harness.log, "registry:"), calls);
        harness.clock.set(START + 3600 + delay);
        harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
        assert_eq!(harness.calls("recover_key"), 1);
        assert_eq!(harness.calls("recover"), 2);
        let script = harness.registry.script.lock().unwrap();
        assert_eq!(script.recovery_bodies[0], script.recovery_bodies[1]);
        drop(script);
        assert_eq!(harness.store.record.lock().unwrap().keys[&account_id("owner")], binding);
        assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
    }
}

#[tokio::test]
async fn account_mesh_rejoin_retries_rate_limited_rekey_with_the_same_pending_key() {
    let harness = fresh_rejoin().await;
    harness.registry.script.lock().unwrap().enrollment = Some(RegistryRefusal::RateLimited { retry_after_seconds: 61 });
    let previous = harness.store.record.lock().unwrap().keys[&account_id("owner")].clone();
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Err(AccountMeshRefusal::RateLimited { retry_after_seconds: 61 }));
    assert_eq!(harness.store.record.lock().unwrap().keys[&account_id("owner")], previous);
    let first = harness.registry.script.lock().unwrap().key_requests.last().unwrap().body.clone();
    harness.registry.script.lock().unwrap().enrollment = None;
    harness.clock.set(START + 3660);
    let calls = count(&harness.log, "registry:");
    let _ = harness.mesh.refresh(EnrollTrigger::Lease).await;
    assert_eq!(count(&harness.log, "registry:"), calls);
    harness.clock.set(START + 3661);
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 2);
    assert_eq!(harness.calls("recover"), 1);
    assert_eq!(harness.registry.script.lock().unwrap().key_requests.last().unwrap().body, first);
    assert_eq!(harness.state().self_installation.as_ref().unwrap().key_generation, previous.generation + 1);
}

#[tokio::test]
async fn account_mesh_rejoin_window_expiry_requires_sign_in_without_network() {
    for rekey_succeeded in [false, true] {
        let harness = fresh_rejoin().await;
        let refusal = RegistryRefusal::RateLimited { retry_after_seconds: 900 };
        if rekey_succeeded { harness.registry.script.lock().unwrap().membership_once = Some(refusal); }
        else { harness.registry.script.lock().unwrap().enrollment = Some(refusal); }
        let _ = harness.mesh.refresh(EnrollTrigger::Automatic).await;
        let calls = count(&harness.log, "registry:");
        harness.clock.set(START + 4500);
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Err(AccountMeshRefusal::ReauthRequired));
        assert_eq!(count(&harness.log, "registry:"), calls);
        assert_eq!(harness.state().last_refusal, Some(AccountMeshRefusal::ReauthRequired));
        assert_eq!(harness.state().retry_at, None);
    }
}

#[tokio::test]
async fn account_mesh_cached_permit_identity_conflict_refreshes_once() {
    for refusal in [RegistryRefusal::Conflict, RegistryRefusal::Forbidden] {
        let harness = Harness::new().await;
        harness.sign_in("owner").await;
        harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
        let lists = harness.calls("list");
        {
            let mut script = harness.registry.script.lock().unwrap();
            script.peer_key = Some(("d".repeat(64), 2));
            script.permit_once = Some(refusal);
        }
        harness.mesh.authorize_link(LinkAuthorization {
            transcript: [9; 32], local_installation_id: INSTALLATION.into(),
            peer_installation_id: PEER.into(), account_id: account_id("owner"),
        }).await.unwrap();
        assert_eq!(harness.calls("list"), lists + 1);
        assert_eq!(harness.calls("link"), 2);
        let bodies = &harness.registry.script.lock().unwrap().link_bodies;
        let first = serde_json::to_value(&bodies[0]).unwrap();
        let second = serde_json::to_value(&bodies[1]).unwrap();
        assert_eq!(first["peer"]["key_generation"], 1);
        assert_eq!(second["peer"]["key_generation"], 2);
        assert_eq!(second["peer"]["key_id"], "d".repeat(64));
    }
}

#[tokio::test]
async fn account_mesh_repeated_permit_identity_refusal_does_not_loop() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.registry.script.lock().unwrap().permit = Some(RegistryRefusal::Conflict);
    assert_eq!(harness.mesh.authorize_link(LinkAuthorization {
        transcript: [10; 32], local_installation_id: INSTALLATION.into(),
        peer_installation_id: PEER.into(), account_id: account_id("owner"),
    }).await, Err(AccountMeshRefusal::Conflict));
    assert_eq!(harness.calls("link"), 2);
    assert_eq!(harness.calls("list"), 1);
}

#[tokio::test]
async fn account_mesh_budget_cached_registry_is_for_display_only() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.mesh.with(|inner| inner.update(&harness.mesh.watch, |state| {
        state.enrollment = Enrollment::Refused;
        state.last_refusal = Some(AccountMeshRefusal::EnrollmentBudget { retry_after_seconds: Some(300) });
    }));
    harness.registry.script.lock().unwrap().peer_unpublished = true;
    let display = harness.mesh.list().await.unwrap();
    assert!(display.rows.iter().find(|row| row.installation_id == PEER).unwrap().endpoint.is_some());
    assert_eq!(harness.calls("list"), 0);
    assert_eq!(harness.mesh.resolve_link_target(PEER).await.err(), Some(AccountMeshRefusal::Unavailable));
    assert_eq!(harness.calls("list"), 1);
}

#[tokio::test(start_paused = true)]
async fn account_mesh_rejoin_timer_parks_at_window_end_before_server_retry_deadline() {
    let harness = Harness::with_leases().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.registry.revoke_self("owner");
    harness.clock.set(START + 3600);
    let _ = harness.mesh.refresh(EnrollTrigger::Lease).await;
    harness.mesh.enrollment_signed_in();
    harness.registry.script.lock().unwrap().enrollment = Some(RegistryRefusal::RateLimited { retry_after_seconds: 3600 });
    let _ = harness.mesh.refresh(EnrollTrigger::Automatic).await;
    let calls = count(&harness.log, "registry:");
    let mut changes = harness.mesh.subscribe();
    harness.clock.set(START + 4500);
    tokio::time::advance(std::time::Duration::from_secs(900)).await;
    tokio::time::timeout(std::time::Duration::from_secs(1), changes.wait_for(|state| state.last_refusal == Some(AccountMeshRefusal::ReauthRequired))).await.unwrap().unwrap();
    assert_eq!(count(&harness.log, "registry:"), calls);
}

#[tokio::test]
async fn account_mesh_rejoin_outage_keeps_retrying_the_same_key_until_window_end() {
    let harness = fresh_rejoin().await;
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Unavailable);
    for elapsed in [0, 30, 90, 210, 450, 750] {
        harness.clock.set(START + 3600 + elapsed);
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Err(AccountMeshRefusal::Unavailable));
    }
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("recover"), 6);
    harness.clock.set(START + 4500);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Err(AccountMeshRefusal::ReauthRequired));
    assert_eq!(harness.calls("recover"), 6);
}

#[tokio::test]
async fn account_mesh_rejoin_lost_committed_reply_replays_or_reconciles_without_rekey() {
    for expired in [false, true] {
        let harness = fresh_rejoin().await;
        harness.registry.script.lock().unwrap().lose_membership_reply = Some(RegistryRefusal::Offline);
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Err(AccountMeshRefusal::Offline));
        assert_eq!(harness.registry.slot("owner"), Some(2));
        let binding = harness.store.record.lock().unwrap().keys[&account_id("owner")].clone();
        harness.registry.script.lock().unwrap().expire_recovery_receipt = expired;
        let lists = harness.calls("list");
        harness.clock.set(START + 3630);
        harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
        assert_eq!(harness.calls("recover_key"), 1);
        assert_eq!(harness.calls("recover"), 2);
        assert_eq!(harness.calls("list"), lists + 1);
        assert_eq!(harness.calls("renew"), if expired { 2 } else { 1 });
        assert_eq!(harness.store.record.lock().unwrap().keys[&account_id("owner")], binding);
        let script = harness.registry.script.lock().unwrap();
        assert_eq!(script.recovery_prepared[0], script.recovery_prepared[1]);
        assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
        assert_eq!(harness.transport.revoked.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn account_mesh_rejoin_pending_receipt_conflict_retries_the_identical_proof() {
    let harness = fresh_rejoin().await;
    harness.registry.script.lock().unwrap().membership_once = Some(RegistryRefusal::Conflict);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Err(AccountMeshRefusal::Conflict));
    assert_eq!(harness.state().retry_at, Some(START + 3630));
    let calls = count(&harness.log, "registry:");
    harness.clock.set(START + 3629);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Err(AccountMeshRefusal::Conflict));
    assert_eq!(count(&harness.log, "registry:"), calls);
    harness.clock.set(START + 3630);
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 1);
    let script = harness.registry.script.lock().unwrap();
    assert_eq!(script.recovery_prepared[0], script.recovery_prepared[1]);
}

#[tokio::test]
async fn account_mesh_unconfirmed_recovery_never_adopts_an_old_slot_generation() {
    let harness = fresh_rejoin().await;
    harness.registry.script.lock().unwrap().membership_once = Some(RegistryRefusal::ChallengeInvalid);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Err(AccountMeshRefusal::ReauthRequired));
    assert!(harness.state().membership.is_none());
    assert_eq!(harness.state().enrollment, Enrollment::Refused);
    let calls = count(&harness.log, "registry:");
    harness.clock.set(START + 3630);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Network).await, Err(AccountMeshRefusal::ReauthRequired));
    assert_eq!(count(&harness.log, "registry:"), calls);
    assert_eq!(harness.calls("recover_key"), 1);
}

#[tokio::test]
async fn account_mesh_rejoin_network_event_joins_the_current_recovery() {
    let harness = fresh_rejoin().await;
    harness.registry.entered.notified().await;
    let gate = Arc::new(Notify::new());
    harness.registry.script.lock().unwrap().gate = Some(gate.clone());
    let recovery = tokio::spawn({
        let mesh = harness.mesh.clone();
        async move { mesh.refresh(EnrollTrigger::Automatic).await }
    });
    harness.registry.entered.notified().await;
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Network).await, Ok(()));
    gate.notify_one();
    assert_eq!(recovery.await.unwrap(), Ok(()));
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("recover"), 1);
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
}
