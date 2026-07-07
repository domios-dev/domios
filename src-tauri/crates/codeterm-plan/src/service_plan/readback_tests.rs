use super::readback::pending_after_readback;
use super::PlanError;

#[test]
fn pending_after_readback_ok_when_mirror_pending_and_readback_matches() {
    let result = pending_after_readback::<()>(
        Err(PlanError::WritePending {
            operation: "plan_index_mirror".into(),
            committed: true,
            committed_id: Some("pl_test".into()),
            message: "database is locked".into(),
        }),
        Ok(()),
    );
    assert!(result.is_ok());
}

#[test]
fn pending_after_readback_fails_when_mirror_pending_but_readback_mismatches() {
    let result = pending_after_readback::<()>(
        Err(PlanError::WritePending {
            operation: "plan_index_mirror".into(),
            committed: true,
            committed_id: Some("pl_test".into()),
            message: "database is locked".into(),
        }),
        Err(PlanError::ReadbackMismatch {
            operation: "plan_update".into(),
            message: "title did not match".into(),
        }),
    );
    assert!(matches!(result, Err(PlanError::ReadbackMismatch { .. })));
}
