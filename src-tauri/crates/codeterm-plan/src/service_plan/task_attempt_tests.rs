use super::*;
use crate::plan::{
    AgentRef, OrchestratorRef, Plan, TaskExecutionAttempt, TaskLaneMetadata, TaskStatus,
};
use codeterm_types::agent::ReportStatus;

mod blocked_dispatch_tests;
mod plan_skill_evidence;
mod session_model_replay;
mod task_attempt_respawn_tests;

pub(super) fn bound_report_store() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Worker reports".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "bound task".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["works".into()],
                test_plan: Some("cargo test -p codeterm-plan worker_report".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "worker", None, 3)
        .unwrap();
    (dir, store, plan.id, task_id)
}

fn mark_bound_task_in_progress(store: &PlanStore, plan_id: &str, task_id: &str) {
    store
        .with_plan(plan_id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::InProgress;
            task.status_changed_at = 3;
            Ok(())
        })
        .unwrap();
}

fn worker_report(status: ReportStatus, sha: Option<&str>) -> task_attempts::BoundWorkerReport {
    task_attempts::BoundWorkerReport {
        message: "CHANGES: report ingestion\nTESTS RUN: cargo test".into(),
        status: Some(status),
        branch: Some("lane/worker".into()),
        sha: sha.map(str::to_string),
        changed_files: vec!["src/report.rs".into()],
        test_cmd: Some("cargo test".into()),
        test_result: Some("ok".into()),
        skipped: None,
        authority: None,
    }
}

#[test]
fn dispatched_attempt_records_worktree_path_and_base() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "dispatch lane".into(),
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
    let lane = TaskLaneMetadata {
        worktree_path: Some("/lanes/task-a".into()),
        branch: Some("lane/task-a".into()),
        base: Some("basecommit".into()),
        ..Default::default()
    };

    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, "worker-tab", "Worker", lane, 3)
        .unwrap();

    let task = store.get(&plan.id).unwrap().tasks.remove(0);
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .expect("dispatch must create the reserved attempt");
    assert_eq!(attempt.worktree_path.as_deref(), Some("/lanes/task-a"));
    assert_eq!(attempt.base.as_deref(), Some("basecommit"));
    assert_eq!(attempt.branch.as_deref(), Some("lane/task-a"));
}

#[test]
fn dispatch_of_blocked_task_unblocks_atomically_and_ack_succeeds() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let block_reason = "awaiting dependency evidence";
    store
        .with_plan(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::Blocked;
            task.status_label = Some(block_reason.into());
            task.status_changed_at = 4;
            Ok(())
        })
        .unwrap();

    let lane = store.show_task(&task_id).unwrap().task.lane;
    let attempt_id = store
        .begin_task_dispatch(&plan_id, &task_id, "worker-next", "Worker", lane.clone(), 5)
        .unwrap();
    let pending = store.get(&plan_id).unwrap();
    let task = crate::plan::task_key::find_task(&pending.tasks, &task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(pending.strides.iter().any(|stride| {
        stride.links.iter().any(|link| link == &task_id)
            && stride.text.contains("unblocked by dispatch")
            && stride.text.contains(block_reason)
    }));

    store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            "worker-next",
            &attempt_id,
            "session-next",
            "nonce-next",
            lane,
            6,
        )
        .unwrap();
    let ack = store
        .acknowledge_live_task_attempt_for_tab("worker-next", Some(&plan_id), Some(&task_id), 7)
        .unwrap();
    assert_eq!(ack.attempt.id, attempt_id);
    assert_eq!(ack.attempt.acknowledged_at, Some(7));
}

#[test]
fn dispatch_without_declared_gates_records_an_explicit_empty_policy() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "dispatch gate default".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "no gate".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let attempt_id = store
        .begin_task_dispatch_with_reserved_attempt_and_gates(
            &plan.id,
            &task_id,
            "worker-tab",
            "Worker",
            TaskLaneMetadata::default(),
            crate::plan::ExecutionIntent::Unspecified,
            None,
            None,
            "attempt-gates",
            Vec::new(),
            crate::plan::WorkerPlacement::IsolatedLane,
            3,
        )
        .unwrap();
    let attempt = store
        .list_task_attempts(&plan.id, &task_id)
        .unwrap()
        .into_iter()
        .find(|attempt| attempt.attempt_id == attempt_id)
        .unwrap();
    assert_eq!(attempt.attempt.authorized_gates, Some(Vec::new()));
}

#[test]
fn undeclared_gate_claim_is_rejected_by_stable_error_code_before_persistence() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    store
        .with_plan(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.execution_attempts[0].authorized_gates = Some(vec!["gate:allowed".into()]);
            Ok(())
        })
        .unwrap();

    let error = store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, Some("abc123")),
            5,
        )
        .unwrap_err();
    assert_eq!(
        error.rpc_code(),
        crate::service_plan::RPC_UNAUTHORIZED_GATE_CLAIM
    );
    assert_eq!(error.to_json()["type"], "unauthorized_gate_claim");

    let task = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.milestones.is_empty());
    assert!(task.execution_attempts[0].report.is_none());
}

#[test]
fn bound_in_review_report_advances_atomically() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    assert!(store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, Some("abc123")),
            4,
        )
        .unwrap());

    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.lane.branch.as_deref(), Some("lane/worker"));
    let report = task.execution_attempts[0].report.as_ref().unwrap();
    assert_eq!(report.sha.as_deref(), Some("abc123"));
    assert_eq!(task.execution_attempts[0].executor_tab_id, "worker");
    assert_eq!(report.changed_files, vec!["src/report.rs"]);
}

#[test]
fn unplanned_task_in_review_report_moves_to_in_review() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Unplanned report".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "unplanned work".into(),
                kind: "unplanned".into(),
                acceptance_criteria: vec!["the brief is satisfied".into()],
                test_plan: Some(
                    "Orchestrator verifies the reported result against the brief".into(),
                ),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "worker", None, 3)
        .unwrap();
    mark_bound_task_in_progress(&store, &plan.id, &task_id);

    assert!(store
        .ingest_bound_worker_report(
            &plan.id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, Some("abc123")),
            4,
        )
        .unwrap());

    assert_eq!(
        store.get(&plan.id).unwrap().tasks[0].status,
        TaskStatus::InReview
    );
}

#[test]
fn planned_task_without_qa_plan_is_still_refused() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    store
        .with_plan(&plan_id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.acceptance_criteria.clear();
            task.test_plan = None;
            Ok(())
        })
        .unwrap();

    let error = store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, None),
            4,
        )
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        crate::service_plan::PlanError::ReviewEvidenceRequired.to_string()
    );
    assert_eq!(
        store.get(&plan_id).unwrap().tasks[0].status,
        TaskStatus::Pending
    );
}

#[test]
fn unplanned_partial_and_blocked_reports_follow_task_status_mapping() {
    for (status, expected) in [
        (ReportStatus::Partial, TaskStatus::InProgress),
        (ReportStatus::Blocked, TaskStatus::Blocked),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Unplanned report mapping".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "unplanned mapping".into(),
                    kind: "unplanned".into(),
                    acceptance_criteria: vec!["brief is satisfied".into()],
                    test_plan: Some("Orchestrator verifies the reported result".into()),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        store
            .assign_task(&plan.id, &task_id, "worker", None, 3)
            .unwrap();
        mark_bound_task_in_progress(&store, &plan.id, &task_id);
        let report = worker_report(status, None);

        assert!(!store
            .ingest_bound_worker_report(
                &plan.id,
                &task_id,
                &AgentRef::from_tab("worker"),
                report,
                4,
            )
            .unwrap());
        assert_eq!(store.get(&plan.id).unwrap().tasks[0].status, expected);
    }
}

#[test]
fn in_review_report_without_sha_still_advances_bound_task() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    assert!(store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, None),
            4,
        )
        .unwrap());
    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InReview);
    assert!(task.execution_attempts[0].report.is_some());
    assert_eq!(task.milestones.len(), 1);
}

#[test]
fn partial_report_keeps_bound_task_actionable() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    mark_bound_task_in_progress(&store, &plan_id, &task_id);
    assert!(!store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::Partial, Some("abc123")),
            4,
        )
        .unwrap());
    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.assigned_tab_id.as_deref(), Some("worker"));
    assert!(task.execution_attempts[0].report.is_some());
    assert!(task.execution_attempts[0].closed_at.is_none());
    assert_eq!(task.milestones.len(), 1);
}

#[test]
fn blocked_report_sets_task_blocked_with_reason() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let report = worker_report(ReportStatus::Blocked, None);
    let reason = report.message.clone();
    assert!(!store
        .ingest_bound_worker_report(&plan_id, &task_id, &AgentRef::from_tab("worker"), report, 4,)
        .unwrap());

    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::Blocked);
    assert_eq!(task.status_label.as_deref(), Some(reason.as_str()));
    assert!(task.execution_attempts[0].closed_at.is_none());
}

#[test]
fn blocked_report_on_in_review_task_is_kept_as_a_milestone() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, Some("abc123")),
            4,
        )
        .unwrap();

    let mut blocked = worker_report(ReportStatus::Blocked, None);
    blocked.message = "review already owns this task".into();
    assert!(!store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            blocked,
            5,
        )
        .unwrap());

    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.milestones.len(), 2);
    assert!(task.milestones[1]
        .text
        .contains("review already owns this task"));
    assert!(task.execution_attempts[0].closed_at.is_none());
}

#[test]
fn partial_report_then_redispatch_reuses_its_own_lane() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let lane = TaskLaneMetadata {
        worktree_path: Some("/repo/lane".into()),
        branch: Some("lane/partial".into()),
        base: Some("base-commit".into()),
        ..Default::default()
    };
    store
        .with_plan(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.lane = lane.clone();
            task.execution_attempts[0].worktree_path = lane.worktree_path.clone();
            task.execution_attempts[0].branch = lane.branch.clone();
            task.execution_attempts[0].base = lane.base.clone();
            Ok(())
        })
        .unwrap();
    assert!(!store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::Partial, Some("abc123")),
            5,
        )
        .unwrap());

    let reported = store.show_task(&task_id).unwrap().task;
    assert_eq!(reported.status, TaskStatus::Pending);
    let old_attempt_id = reported.execution_attempts[0].id.clone();
    let old_reported_at = reported.execution_attempts[0].reported_at;
    let observed_owner = store.dispatch_startup_owner_candidate(&plan_id, &task_id);
    assert!(observed_owner.is_none());
    let replacement_id = store
        .begin_task_dispatch_with_observation(
            &plan_id,
            &task_id,
            "replacement",
            "Worker",
            lane,
            observed_owner,
            false,
            6,
        )
        .unwrap();
    let redispatched = store.show_task(&task_id).unwrap().task;
    let old_attempt = redispatched
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == old_attempt_id)
        .unwrap();

    assert_eq!(old_reported_at, Some(5));
    assert_eq!(old_attempt.closed_at, Some(6));
    assert_eq!(old_attempt.worktree_path.as_deref(), Some("/repo/lane"));
    assert_eq!(old_attempt.branch.as_deref(), Some("lane/partial"));
    assert_eq!(old_attempt.base.as_deref(), Some("base-commit"));
    assert_eq!(
        redispatched.lane.worktree_path.as_deref(),
        Some("/repo/lane")
    );
    assert_eq!(redispatched.lane.branch.as_deref(), Some("lane/partial"));
    assert!(redispatched
        .execution_attempts
        .iter()
        .any(|attempt| attempt.id == replacement_id && attempt.closed_at.is_none()));

    let mut attempts = redispatched.execution_attempts;
    let replacement = TaskExecutionAttempt {
        actual_reasoning_effort: None,
        id: "later-replacement".into(),
        executor_tab_id: "later-worker".into(),
        ..Default::default()
    };
    task_attempts::record_noncompetitive_replacement_attempt(&mut attempts, replacement, 7);
    assert_eq!(
        attempts
            .iter()
            .find(|attempt| attempt.id == old_attempt_id)
            .unwrap()
            .closed_at,
        Some(6)
    );
}

#[test]
fn duplicate_stale_or_unbound_reports_never_regress_task() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, Some("abc123")),
            4,
        )
        .unwrap();
    assert!(!store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::Partial, Some("old")),
            3,
        )
        .unwrap());
    assert!(!store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("unbound"),
            worker_report(ReportStatus::InReview, Some("other")),
            5,
        )
        .unwrap());
    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(
        task.execution_attempts[0]
            .report
            .as_ref()
            .unwrap()
            .sha
            .as_deref(),
        Some("abc123")
    );
}

#[test]
fn assignment_attempt_records_planner_role() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Planner attempt".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "plan".into(),
                kind: "plan".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_role(&p.id, &task_id, "planner-tab", None, Some("planner"), 3)
        .unwrap();

    let attempt = store.list_task_attempts(&p.id, &task_id).unwrap().remove(0);
    assert_eq!(attempt.attempt.role.as_deref(), Some("planner"));
}

#[test]
fn task_attempt_ids_are_stable_distinct_from_executor_tab() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Attempts".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "same tab".into(),
                kind: "implement".into(),
                body: "Run twice in same tab.".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .assign_task(&p.id, &task_id, "tab_same", None, 3)
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_same", None, 4)
        .unwrap();

    let attempts = store.list_task_attempts(&p.id, &task_id).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_ne!(attempts[0].attempt_id, attempts[1].attempt_id);
    assert_ne!(attempts[0].attempt_id, "tab_same");
    assert_eq!(attempts[0].attempt.executor_tab_id, "tab_same");
    assert_eq!(attempts[1].attempt.executor_tab_id, "tab_same");
    assert!(attempts[0].attempt.executor_title.is_none());
    assert!(attempts[1].attempt.executor_title.is_none());

    let shown = store
        .show_task_attempt(&p.id, &task_id, &attempts[0].attempt_id)
        .unwrap();
    assert_eq!(shown.attempt_id, attempts[0].attempt_id);
    assert!(store
        .show_task_attempt(&p.id, &task_id, "tab_same")
        .is_err());
}

#[test]
fn terminal_status_update_leaves_history_but_no_active_presence() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Attempts".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "terminal".into(),
                kind: "implement".into(),
                body: "Close me.".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_live", None, 3)
        .unwrap();
    store
        .update_task(
            &p.id,
            &task_id,
            None,
            None,
            None,
            None,
            Some("cancelled"),
            None,
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
            4,
        )
        .unwrap();

    let p = store.get(&p.id).unwrap();
    let task = p.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Cancelled);
    assert!(task.assigned_tab_id.is_none());
    assert!(task.lane.tab_id.is_none());
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].executor_tab_id, "tab_live");
    assert_eq!(task.execution_attempts[0].closed_at, Some(4));
    assert!(store.task_binding_for_tab("tab_live").is_none());
    assert!(store.task_bindings_for_tab("tab_live").is_empty());
}

#[test]
fn recent_worker_attempt_titles_filters_workspace_session_and_limit() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Recent titles".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "task".into(),
                kind: "implement".into(),
                lane: TaskLaneMetadata {
                    workspace_id: Some("ws_target".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_role(&plan.id, &task_id, "tab-1", None, Some("worker"), 3)
        .unwrap();
    store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            "tab-1",
            Some("Ada"),
            Some("sess-1"),
            None,
            4,
        )
        .unwrap();
    store
        .assign_task_with_role(&plan.id, &task_id, "tab-2", None, Some("worker"), 5)
        .unwrap();
    store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            "tab-2",
            Some("Bea"),
            Some("sess-2"),
            None,
            6,
        )
        .unwrap();

    assert_eq!(
        store.recent_worker_attempt_titles("ws_target", Some("sess-1"), 10),
        vec!["Ada".to_string()]
    );
    assert_eq!(
        store.recent_worker_attempt_titles("ws_target", None, 1),
        vec!["Bea".to_string()]
    );
}

#[test]
fn reconcile_assignment_cleanup_closes_live_attempt_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Attempts".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "reconcile".into(),
                kind: "implement".into(),
                body: "Tab vanished.".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_live", None, 3)
        .unwrap();
    let drift = [crate::plan::reconcile::DriftItem {
        plan_id: p.id.clone(),
        task_id: task_id.clone(),
        kind: crate::plan::reconcile::DriftKind::BoundToClosedTab,
        detail: "closed".into(),
        suggested_fix: "clear".into(),
        not_mine: false,
    }];

    store.apply_reconcile_fixes(&p.id, &drift, 4).unwrap();

    let p = store.get(&p.id).unwrap();
    let task = p.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert!(task.assigned_tab_id.is_none());
    assert!(task.lane.tab_id.is_none());
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].executor_tab_id, "tab_live");
    assert_eq!(task.execution_attempts[0].closed_at, Some(4));
}

// Reproduces a proxy reporter attempting to replace the assigned executor.
#[test]
fn report_does_not_rebind_executor() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Masquerade".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "owned by A".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    // Explicit assignment path binds the real executor A.
    store
        .assign_task(&p.id, &task_id, "tab_A", None, 3)
        .unwrap();

    // Exercise the report path: lane truth, milestone, then attempt report.
    let reporter_b = AgentRef::from_tab("tab_B");
    store
        .record_report_lane_truth(&p.id, &task_id, Some("lane/b"), None, 4)
        .unwrap();
    store
        .add_milestone(
            &p.id,
            &task_id,
            "note",
            "STATUS: in_review from B",
            reporter_b.clone(),
            true,
            4,
        )
        .unwrap();
    store
        .record_task_attempt_report(
            &p.id,
            &task_id,
            &reporter_b,
            "STATUS: in_review\nCHANGES: done",
            Some(ReportStatus::InReview),
            4,
        )
        .unwrap();

    let p = store.get(&p.id).unwrap();
    let task = p.tasks.iter().find(|t| t.id == task_id).unwrap();

    // Executor binding is untouched by the report.
    assert_eq!(task.assigned_tab_id.as_deref(), Some("tab_A"));
    assert_eq!(task.lane.tab_id.as_deref(), Some("tab_A"));

    // No masquerade attempt is created.
    assert_eq!(task.execution_attempts.len(), 1);
    let attempt = &task.execution_attempts[0];
    assert_eq!(attempt.executor_tab_id, "tab_A");

    // The stride/milestone carries reporter B.
    let stride = task
        .milestones
        .iter()
        .find(|m| m.kind == "note")
        .expect("reporter stride");
    assert_eq!(stride.from.tab_id.as_deref(), Some("tab_B"));

    // The executor's attempt retains distinct reporter attribution.
    let report = attempt.report.as_ref().expect("attempt report recorded");
    assert_eq!(report.reporter_tab_id.as_deref(), Some("tab_B"));
    assert!(report.message.as_deref().unwrap().contains("CHANGES: done"));

    // Lane truth (branch) from the report was captured.
    assert_eq!(task.lane.branch.as_deref(), Some("lane/b"));
}

// A report cannot use an empty task assignment to capture executor ownership.
#[test]
fn report_on_unassigned_task_never_binds_reporter() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Unbound".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "unowned".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .record_report_lane_truth(&p.id, &task_id, Some("lane/b"), None, 3)
        .unwrap();

    let p = store.get(&p.id).unwrap();
    let task = p.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert!(
        task.assigned_tab_id.is_none(),
        "a report must not bind an executor onto an unassigned task"
    );
    assert!(task.lane.tab_id.is_none());
    assert_eq!(task.lane.branch.as_deref(), Some("lane/b"));
}

#[test]
fn a_live_dispatch_refuses_a_second_start_without_superseding() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let lane = TaskLaneMetadata::default();

    let live_attempt = store
        .begin_task_dispatch(&plan_id, &task_id, "winner-tab", "Worker", lane.clone(), 3)
        .unwrap();
    let observed_owner = store.dispatch_startup_owner_candidate(&plan_id, &task_id);
    let refusal = store
        .begin_task_dispatch_with_observation(
            &plan_id,
            &task_id,
            "loser-tab",
            "Worker",
            lane,
            observed_owner,
            true,
            4,
        )
        .expect_err("a live dispatch must reject a second start");
    let PlanError::DispatchInFlight { attempt_id, tab_id } = refusal else {
        panic!("expected typed in-flight refusal, got {refusal}");
    };
    assert_eq!(attempt_id, live_attempt);
    assert_eq!(tab_id, "winner-tab");

    let task = store.show_task(&task_id).unwrap().task;
    let open: Vec<_> = task
        .execution_attempts
        .iter()
        .filter(|attempt| attempt.closed_at.is_none())
        .collect();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, live_attempt);
    assert_eq!(open[0].executor_tab_id, "winner-tab");

    // A tab that never had an attempt is still the plain unresolved case.
    let stray = store
        .acknowledge_live_task_attempt_for_tab("stray-tab", None, None, 6)
        .expect_err("a stray shell must not acknowledge");
    assert!(
        stray
            .to_string()
            .contains(super::task_dispatch::ACK_CAUSE_NO_LIVE_ATTEMPT),
        "a tab with no attempt history keeps the unresolved cause, got: {stray}"
    );

    // The winner is unaffected by the loser's refusal.
    store
        .acknowledge_live_task_attempt_for_tab("winner-tab", Some(&plan_id), Some(&task_id), 7)
        .expect("the live attempt must still acknowledge");
}

#[test]
fn assign_then_dispatch_keeps_the_existing_supersede_semantics() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let observed_owner = store.dispatch_startup_owner_candidate(&plan_id, &task_id);
    assert!(observed_owner.is_none());

    let replacement_id = store
        .begin_task_dispatch_with_observation(
            &plan_id,
            &task_id,
            "worker",
            "Worker",
            TaskLaneMetadata::default(),
            observed_owner,
            false,
            4,
        )
        .expect("an assignment without a dispatched startup remains redispatchable");
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.execution_attempts.len(), 2);
    assert_eq!(task.execution_attempts[0].closed_at, Some(4));
    assert_eq!(task.execution_attempts[1].id, replacement_id);
    assert_eq!(task.execution_attempts[1].closed_at, None);
}

#[test]
fn concurrent_dispatch_reservations_allow_exactly_one_live_attempt() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let store = std::sync::Arc::new(store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let starts = ["worker-one", "worker-two"].map(|tab_id| {
        let store = store.clone();
        let barrier = barrier.clone();
        let plan_id = plan_id.clone();
        let task_id = task_id.clone();
        std::thread::spawn(move || {
            let observed_owner = store.dispatch_startup_owner_candidate(&plan_id, &task_id);
            barrier.wait();
            let result = store.begin_task_dispatch_with_observation(
                &plan_id,
                &task_id,
                tab_id,
                "Worker",
                TaskLaneMetadata::default(),
                observed_owner,
                false,
                4,
            );
            (tab_id, result)
        })
    });
    barrier.wait();

    let outcomes = starts.map(|start| start.join().unwrap());
    let mut winner = None;
    let mut refusal = None;
    for (tab_id, result) in outcomes {
        match result {
            Ok(attempt_id) => {
                assert!(winner.replace((tab_id.to_string(), attempt_id)).is_none());
            }
            Err(PlanError::DispatchInFlight { attempt_id, tab_id }) => {
                assert!(refusal.replace((attempt_id, tab_id)).is_none());
            }
            Err(error) => panic!("unexpected dispatch error: {error}"),
        }
    }
    let (winner_tab, winner_attempt) = winner.expect("one dispatch should reserve the task");
    assert_eq!(
        refusal,
        Some((winner_attempt.clone(), winner_tab.clone())),
        "the refusal must name the live attempt and tab"
    );

    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.assigned_tab_id.as_deref(), Some(winner_tab.as_str()));
    let open: Vec<_> = task
        .execution_attempts
        .iter()
        .filter(|attempt| attempt.closed_at.is_none())
        .collect();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, winner_attempt);
}

#[test]
fn concurrent_startup_reservations_refuse_before_worker_spawn() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let store = std::sync::Arc::new(store);
    let observed = std::sync::Arc::new(std::sync::Barrier::new(2));
    let reserved = std::sync::Arc::new(std::sync::Barrier::new(2));
    let attempts =
        [("attempt-a", "worker-a"), ("attempt-b", "worker-b")].map(|(attempt_id, tab_id)| {
            let store = store.clone();
            let plan_id = plan_id.clone();
            let task_id = task_id.clone();
            let observed = observed.clone();
            let reserved = reserved.clone();
            std::thread::spawn(move || {
                let owner = store.dispatch_startup_owner_candidate(&plan_id, &task_id);
                assert!(owner.is_none());
                observed.wait();
                let result = store.reserve_task_dispatch_startup(
                    &plan_id, &task_id, attempt_id, tab_id, owner, false,
                );
                match result {
                    Ok(reservation) => {
                        reserved.wait();
                        drop(reservation);
                        Ok((attempt_id.to_string(), tab_id.to_string()))
                    }
                    Err(error) => {
                        reserved.wait();
                        Err(error)
                    }
                }
            })
        });

    let outcomes = attempts.map(|attempt| attempt.join().unwrap());
    let (winner_attempt, winner_tab) = outcomes
        .iter()
        .find_map(|outcome| outcome.as_ref().ok().cloned())
        .expect("one startup reservation succeeds");
    let refusal = outcomes
        .into_iter()
        .find_map(Result::err)
        .expect("the competing startup reservation is refused");
    let PlanError::DispatchInFlight { attempt_id, tab_id } = refusal else {
        panic!("expected typed startup reservation refusal")
    };
    assert_eq!(attempt_id, winner_attempt);
    assert_eq!(tab_id, winner_tab);

    let reservation = store
        .reserve_task_dispatch_startup(
            &plan_id,
            &task_id,
            "attempt-after-release",
            "worker-after-release",
            None,
            false,
        )
        .expect("dropping the reservation releases the task");
    drop(reservation);
}

#[test]
fn repeated_runtime_binding_keeps_one_plan_attempt() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let attempt_id = store
        .begin_task_dispatch(
            &plan_id,
            &task_id,
            "worker-tab",
            "Worker",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    let attempts_before = store
        .show_task(&task_id)
        .unwrap()
        .task
        .execution_attempts
        .len();

    for now_ms in [4, 5] {
        let ready = store
            .mark_live_task_attempt_runtime_ready_for_tab("worker-tab", "session", now_ms)
            .unwrap()
            .expect("the same live attempt remains bindable");
        assert_eq!(ready.attempt.id, attempt_id);
    }

    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.execution_attempts.len(), attempts_before);
    let bound: Vec<_> = task
        .execution_attempts
        .iter()
        .filter(|attempt| attempt.id == attempt_id)
        .collect();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].session_id.as_deref(), Some("session"));
}

#[test]
fn a_closed_attempt_without_a_newer_live_dispatch_is_not_reported_as_superseded() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let attempt_id = store
        .begin_task_dispatch(
            &plan_id,
            &task_id,
            "worker-tab",
            "Worker",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    assert!(store
        .mark_worker_startup_deadline(&plan_id, &task_id, "worker-tab", &attempt_id, true, 4)
        .unwrap());

    let refusal = store
        .acknowledge_live_task_attempt_for_tab("worker-tab", Some(&plan_id), Some(&task_id), 5)
        .expect_err("a closed attempt without a replacement is not acknowledgeable");
    let message = refusal.to_string();
    assert!(message.contains(super::task_dispatch::ACK_CAUSE_NO_LIVE_ATTEMPT));
    assert!(!message.contains(super::task_dispatch::ACK_CAUSE_ATTEMPT_SUPERSEDED));
}
