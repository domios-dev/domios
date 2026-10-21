use super::support::*;
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::*;

const UID: &str = "user-1";
const LIMIT: RegistryRefusal = RegistryRefusal::MachineLimit {
    limit: 2,
    active_count: 2,
};

async fn refused(refusal: RegistryRefusal) -> Harness {
    let harness = Harness::new().await;
    harness.registry.script.lock().unwrap().membership = Some(refusal);
    harness.sign_in(UID).await;
    harness
}

#[tokio::test]
async fn account_mesh_third_machine_refusal_keeps_sign_in_and_raises_one_owned_notice() {
    let harness = refused(LIMIT).await;
    let refusal = AccountMeshRefusal::MachineLimit {
        limit: 2,
        active_count: 2,
    };
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(refusal)
    );
    let state = harness.state();
    assert_eq!(state.signed_in_account(), Some(account_id(UID).as_str()));
    assert_eq!(state.traffic(), Traffic::Open);
    assert_eq!(state.enrollment, Enrollment::Refused);
    assert!(state.membership.is_none());
    let notice = state.limit_notice.clone().expect("notice");
    assert_eq!(notice.refusal, refusal);
    assert_eq!(notice.trigger, EnrollTrigger::Automatic);
    assert_eq!(notice.request_id.len(), 64);
    assert_eq!(
        serde_json::to_value(&notice.refusal).unwrap(),
        serde_json::json!({"reason": "mesh.machines", "limit": 2, "active_count": 2})
    );

    for trigger in [
        EnrollTrigger::Automatic,
        EnrollTrigger::Lease,
        EnrollTrigger::Network,
    ] {
        assert_eq!(harness.mesh.refresh(trigger).await, Err(refusal));
        assert_eq!(
            harness.state().limit_notice.as_ref(),
            Some(&notice),
            "{trigger:?}"
        );
    }
    assert!(
        harness
            .mesh
            .acknowledge_limit_notice(&notice.request_id)
            .await
    );
    assert!(
        !harness
            .mesh
            .acknowledge_limit_notice(&notice.request_id)
            .await
    );
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap_err();
    assert_eq!(
        harness.state().limit_notice,
        None,
        "automatic refusals notify once per sign-in"
    );

    harness
        .mesh
        .refresh(EnrollTrigger::Explicit)
        .await
        .unwrap_err();
    let retry = harness
        .state()
        .limit_notice
        .clone()
        .expect("explicit retry");
    assert_ne!(retry.request_id, notice.request_id);
    assert_eq!(retry.trigger, EnrollTrigger::Explicit);
    assert_eq!(harness.registry.slot(UID), None, "no slot is consumed");
}

#[tokio::test]
async fn account_mesh_other_refusals_never_raise_the_plans_notice() {
    let cases = [
        (
            RegistryRefusal::Unauthenticated,
            AccountMeshRefusal::Unauthenticated,
        ),
        (RegistryRefusal::AppCheck, AccountMeshRefusal::AppCheck),
        (RegistryRefusal::Forbidden, AccountMeshRefusal::Forbidden),
        (
            RegistryRefusal::RateLimited {
                retry_after_seconds: 300,
            },
            AccountMeshRefusal::RateLimited {
                retry_after_seconds: 300,
            },
        ),
        (RegistryRefusal::Offline, AccountMeshRefusal::Offline),
        (
            RegistryRefusal::Unavailable,
            AccountMeshRefusal::Unavailable,
        ),
        (
            RegistryRefusal::AuthorityUnavailable,
            AccountMeshRefusal::AuthorityUnavailable,
        ),
    ];
    for (server, desktop) in cases {
        let harness = refused(server).await;
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Explicit).await,
            Err(desktop)
        );
        let state = harness.state();
        assert_eq!(state.last_refusal, Some(desktop));
        assert_eq!(state.limit_notice, None, "{desktop:?}");
        assert!(state.signed_in_account().is_some());
    }
}

#[tokio::test]
async fn account_mesh_unpinned_trust_is_verification_unavailable_without_calls() {
    let harness = Harness::with_trust(None).await;
    harness.sign_in(UID).await;
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::VerificationUnavailable)
    );
    assert_eq!(harness.state().limit_notice, None);
    assert_eq!(count(&harness.log, "registry:"), 0);
}

#[tokio::test]
async fn account_mesh_refusal_before_sign_in_is_not_a_registry_call() {
    let harness = Harness::new().await;
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Explicit).await,
        Err(AccountMeshRefusal::Unauthenticated)
    );
    assert_eq!(
        harness.mesh.sign_possession(&[1u8; 32]),
        Err(AccountMeshRefusal::Unauthenticated)
    );
    assert_eq!(count(&harness.log, "registry:"), 0);
}
