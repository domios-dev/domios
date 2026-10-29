use super::support::*;
use crate::account_mesh::callable::{refusal, CallableFailure};
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::{AccountMeshRefusal, EnrollTrigger};
use serde_json::json;

const REFUSALS: &[(&str, RegistryRefusal)] = &[
    ("key_conflict", RegistryRefusal::KeyConflict),
    ("generation_mismatch", RegistryRefusal::GenerationMismatch),
    ("challenge_invalid", RegistryRefusal::ChallengeInvalid),
    ("enrollment_budget", RegistryRefusal::EnrollmentBudget),
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
