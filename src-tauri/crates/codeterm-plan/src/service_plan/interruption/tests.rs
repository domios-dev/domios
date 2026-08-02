use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskLaneMetadata};
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
            "old-nonce",
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
fn restart_preserves_handoff_projection_and_interrupted_attempt_ledger() {
    let (dir, store, plan_id, task_id, attempt_id) = bound_store();
    store
        .interrupt_task_attempt(
            &plan_id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            "forced_context_reset",
            checkpoint(&task_id, &attempt_id),
            5,
        )
        .unwrap();
    drop(store);
    let reopened = PlanStore::new_for_test(dir.path());
    let plan = reopened.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(
        crate::service_plan::state::task_projection(&plan, task).presentation,
        crate::service_plan::TaskPresentation::HandoffRequired
    );
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert_eq!(attempt.interruption.reason.as_deref(), Some("forced_context_reset"));
    assert_eq!(
        attempt.interruption.checkpoint_digest,
        task.interruption
            .checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.diff_sha256.clone())
    );
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

#[test]
fn dispatch_persists_resolved_provider_on_exact_attempt() {
    let (_dir, store, plan_id, task_id, _) = bound_store();
    store.revert_task_dispatch(&plan_id, &task_id, 5).unwrap();
    let attempt_id = store
        .begin_task_dispatch_with_policy(
            &plan_id,
            &task_id,
            "provider-tab",
            "Worker",
            TaskLaneMetadata::default(),
            ExecutionIntent::Mutating,
            Some(allowance(90.0, 6)),
            Some("codex"),
            6,
        )
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert_eq!(attempt.provider.as_deref(), Some("codex"));
    assert_eq!(attempt.agent_type.as_deref(), Some("codex"));
}

#[test]
fn continued_review_attempt_is_interruptible_but_undelivered_epoch_is_not() {
    let (_dir, store, plan_id, task_id, attempt_id) = bound_store();
    store
        .with_plan(&plan_id, 5, |plan| {
            let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
            task.status = TaskStatus::InProgress;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .unwrap();
            attempt.reported_at = Some(4);
            attempt.continuation.epoch = 1;
            attempt.continuation.state = Some("continued".into());
            attempt.continuation.delivered_at = Some(5);
            Ok(())
        })
        .unwrap();
    assert!(store
        .begin_task_interruption(
            &plan_id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            "provider_exhausted",
            6,
        )
        .is_ok());

    let (_dir, store, plan_id, task_id, attempt_id) = bound_store();
    store
        .with_plan(&plan_id, 5, |plan| {
            let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .unwrap();
            attempt.reported_at = Some(4);
            attempt.continuation.epoch = 1;
            attempt.continuation.state = Some("awaiting_continuation".into());
            Ok(())
        })
        .unwrap();
    assert!(store
        .begin_task_interruption(
            &plan_id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            "provider_exhausted",
            6,
        )
        .is_err());
}

#[test]
fn orchestrator_can_close_and_assign_after_worker_epoch_adoption() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut owner = OrchestratorRef::from_tab("orch");
    owner.session_id = Some("orch-session".into());
    let mut plan = Plan::new("authority".into(), dir.path().display().to_string(), owner);
    store.save(&mut plan, 1).unwrap();
    let assign_task = store
        .add_task(&plan.id, NewTask { name: "assign".into(), kind: "implement".into(), ..Default::default() }, 2)
        .unwrap();
    let close_task = store
        .add_task(&plan.id, NewTask { name: "close".into(), kind: "implement".into(), ..Default::default() }, 3)
        .unwrap();
    store.with_plan(&plan.id, 4, |plan| {
        for task in &mut plan.tasks {
            task.interruption.owner_epoch = 2;
            task.interruption.owner_tab_id = Some("worker".into());
            task.interruption.owner_session_id = Some("worker-session".into());
        }
        Ok(())
    }).unwrap();
    let authority = TaskMutationAuthority {
        tab_id: "orch".into(),
        session_id: "orch-session".into(),
        session_binding_nonce: "orch-nonce".into(),
        owner_epoch: 2,
    };
    store.assign_task_with_role_authorized(
        &plan.id, &assign_task, "replacement", None, Some("worker"), &authority, 5,
    ).unwrap();
    store.close_task_authorized(
        &plan.id, &close_task, Some("accepted"), AgentRef::from_tab("orch"), &authority, 6,
    ).unwrap();
    let plan = store.get(&plan.id).unwrap();
    assert_eq!(plan.tasks.iter().find(|task| task.id == assign_task).unwrap().assigned_tab_id.as_deref(), Some("replacement"));
    assert_eq!(plan.tasks.iter().find(|task| task.id == close_task).unwrap().status, TaskStatus::Done);
}

#[test]
fn takeover_between_verification_and_write_is_rejected_atomically() {
    let (_dir, store, plan_id, task_id, _) = bound_store();
    let stale = TaskMutationAuthority {
        tab_id: "old-tab".into(),
        session_id: "old-session".into(),
        session_binding_nonce: "old-nonce".into(),
        owner_epoch: 1,
    };
    store.with_plan(&plan_id, 5, |plan| {
        let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
        task.interruption.owner_epoch = 2;
        task.interruption.owner_tab_id = Some("new-tab".into());
        task.interruption.owner_session_id = Some("new-session".into());
        Ok(())
    }).unwrap();
    assert!(store.close_task_authorized(
        &plan_id, &task_id, None, AgentRef::from_tab("old-tab"), &stale, 6,
    ).is_err());
    assert_ne!(store.show_task(&task_id).unwrap().task.status, TaskStatus::Done);
}
