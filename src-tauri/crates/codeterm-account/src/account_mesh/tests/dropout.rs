use super::support::*;
use crate::account_mesh::{
    callable::{refusal, CallableFailure},
    wire::RegistryRefusal,
    *,
};
use std::time::Duration;

async fn enrolled() -> Harness {
    let harness = Harness::new().await;
    harness.registry.script.lock().unwrap().membership_seconds = Some(86400);
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness
}

#[tokio::test]
async fn account_mesh_lost_renew_reply_with_legacy_forbidden_reconciles_without_rotation() {
    let harness = enrolled().await;
    let key = harness
        .state()
        .self_installation
        .as_ref()
        .unwrap()
        .key_id
        .clone();
    let due = harness
        .state()
        .membership
        .as_ref()
        .unwrap()
        .verified
        .refresh_after();
    harness.clock.set(due);
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.disable_renew_replay = true;
        script.lose_membership_reply = Some(RegistryRefusal::Offline);
    }
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Lease).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert_eq!(harness.registry.slot("owner"), Some(2));
    let lists = harness.calls("list");
    harness.clock.set(harness.state().retry_at.unwrap());
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("list"), lists + 1);
    assert_eq!(harness.calls("recover_key"), 0);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.registry.slot("owner"), Some(3));
    assert_eq!(
        harness.state().self_installation.as_ref().unwrap().key_id,
        key
    );
    assert!(harness
        .state()
        .current_membership(harness.mesh.now())
        .is_some());
}

#[tokio::test]
async fn account_mesh_active_slot_with_different_key_uses_recover_after_rotation() {
    let mut harness = enrolled().await;
    *harness.keys.0.lock().unwrap() = Some(InstallationKey::generate().to_secret());
    harness.clock.set(START + 86400);
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("recover"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(
        harness
            .state()
            .self_installation
            .as_ref()
            .unwrap()
            .key_generation,
        2
    );
    assert!(harness.state().current_membership(START + 86400).is_some());
}

#[tokio::test]
async fn account_mesh_outage_beyond_retry_window_renews_before_expiry() {
    let harness = enrolled().await;
    harness.clock.set(START + 64800);
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Unavailable);
    let mut delays = Vec::new();
    for _ in 0..12 {
        let now = harness.mesh.now();
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Lease).await,
            Err(AccountMeshRefusal::Unavailable)
        );
        let due = harness
            .mesh
            .lease_due(&Err(AccountMeshRefusal::Unavailable))
            .unwrap();
        assert_eq!(Some(due), harness.state().retry_at);
        delays.push(due - now);
        assert!(harness.state().current_membership(due).is_some());
        harness.clock.set(due);
    }
    assert_eq!(
        delays,
        [30, 60, 120, 240, 300, 300, 300, 300, 300, 300, 300, 300]
    );
    harness.registry.script.lock().unwrap().membership = None;
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert!(harness.mesh.now() < START + 86400);
    assert!(
        harness
            .state()
            .membership
            .as_ref()
            .unwrap()
            .verified
            .expires_at
            > START + 86400
    );
}

#[tokio::test]
async fn account_mesh_partial_publication_failure_and_clock_skew_keep_a_deadline() {
    let harness = enrolled().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.registry.script.lock().unwrap().publication = Some(RegistryRefusal::Invalid);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Network).await,
        Err(AccountMeshRefusal::Invalid)
    );
    assert_eq!(
        harness.mesh.lease_due(&Err(AccountMeshRefusal::Invalid)),
        Some(START + 300)
    );
    harness.registry.script.lock().unwrap().publication = None;
    harness.clock.set(START + 300);
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.state().publication, Publication::Published);

    let skewed = Harness::new().await;
    skewed.registry.script.lock().unwrap().issuer_ahead = 121;
    skewed.sign_in("owner").await;
    assert_eq!(
        skewed.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::Invalid)
    );
    let due = skewed
        .mesh
        .lease_due(&Err(AccountMeshRefusal::Invalid))
        .unwrap();
    assert_eq!(due, START + 300);
    skewed.clock.set(due);
    skewed.registry.script.lock().unwrap().issuer_ahead = 0;
    skewed.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert!(skewed.state().current_membership(due).is_some());
    assert_eq!(skewed.calls("recover_key"), 0);
}

#[tokio::test(start_paused = true)]
async fn account_mesh_sleep_across_expiry_recovers_on_next_wall_clock_tick() {
    let harness = Harness::with_leases().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let first = harness.state().membership_epoch;
    let mut state = harness.mesh.subscribe();
    // Monotonic time stays still while wall time crosses expiry.
    harness.clock.set(START + 7200);
    let started = tokio::time::Instant::now();
    tokio::time::timeout(
        Duration::from_secs(61),
        state.wait_for(|state| {
            state.membership_epoch > first && state.enrollment == Enrollment::Enrolled
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(started.elapsed() <= Duration::from_secs(60));
    assert!(harness.state().current_membership(START + 7200).is_some());
    assert_eq!(harness.calls("renew"), 1);
    harness.mesh.sign_out_local().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn account_mesh_drift_check_does_no_network_before_wall_deadline() {
    let harness = Harness::with_leases().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let before = count(&harness.log, "registry:");
    tokio::time::advance(Duration::from_secs(600)).await;
    assert_eq!(count(&harness.log, "registry:"), before);
    harness.mesh.sign_out_local().await.unwrap();
}

#[tokio::test]
async fn account_mesh_untyped_403_and_404_retry_without_rejoining() {
    for status in [403, 404] {
        let harness = enrolled().await;
        let failure = CallableFailure::Status {
            status,
            body: None,
            retry_after_seconds: None,
        };
        let mapped = refusal(&failure);
        assert_eq!(mapped, RegistryRefusal::Unavailable);
        harness.clock.set(START + 64800);
        harness.registry.script.lock().unwrap().membership_once = Some(mapped);
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Lease).await,
            Err(AccountMeshRefusal::Unavailable)
        );
        harness.clock.set(harness.state().retry_at.unwrap());
        harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
        assert_eq!(harness.calls("recover_key"), 0);
        assert_eq!(harness.calls("enroll"), 1);
    }
    assert_eq!(
        refusal(&CallableFailure::Retired),
        RegistryRefusal::Cancelled
    );
    assert_eq!(
        AccountMeshRefusal::from(RegistryRefusal::Cancelled),
        AccountMeshRefusal::Cancelled
    );
}

#[tokio::test]
async fn account_mesh_forbidden_own_registry_read_rejoins_revoked_slot() {
    let harness = enrolled().await;
    harness.registry.revoke_self("owner");
    harness.clock.set(START + 64800);
    harness.mesh.enrollment_signed_in();
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("list"), 2);
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("recover"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert!(harness.state().current_membership(START + 64800).is_some());
}

#[tokio::test(start_paused = true)]
async fn account_mesh_retired_epoch_cannot_cancel_the_current_lease_timer() {
    let harness = Harness::with_leases().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let current = harness.state().account_epoch;
    let lease = harness.mesh.with(|inner| inner.lease);
    harness
        .mesh
        .schedule_lease(current - 1, &Err(AccountMeshRefusal::Cancelled));
    assert_eq!(harness.mesh.with(|inner| inner.lease), lease);
    let mut state = harness.mesh.subscribe();
    let membership_epoch = state.borrow().membership_epoch;
    harness.clock.set(START + 7200);
    tokio::time::timeout(
        Duration::from_secs(61),
        state.wait_for(|state| {
            state.membership_epoch > membership_epoch && state.enrollment == Enrollment::Enrolled
        }),
    )
    .await
    .unwrap()
    .unwrap();
    harness.mesh.sign_out_local().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn account_mesh_cold_machine_limit_stays_parked_until_explicit_or_signin_rearm() {
    for trigger in [EnrollTrigger::Explicit, EnrollTrigger::Automatic] {
        let harness = Harness::with_leases().await;
        harness.sign_in("owner").await;
        let refusal = RegistryRefusal::MachineLimit {
            limit: 2,
            active_count: 2,
        };
        harness.registry.script.lock().unwrap().membership = Some(refusal);
        let error = Err(AccountMeshRefusal::from(refusal));
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, error);
        assert!(harness.state().membership.is_none());
        assert_eq!(harness.mesh.lease_due(&error), None);
        let before = count(&harness.log, "registry:");
        harness.clock.set(START + 86400);
        tokio::time::advance(Duration::from_secs(86400)).await;
        assert_eq!(count(&harness.log, "registry:"), before);
        harness.registry.script.lock().unwrap().membership = None;
        if trigger == EnrollTrigger::Automatic {
            harness.mesh.enrollment_signed_in();
        }
        harness.mesh.refresh(trigger).await.unwrap();
        assert_eq!(harness.calls("enroll"), 2);
        assert!(harness.state().current_membership(START + 86400).is_some());
        harness.mesh.sign_out_local().await.unwrap();
    }
}

#[tokio::test]
async fn account_mesh_retired_renew_waiting_for_key_cannot_repopulate_new_epoch_request() {
    use crate::account_mesh::wire::{MeshAdmissionBody, MeshOperation};
    use std::{future::Future, task::Poll};
    let harness = enrolled().await;
    let signed = harness.mesh.signed(None).await.unwrap();
    let key_lock = harness.mesh.key_lock.lock().await;
    let mut old = std::pin::pin!(harness.mesh.admission(
        &signed,
        MeshOperation::Renew {
            expected_slot_revision: 1
        }
    ));
    std::future::poll_fn(|context| {
        assert!(old.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    harness.sign_in("other-owner").await;
    let new_request = MeshAdmissionBody {
        schema_version: 1,
        installation_id: INSTALLATION.into(),
        request_id: "e".repeat(64),
        requested_membership_seconds: Some(86400),
        operation: MeshOperation::Renew {
            expected_slot_revision: 77,
        },
    };
    harness.mesh.with(|inner| {
        inner.pending_renew = Some(new_request.clone());
        inner.last_request = Some(new_request.request_id.clone());
    });
    drop(key_lock);
    assert_eq!(old.await, Err(AccountMeshRefusal::Cancelled));
    harness.mesh.with(|inner| {
        assert_eq!(inner.pending_renew.as_ref(), Some(&new_request));
        assert_eq!(
            inner.last_request.as_deref(),
            Some(new_request.request_id.as_str())
        );
    });
    assert_eq!(harness.calls("renew"), 0);
}
