use super::support::*;
use crate::account_mesh::wire::{MeshOperation, RegistryRefusal};
use crate::account_mesh::*;
use std::sync::atomic::Ordering;
use std::time::Duration;

async fn expired() -> Harness {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.registry.entered.notified().await;
    harness.clock.set(START + 3600);
    harness.log.lock().unwrap().clear();
    harness
}

#[tokio::test]
async fn account_mesh_401_refresh_outage_preserves_retry_then_recovers() {
    let harness = expired().await;
    harness.registry.script.lock().unwrap().membership_once =
        Some(RegistryRefusal::Unauthenticated);
    *harness.sessions.2.lock().unwrap() = Some(AccountMeshRefusal::Offline);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Lease).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert_eq!(harness.sessions.1.load(Ordering::SeqCst), 1);
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.state().retry_at, Some(START + 3630));
    *harness.sessions.2.lock().unwrap() = None;
    harness.clock.set(START + 3630);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Ok(()));
    assert_eq!(harness.calls("renew"), 2);
}

#[tokio::test]
async fn account_mesh_401_retry_preserves_transient_deadlines() {
    for (refusal, delay) in [
        (RegistryRefusal::Unavailable, 30),
        (
            RegistryRefusal::RateLimited {
                retry_after_seconds: 240,
            },
            240,
        ),
    ] {
        let harness = expired().await;
        {
            let mut script = harness.registry.script.lock().unwrap();
            script.challenge_once = Some(RegistryRefusal::Unauthenticated);
            script.membership = Some(refusal);
        }
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Lease).await,
            Err(AccountMeshRefusal::from(refusal))
        );
        assert_eq!(harness.state().retry_at, Some(START + 3600 + delay));
        assert_eq!(harness.calls("renew"), 1);
        assert_eq!(harness.sessions.1.load(Ordering::SeqCst), 1);
        harness.registry.script.lock().unwrap().membership = None;
        harness.clock.set(START + 3600 + delay);
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Ok(()));
        assert_eq!(harness.calls("renew"), 2);
    }
}

#[tokio::test]
async fn account_mesh_401_retry_preserves_typed_refusals_and_epoch() {
    for refusal in [
        RegistryRefusal::Revoked,
        RegistryRefusal::Forbidden,
        RegistryRefusal::Conflict,
        RegistryRefusal::Invalid,
    ] {
        let harness = expired().await;
        let signed = harness.mesh.signed(None).await.unwrap();
        {
            let mut script = harness.registry.script.lock().unwrap();
            script.challenge_once = Some(RegistryRefusal::Unauthenticated);
            script.membership = Some(refusal);
        }
        assert_eq!(
            harness
                .mesh
                .admission(
                    &signed,
                    MeshOperation::Renew {
                        expected_slot_revision: 1
                    }
                )
                .await,
            Err(AccountMeshRefusal::from(refusal))
        );
        assert_eq!(harness.state().account_epoch, signed.epoch);
        assert!(harness.mesh.with(|inner| inner.auth_parked.is_none()));
        assert_eq!(harness.sessions.1.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn account_mesh_401_retry_conflict_reconciles_and_revocation_rejoins() {
    for revoked in [false, true] {
        let harness = expired().await;
        {
            let mut script = harness.registry.script.lock().unwrap();
            script.challenge_once = Some(RegistryRefusal::Unauthenticated);
            if !revoked {
                script.membership_once = Some(RegistryRefusal::Conflict);
            }
        }
        if revoked {
            harness.registry.revoke_self("owner");
        }
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Ok(()));
        if revoked {
            assert_eq!(harness.calls("recover_key"), 1);
            assert_eq!(harness.calls("enroll"), 1);
        } else {
            assert_eq!(harness.calls("list"), 1);
            assert_eq!(harness.calls("renew"), 2);
        }
        assert!(harness.mesh.with(|inner| inner.auth_parked.is_none()));
    }
}

#[tokio::test(start_paused = true)]
async fn account_mesh_short_successful_leases_never_renew_immediately() {
    for seconds in [1, 119, 120, 121, 3600] {
        let harness = Harness::with_leases().await;
        harness.registry.script.lock().unwrap().membership_seconds = Some(seconds);
        harness.sign_in("owner").await;
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Ok(()));
        harness.registry.entered.notified().await;
        assert!(
            tokio::time::timeout(Duration::ZERO, harness.registry.entered.notified())
                .await
                .is_err()
        );
        assert_eq!(harness.calls("enroll"), 1);
        assert_eq!(harness.calls("renew"), 0);
        let due = harness
            .state()
            .membership
            .as_ref()
            .unwrap()
            .verified
            .refresh_after();
        assert!(due > START);
        harness.clock.set(due);
        tokio::time::timeout(
            Duration::from_secs(3600),
            harness.registry.entered.notified(),
        )
        .await
        .unwrap();
        let mesh = harness.mesh.clone();
        mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
        assert_eq!(harness.calls("renew"), 1);
        assert!(
            tokio::time::timeout(Duration::ZERO, harness.registry.entered.notified())
                .await
                .is_err()
        );
        harness.mesh.sign_out_local().await.unwrap();
    }
}

#[tokio::test]
async fn account_mesh_refresh_during_rejected_retry_cannot_lose_credential_rearm() {
    let harness = expired().await;
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.challenge_once = Some(RegistryRefusal::Unauthenticated);
        script.membership = Some(RegistryRefusal::Unauthenticated);
        script.gate = Some(gate.clone());
    }
    let refresh = harness.mesh.refresh(EnrollTrigger::Lease);
    let rotate = async {
        harness.registry.entered.notified().await;
        let mut session = harness.sessions.0.lock().unwrap().clone().unwrap();
        session.id_token = crate::Secret::new("concurrent-credential");
        *harness.sessions.0.lock().unwrap() = Some(session.clone());
        assert!(!harness.mesh.credentials_refreshed(&session));
        gate.notify_one();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(biased; refresh, rotate)
    })
    .await
    .unwrap();
    assert_eq!(result, Err(AccountMeshRefusal::Unavailable));
    assert!(harness.mesh.with(|inner| inner.auth_parked.is_none()));
    assert_eq!(harness.state().retry_at, Some(START + 3630));
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.membership = None;
        script.gate = None;
    }
    harness.clock.set(START + 3630);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Ok(()));
}
