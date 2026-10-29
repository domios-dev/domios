use super::support::*;
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::Notify;

async fn admitted() -> Harness {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.log.lock().unwrap().clear();
    harness
}

#[tokio::test]
async fn account_mesh_admission_reuse_within_validity_makes_zero_calls() {
    let harness = admitted().await;
    harness.clock.set(START + 3479);
    for trigger in [EnrollTrigger::Automatic, EnrollTrigger::Lease,
        EnrollTrigger::Explicit, EnrollTrigger::Network] {
        assert_eq!(harness.mesh.refresh(trigger).await, Ok(()));
    }
    assert_eq!(count(&harness.log, "registry:"), 0);
    assert_eq!(harness.calls("challenge"), 0);
    assert_eq!(harness.calls("renew"), 0);
    assert_eq!(harness.calls("sync"), 0);
    assert_eq!(harness.calls("presence"), 0);
    assert_eq!(harness.calls("publish"), 0);
}

#[tokio::test]
async fn account_mesh_admission_reuse_expiry_runs_one_handshake() {
    for elapsed in [3480, 3600] {
        let harness = admitted().await;
        harness.clock.set(START + elapsed);
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Ok(()));
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Ok(()));
        assert_eq!(harness.calls("renew"), 1);
        assert_eq!(harness.calls("sync"), 1);
        assert_eq!(harness.calls("challenge"), 2);
        assert_eq!(harness.state().membership.as_ref().unwrap().verified.expires_at,
            START + elapsed + 3600);
    }
}

#[tokio::test]
async fn account_mesh_admission_reuse_401_refreshes_once_retries_once_then_parks() {
    for challenge_failure in [false, true] {
        let harness = admitted().await;
        harness.clock.set(START + 3600);
        {
            let mut script = harness.registry.script.lock().unwrap();
            if challenge_failure {
                script.challenge = Some(RegistryRefusal::Unauthenticated);
            } else {
                script.membership = Some(RegistryRefusal::Unauthenticated);
            }
        }
        assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await,
            Err(AccountMeshRefusal::Unauthenticated));
        assert_eq!(harness.sessions.1.load(Ordering::SeqCst), 1);
        assert_eq!(harness.calls("challenge"), 2);
        assert_eq!(harness.calls("renew"), if challenge_failure { 0 } else { 2 });
        for trigger in [EnrollTrigger::Automatic, EnrollTrigger::Network, EnrollTrigger::Lease] {
            assert_eq!(harness.mesh.refresh(trigger).await,
                Err(AccountMeshRefusal::Unauthenticated));
        }
        assert_eq!(harness.mesh.list().await, Err(AccountMeshRefusal::Unauthenticated));
        assert_eq!(harness.calls("challenge"), 2);
        assert_eq!(harness.sessions.1.load(Ordering::SeqCst), 1);
        assert_eq!(harness.state().retry_at, None);
        {
            let mut script = harness.registry.script.lock().unwrap();
            script.challenge = None;
            script.membership = None;
        }
        let resume = if challenge_failure {
            EnrollTrigger::Explicit
        } else {
            harness.mesh.enrollment_signed_in();
            EnrollTrigger::Automatic
        };
        assert_eq!(harness.mesh.refresh(resume).await, Ok(()));
        assert_eq!(harness.calls("renew"), if challenge_failure { 1 } else { 3 });
    }
}

#[tokio::test]
async fn account_mesh_admission_reuse_401_success_uses_refreshed_session() {
    let harness = admitted().await;
    harness.clock.set(START + 3600);
    harness.registry.script.lock().unwrap().membership_once =
        Some(RegistryRefusal::Unauthenticated);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Ok(()));
    assert_eq!(harness.sessions.1.load(Ordering::SeqCst), 1);
    assert_eq!(harness.calls("renew"), 2);
    assert!(harness.log.lock().unwrap().iter()
        .any(|entry| entry == "registry:renew:refreshed-token"));
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Ok(()));
    assert_eq!(harness.calls("renew"), 2);
}

#[tokio::test]
async fn account_mesh_admission_reuse_concurrent_callers_share_one_handshake() {
    let harness = admitted().await;
    harness.clock.set(START + 3600);
    let gate = Arc::new(Notify::new());
    harness.registry.script.lock().unwrap().gate = Some(gate.clone());
    let owner = harness.mesh.refresh(EnrollTrigger::Lease);
    let joined = async {
        harness.registry.entered.notified().await;
        let explicit = harness.mesh.refresh(EnrollTrigger::Explicit);
        let release = async { gate.notify_one(); };
        tokio::join!(biased; explicit, release).0
    };
    harness.registry.entered.notified().await;
    let (owner, joined) = tokio::join!(biased; owner, joined);
    assert_eq!(owner, Ok(()));
    assert_eq!(joined, Ok(()));
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.calls("sync"), 1);
    assert_eq!(harness.calls("challenge"), 2);
}
