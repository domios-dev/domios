use super::readback::pending_after_readback;
use super::{NewTask, PlanError, PlanStore};
use crate::plan::{OrchestratorRef, Plan};

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

// A raw --handle ("T1") is server-normalized to a slug ("t1"); readback must not
// compare the raw requested handle against the committed slug and false-mismatch.
#[test]
fn add_task_with_raw_handle_does_not_false_mismatch_readback() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Readback".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("abc"),
    );
    store.save(&mut plan, 1).unwrap();

    let tid = store
        .add_task(
            &plan.id,
            NewTask {
                name: "T1 substrate".into(),
                kind: "implement".into(),
                handle: Some("T1".into()),
                acceptance_criteria: vec!["one indexed query".into()],
                test_plan: Some("unit tests".into()),
                correctness: Some("round-trips".into()),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .expect("add_task with a raw handle must not false-mismatch on readback");

    let committed = store.get(&plan.id).unwrap();
    let task = committed.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(task.handle.as_deref(), Some("t1"));
}
