use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, PlanMode, TaskStatus};
use crate::service_plan::interruption::TaskMutationAuthority;
use crate::service_plan::task_assignment::AssignmentRuntimeBinding;
use crate::service_plan::task_attempts::BoundWorkerReport;
use crate::service_plan::{NewTask, TaskLaneUpdate};
use codeterm_types::agent::ReportStatus;

fn setup() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Review bounce".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "cleaned review generation".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["the review can be bounced".into()],
                test_plan: Some("Store-level lifecycle fixture".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, plan.id, task_id)
}

fn add_reported_generation(
    dir: &tempfile::TempDir,
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    generation: u64,
) -> String {
    let assigned_at = generation * 10;
    store
        .update_task_lane(
            plan_id,
            task_id,
            TaskLaneUpdate {
                worktree_path: Some(Some(
                    dir.path()
                        .join(format!("removed-lane-{generation}"))
                        .display()
                        .to_string(),
                )),
                branch: Some(Some(format!("lane/removed-{generation}"))),
                ..Default::default()
            },
            assigned_at - 1,
        )
        .unwrap();
    let session_id = format!("worker-session-{generation}");
    let nonce = format!("worker-nonce-{generation}");
    let binding = AssignmentRuntimeBinding {
        session_id: session_id.clone(),
        session_binding_nonce: nonce.clone(),
        provider: Some("codex".into()),
        model: Some("codex-test".into()),
        session_model: Some("codex-test".into()),
        reasoning_effort: Some("high".into()),
    };
    store
        .assign_task_with_runtime_binding(
            plan_id,
            task_id,
            tab_id,
            None,
            Some("Worker"),
            Some(&binding),
            None,
            assigned_at,
        )
        .unwrap();
    let assigned = store.show_task(task_id).unwrap().task;
    let attempt_id = assigned.execution_attempts.last().unwrap().id.clone();
    store
        .acknowledge_task_attempt(plan_id, task_id, tab_id, &attempt_id, assigned_at + 1)
        .unwrap();
    let authority = TaskMutationAuthority {
        tab_id: tab_id.into(),
        session_id,
        session_binding_nonce: nonce,
        owner_epoch: assigned.interruption.owner_epoch,
    };
    store
        .ingest_bound_worker_report(
            plan_id,
            task_id,
            &AgentRef::from_tab(tab_id),
            BoundWorkerReport {
                message: format!("review report {generation}"),
                status: Some(ReportStatus::InReview),
                sha: Some(format!("abcdef{generation:06}")),
                authority: Some(authority),
                ..Default::default()
            },
            assigned_at + 2,
        )
        .unwrap();
    attempt_id
}

fn retire_and_clean(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    outcome: TaskStatus,
    now_ms: u64,
) {
    store
        .retire_stale_task_owner(plan_id, task_id, Some(outcome), |_| false, now_ms)
        .unwrap();
    let plan = store.get(plan_id).unwrap();
    assert!(matches!(
        crate::service_plan::lane_cleanup::try_cleanup_task_lane(
            store,
            &plan,
            task_id,
            |_| false,
            now_ms + 1,
        ),
        crate::service_plan::lane_cleanup::LaneCleanupOutcome::Cleaned
    ));
    let task = store.show_task(task_id).unwrap().task;
    assert_eq!(task.lane.cleanup_status.as_deref(), Some("done"));
    assert_eq!(task.lane.worktree_path, None);
    assert_eq!(task.lane.branch, None);
    assert_eq!(task.assigned_tab_id, None);
    assert_eq!(task.peer, None);
}

fn set_status_label(store: &PlanStore, plan_id: &str, task_id: &str, label: &str, now_ms: u64) {
    store
        .update_task(
            plan_id,
            task_id,
            None,
            None,
            None,
            None,
            None,
            Some(Some(label)),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            now_ms,
        )
        .unwrap();
}

fn two_reported_cleaned_generations() -> (tempfile::TempDir, PlanStore, String, String, Vec<String>) {
    let (dir, store, plan_id, task_id) = setup();
    let first = add_reported_generation(&dir, &store, &plan_id, &task_id, "worker-one", 1);
    retire_and_clean(
        &store,
        &plan_id,
        &task_id,
        TaskStatus::Pending,
        13,
    );
    let second = add_reported_generation(&dir, &store, &plan_id, &task_id, "worker-two", 2);
    retire_and_clean(
        &store,
        &plan_id,
        &task_id,
        TaskStatus::InReview,
        25,
    );
    set_status_label(&store, &plan_id, &task_id, "review", 27);
    (dir, store, plan_id, task_id, vec![first, second])
}

#[test]
fn review_bounce_cleaned_two_generations_returns_pending() {
    let (_dir, store, plan_id, task_id, attempt_ids) = two_reported_cleaned_generations();
    let before = store.show_task(&task_id).unwrap().task;
    assert_eq!(before.status, TaskStatus::InReview);
    assert_eq!(before.execution_attempts.len(), 2);
    assert_eq!(
        before.execution_attempts[1].id,
        attempt_ids[1],
        "persisted append order establishes the terminal generation"
    );

    let bounced = store
        .bounce_review(
            &plan_id,
            &task_id,
            "resume review after cleanup",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            28,
        )
        .unwrap_or_else(|error| panic!("cleaned terminal generation should bounce: {error:?}"));

    assert_eq!(bounced.continuation, None);
    assert_eq!(bounced.review_round, before.review_round + 1);
    let after = store.show_task(&task_id).unwrap().task;
    assert_eq!(after.status, TaskStatus::Pending);
    assert_eq!(after.status_label.as_deref(), Some("review bounced"));
    assert_eq!(after.review_evidence.len(), before.review_evidence.len() + 1);
    assert_eq!(after.milestones.len(), before.milestones.len() + 1);
    assert_eq!(after.execution_attempts[0], before.execution_attempts[0]);
    assert_eq!(
        after.execution_attempts[1].report,
        before.execution_attempts[1].report
    );
    assert_eq!(
        after.execution_attempts[1].session_id,
        before.execution_attempts[1].session_id
    );
    assert_eq!(
        after.execution_attempts[1].closed_at,
        before.execution_attempts[1].closed_at
    );
    assert_eq!(
        after.execution_attempts[1].retired_at,
        before.execution_attempts[1].retired_at
    );
    assert_eq!(
        after.execution_attempts[1].continuation.review_round,
        Some(bounced.review_round)
    );
    assert_eq!(
        after.execution_attempts[1].continuation.state.as_deref(),
        Some("executor_gone")
    );
    assert_eq!(
        after.execution_attempts[1].continuation.milestone_id.as_deref(),
        Some(bounced.milestone_id.as_str())
    );
}
