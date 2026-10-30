use super::support::*;
use crate::account_mesh::callable::{refusal, CallableFailure};
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::{AccountMeshRefusal, EnrollTrigger};
use serde_json::json;

const REFUSALS: &[(&str, RegistryRefusal)] = &[
    ("key_conflict", RegistryRefusal::KeyConflict),
    ("generation_mismatch", RegistryRefusal::GenerationMismatch),
    ("challenge_invalid", RegistryRefusal::ChallengeInvalid),
    (
        "enrollment_budget",
        RegistryRefusal::EnrollmentBudget {
            retry_after_seconds: None,
        },
    ),
    ("proof_invalid", RegistryRefusal::ProofInvalid),
];

#[test]
fn enrollment_callable_details_preserve_each_typed_refusal() {
    for &(reason, expected) in REFUSALS {
        let failure = CallableFailure::Status {
            status: if reason == "enrollment_budget" {
                429
            } else {
                400
            },
            retry_after_seconds: None,
            body: Some(json!({"error": {"details": {"reason": reason}}})),
        };
        assert_eq!(refusal(&failure), expected);
        assert_eq!(
            serde_json::to_value(AccountMeshRefusal::from(expected)).unwrap(),
            json!({"reason": reason})
        );
    }
    assert_eq!(
        RegistryRefusal::from_details(&json!({"reason":"unknown_enrollment_failure"})),
        RegistryRefusal::Unavailable
    );
}

#[tokio::test]
async fn enrollment_refusals_reach_last_refusal_without_recovering_the_key() {
    for &(reason, refusal) in REFUSALS {
        let harness = Harness::new().await;
        harness.registry.script.lock().unwrap().enrollment = Some(refusal);
        harness.sign_in("user-1").await;
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Automatic).await,
            Err(AccountMeshRefusal::from(refusal))
        );
        let view = serde_json::to_value(harness.state().view()).unwrap();
        assert_eq!(view["enrollment"], "refused");
        assert_eq!(view["last_refusal"], json!({"reason": reason}));
        assert_eq!(harness.calls("enroll_key"), 1);
        assert_eq!(harness.calls("recover_key"), 0);
    }
}

#[test]
fn enrollment_budget_tolerates_optional_lenient_deadlines_and_extra_details() {
    for (value, expected) in [
        (json!(61), Some(61)),
        (json!(" 61.2 "), Some(62)),
        (json!(0.5), Some(1)),
        (json!(0), None),
        (json!(-1), None),
        (json!(null), None),
        (json!("invalid"), None),
        (json!(true), None),
        (json!("NaN"), None),
        (json!("Infinity"), None),
    ] {
        let details =
            json!({"reason":"enrollment_budget", "retry_after_seconds":value, "future_key":true});
        let expected = RegistryRefusal::EnrollmentBudget {
            retry_after_seconds: expected,
        };
        assert_eq!(RegistryRefusal::from_details(&details), expected);
        assert_eq!(
            refusal(&CallableFailure::Status {
                status: 429,
                retry_after_seconds: None,
                body: Some(json!({"error":{"details":details}})),
            }),
            expected
        );
    }
}

#[tokio::test]
async fn budget_deadline_is_scheduled_once_and_cached_registry_remains_readable() {
    let mut harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let registry = harness.state().registry.clone().unwrap();
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.clock.set(START + 64800);
    let refusal = RegistryRefusal::EnrollmentBudget {
        retry_after_seconds: Some(61),
    };
    harness.registry.script.lock().unwrap().membership = Some(refusal);
    let expected = Err(AccountMeshRefusal::from(refusal));
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        expected
    );
    assert_eq!(harness.state().retry_at, Some(START + 64861));
    assert_eq!(harness.mesh.lease_due(&expected), Some(START + 64861));
    assert_eq!(harness.mesh.list().await.unwrap(), registry);
    harness.clock.set(START + 64861);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, expected);
    assert_eq!(harness.state().retry_at, None);
    assert_eq!(harness.mesh.lease_due(&expected), None);
    assert_eq!(harness.mesh.list().await.unwrap(), registry);
    let calls = harness.calls("renew");
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Network).await, expected);
    assert_eq!(harness.calls("renew"), calls);
}

#[tokio::test(start_paused = true)]
async fn budget_timer_fires_once_then_stops_until_explicit_recovery() {
    let harness = Harness::with_leases().await;
    harness.sign_in("owner").await;
    harness.registry.script.lock().unwrap().enrollment = Some(RegistryRefusal::EnrollmentBudget {
        retry_after_seconds: None,
    });
    let refusal = Err(AccountMeshRefusal::EnrollmentBudget {
        retry_after_seconds: None,
    });
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        refusal
    );
    assert_eq!(harness.calls("enroll_key"), 1);
    assert_eq!(harness.state().retry_at, Some(START + 900));
    let mut changed = harness.mesh.subscribe();
    harness.clock.set(START + 900);
    tokio::time::advance(std::time::Duration::from_secs(900)).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        changed.wait_for(|state| {
            state.enrollment == crate::account_mesh::Enrollment::Refused && state.retry_at.is_none()
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(harness.calls("enroll_key"), 2);
    harness.clock.set(START + 3600);
    tokio::time::advance(std::time::Duration::from_secs(2700)).await;
    assert_eq!(harness.calls("enroll_key"), 2);
    harness.registry.script.lock().unwrap().enrollment = None;
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
}

#[tokio::test]
async fn ordinary_enrollment_outage_does_not_prevent_a_fresh_registry_read() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.clock.set(START + 64800);
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Unavailable);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Lease).await,
        Err(AccountMeshRefusal::Unavailable)
    );
    harness.registry.script.lock().unwrap().membership = None;
    let calls = harness.calls("list");
    let registry = harness.mesh.list().await.unwrap();
    assert_eq!(registry.observed_at, START + 64800);
    assert_eq!(harness.calls("list"), calls + 1);
}
