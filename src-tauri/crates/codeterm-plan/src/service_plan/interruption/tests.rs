use super::*;
use crate::plan::{OrchestratorRef, Plan, TaskLaneMetadata};
use crate::service_plan::NewTask;

fn allowance(remaining_pct: f32, observed_at: u64) -> ProviderAllowance {
    ProviderAllowance {
        remaining_pct,
        observed_at,
        stale_after_ms: 100,
        reserve_pct: 10.0,
    }
}

#[test]
fn mutating_dispatch_below_provider_reserve_changes_no_bytes() {
    assert!(provider_allowance_preflight(
        &ExecutionIntent::Mutating,
        Some(&allowance(9.0, 100)),
        100,
    )
    .is_err());
    assert!(provider_allowance_preflight(&ExecutionIntent::ReadOnly, None, 100).is_ok());
}

#[test]
fn stale_allowance_blocks_new_mutation_without_stopping_active_lane() {
    assert!(provider_allowance_preflight(
        &ExecutionIntent::Mutating,
        Some(&allowance(90.0, 1)),
        102,
    )
    .is_err());
}

fn bound_store() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "handoff".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "mutate".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = store
        .begin_task_dispatch(
            &plan.id,
            &task_id,
            "old-tab",
            "Worker",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    store
        .record_dispatch_policy(
            &plan.id,
            &task_id,
            &attempt_id,
            ExecutionIntent::Mutating,
            Some(allowance(90.0, 3)),
            3,
        )
        .unwrap();
    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            "old-tab",
            &attempt_id,
            "old-session",
            TaskLaneMetadata::default(),
            4,
        )
        .unwrap();
    (dir, store, plan.id, task_id, attempt_id)
}

fn checkpoint(task_id: &str, attempt_id: &str) -> InterruptionCheckpoint {
    let diff_artifact = "diff --git a/file b/file\n+dirty\n".to_string();
    InterruptionCheckpoint {
        task_id: task_id.into(),
        attempt_id: attempt_id.into(),
        tab_id: "old-tab".into(),
        session_id: "old-session".into(),
        owner_epoch: 1,
        repo_path: "repo".into(),
        worktree_path: "worktree".into(),
        base_head: "base".into(),
        current_head: "head".into(),
        git_status: "1 .M file".into(),
        diff_sha256: checkpoint_digest(&diff_artifact),
        diff_artifact,
        completed_tests: vec!["focused test".into()],
        unresolved_work: "finish implementation".into(),
        next_acceptance_gate: "canonical QA".into(),
    }
}

#[test]
fn exhaustion_after_dirty_edit_recovers_handoff_required() {
    let (_dir, store, plan_id, task_id, attempt_id) = bound_store();
    let sealed = checkpoint(&task_id, &attempt_id);
    store
        .interrupt_task_attempt(
            &plan_id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            "provider_exhausted",
            sealed.clone(),
            5,
        )
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    assert!(task.interruption.handoff_required);
    assert_eq!(task.status_label.as_deref(), Some("handoff_required"));
    assert_eq!(task.interruption.checkpoint, Some(sealed));
    assert!(task.execution_attempts[0].closed_at.is_some());
}

#[test]
fn takeover_epoch_fences_exhausted_owner() {
    let (_dir, store, plan_id, task_id, attempt_id) = bound_store();
    let sealed = checkpoint(&task_id, &attempt_id);
    store
        .interrupt_task_attempt(
            &plan_id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            "process_exited",
            sealed.clone(),
            5,
        )
        .unwrap();
    let (epoch, _) = store
        .adopt_interrupted_task(
            &plan_id,
            &task_id,
            &sealed.diff_sha256,
            "base",
            "new-tab",
            "new-session",
            6,
        )
        .unwrap();
    assert_eq!(epoch, 2);
    assert!(store
        .verify_task_owner_epoch(&task_id, "old-tab", "old-session", 1)
        .is_err());
    assert!(store
        .verify_task_owner_epoch(&task_id, "new-tab", "new-session", 2)
        .is_ok());
}
