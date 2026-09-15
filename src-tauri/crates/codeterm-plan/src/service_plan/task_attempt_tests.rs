use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskLaneMetadata, TaskStatus};
use codeterm_types::agent::ReportStatus;

mod plan_skill_evidence;
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
        diagnostics: None,
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
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.execution_attempts[0].report.is_some());
    assert_eq!(task.milestones.len(), 1);
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
fn a_superseded_dispatch_is_told_it_lost_the_task_not_that_it_owns_nothing() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let lane = TaskLaneMetadata::default();

    store
        .begin_task_dispatch(&plan_id, &task_id, "loser-tab", "Worker", lane.clone(), 3)
        .unwrap();
    // A second dispatch closes the first attempt by design (last writer wins).
    store
        .begin_task_dispatch(&plan_id, &task_id, "winner-tab", "Worker", lane, 4)
        .unwrap();

    let refusal = store
        .acknowledge_live_task_attempt_for_tab("loser-tab", Some(&plan_id), Some(&task_id), 5)
        .expect_err("a superseded tab must not acknowledge");
    let message = refusal.to_string();
    assert!(
        message.contains(super::task_dispatch::ACK_CAUSE_ATTEMPT_SUPERSEDED),
        "the loser must be told it was superseded, got: {message}"
    );
    assert!(
        message.contains("winner-tab"),
        "the refusal must name the tab that now holds the task, got: {message}"
    );
    assert!(
        !message.contains(super::task_dispatch::ACK_CAUSE_NO_LIVE_ATTEMPT),
        "'owns no live attempt' reads as a bug and makes the worker retry, got: {message}"
    );

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
