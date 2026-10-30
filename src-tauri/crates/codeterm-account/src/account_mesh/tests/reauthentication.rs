use super::support::*;
use crate::account_mesh::callable::{refusal, CallableFailure};
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Notify;

const UID: &str = "user-1";

fn set_auth_time(harness: &Harness, auth_time: u64) {
    let mut sessions = harness.sessions.0.lock().unwrap();
    sessions.as_mut().unwrap().id_token = crate::Secret::new(format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(json!({ "auth_time": auth_time }).to_string())
    ));
}

#[test]
fn account_mesh_reauth_required_preserves_the_callable_reason() {
    let failure = CallableFailure::Status {
        status: 400,
        retry_after_seconds: None,
        body: Some(json!({"error": {"details": {"reason": "reauth_required"}}})),
    };
    assert_eq!(refusal(&failure), RegistryRefusal::ReauthRequired);
    assert_eq!(
        serde_json::to_value(AccountMeshRefusal::from(refusal(&failure))).unwrap(),
        json!({"reason": "reauth_required"})
    );
}

#[tokio::test]
async fn account_mesh_old_session_enrolls_without_sign_in() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    set_auth_time(&harness, START - 86400);
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.calls("enroll_key"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
    assert_eq!(harness.state().last_refusal, None);
}

#[tokio::test]
async fn account_mesh_typed_reauth_waits_until_verified_sign_in() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness.registry.script.lock().unwrap().enrollment = Some(RegistryRefusal::ReauthRequired);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    harness.registry.script.lock().unwrap().enrollment = None;
    assert_eq!(
        harness.mesh.list().await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Network).await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    assert_eq!(harness.calls("enroll_key"), 1);
    assert_eq!(harness.state().view().retry_at, None);
    harness.mesh.enrollment_signed_in();
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.calls("enroll_key"), 2);
    assert_eq!(harness.calls("enroll"), 1);
}

#[tokio::test]
async fn account_mesh_old_session_renews_without_sign_in() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    set_auth_time(&harness, START - 86400);
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.clock.set(START + 3600);
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("enroll_key"), 1);
    assert_eq!(harness.calls("renew"), 1);
}

#[tokio::test]
async fn account_mesh_old_session_rotates_a_lost_key_without_sign_in() {
    let mut harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    *harness.keys.0.lock().unwrap() = None;
    harness.restart().await;
    harness.sign_in(UID).await;
    set_auth_time(&harness, START - 86400);
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
    assert_eq!(harness.state().last_refusal, None);
}

#[tokio::test]
async fn account_mesh_post_sign_in_list_and_automatic_refresh_enroll_the_key_once() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    let gate = Arc::new(Notify::new());
    harness.registry.script.lock().unwrap().enrollment_gate = Some(gate.clone());
    let automatic = tokio::spawn({
        let mesh = harness.mesh.clone();
        async move { mesh.refresh(EnrollTrigger::Automatic).await }
    });
    harness.registry.entered.notified().await;
    let list = async { harness.mesh.list().await };
    let release = async {
        harness.registry.script.lock().unwrap().enrollment_gate = None;
        gate.notify_one();
        automatic.await.unwrap()
    };
    let (list, automatic) = tokio::join!(list, release);
    assert!(list.is_ok());
    assert_eq!(automatic, Ok(()));
    assert_eq!(harness.calls("enroll_key"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert!(harness.state().current_membership(START).is_some());
    assert!(harness
        .store
        .record
        .lock()
        .unwrap()
        .keys
        .contains_key(&account_id(UID)));
}

#[tokio::test]
async fn account_mesh_terminal_admission_waits_for_fresh_signin_across_restart() {
    for refusal in [
        RegistryRefusal::GenerationMismatch,
        RegistryRefusal::Forbidden,
    ] {
        let mut harness = Harness::new().await;
        harness.sign_in(UID).await;
        if refusal == RegistryRefusal::GenerationMismatch {
            harness.registry.script.lock().unwrap().enrollment = Some(refusal);
        } else {
            harness
                .mesh
                .refresh(EnrollTrigger::Automatic)
                .await
                .unwrap();
            harness.clock.set(START + 3600);
            harness.registry.script.lock().unwrap().membership = Some(refusal);
        }
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Automatic).await,
            Err(AccountMeshRefusal::ReauthRequired)
        );
        let calls = count(&harness.log, "registry:");
        for trigger in [
            EnrollTrigger::Lease,
            EnrollTrigger::Network,
            EnrollTrigger::Explicit,
        ] {
            harness.clock.set(START + 86400);
            assert_eq!(
                harness.mesh.refresh(trigger).await,
                Err(AccountMeshRefusal::ReauthRequired)
            );
            assert_eq!(
                harness.mesh.list().await,
                Err(AccountMeshRefusal::ReauthRequired)
            );
        }
        assert_eq!(count(&harness.log, "registry:"), calls);
        assert!(harness.state().membership.is_none());
        assert_eq!(harness.state().retry_at, None);
        harness.restart().await;
        harness.sign_in(UID).await;
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Automatic).await,
            Err(AccountMeshRefusal::ReauthRequired)
        );
        assert_eq!(count(&harness.log, "registry:"), calls);
        {
            let mut script = harness.registry.script.lock().unwrap();
            script.enrollment = None;
            script.membership = None;
        }
        harness.mesh.enrollment_signed_in();
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
        assert!(!harness
            .store
            .record
            .lock()
            .unwrap()
            .reauth_required_accounts
            .contains(&account_id(UID)));
    }
}

#[tokio::test]
async fn account_mesh_credential_refresh_cannot_reopen_terminal_signin_gate() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Unauthenticated);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::Unauthenticated)
    );
    assert_eq!(
        harness
            .mesh
            .require_sign_in(harness.state().account_epoch)
            .await,
        AccountMeshRefusal::ReauthRequired
    );
    let mut refreshed = session(UID);
    refreshed.id_token = crate::Secret::new("refreshed-after-terminal-refusal");
    assert!(!harness.mesh.credentials_refreshed(&refreshed));
    assert_eq!(
        harness.state().last_refusal,
        Some(AccountMeshRefusal::ReauthRequired)
    );
    assert_eq!(
        harness.mesh.list().await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    assert_eq!(harness.state().retry_at, None);
}
