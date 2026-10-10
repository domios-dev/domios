use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, PlanMode, TaskStatus};
use crate::service_plan::interruption::TaskMutationAuthority;
use crate::service_plan::task_assignment::AssignmentRuntimeBinding;
use crate::service_plan::task_attempts::{BoundWorkerReport, TaskCompeteDraft};
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

fn two_reported_cleaned_generations() -> (tempfile::TempDir, PlanStore, String, String, Vec<String>)
{
    let (dir, store, plan_id, task_id) = setup();
    let first = add_reported_generation(&dir, &store, &plan_id, &task_id, "worker-one", 1);
    retire_and_clean(&store, &plan_id, &task_id, TaskStatus::Pending, 13);
    let second = add_reported_generation(&dir, &store, &plan_id, &task_id, "worker-two", 2);
    retire_and_clean(&store, &plan_id, &task_id, TaskStatus::InReview, 25);
    set_status_label(&store, &plan_id, &task_id, "review", 27);
    (dir, store, plan_id, task_id, vec![first, second])
}

fn one_reported_cleaned_generation() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let (dir, store, plan_id, task_id) = setup();
    let attempt_id = add_reported_generation(&dir, &store, &plan_id, &task_id, "worker-one", 1);
    retire_and_clean(&store, &plan_id, &task_id, TaskStatus::InReview, 13);
    set_status_label(&store, &plan_id, &task_id, "review", 15);
    (dir, store, plan_id, task_id, attempt_id)
}

fn set_status(store: &PlanStore, plan_id: &str, task_id: &str, status: &str, now_ms: u64) {
    store
        .update_task(
            plan_id,
            task_id,
            None,
            None,
            None,
            None,
            Some(status),
            Some(None),
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

fn set_status_and_label(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    status: &str,
    label: &str,
    now_ms: u64,
) {
    store
        .update_task(
            plan_id,
            task_id,
            None,
            None,
            None,
            None,
            Some(status),
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

fn assert_review_bounce_refused_unchanged(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    now_ms: u64,
) {
    let before = store.get(plan_id).unwrap();
    let error = store
        .bounce_review(
            plan_id,
            task_id,
            "refusal leaves review state intact",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            now_ms,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        PlanError::ReviewContinuationAttemptNotFound { .. }
    ));
    assert_eq!(store.get(plan_id).unwrap(), before);
}

fn assert_attempt_history_preserved(
    before: &crate::plan::Task,
    after: &crate::plan::Task,
    selected_index: usize,
    bounced: &crate::service_plan::review_gate::ReviewTransitionResult,
) {
    assert_eq!(
        after.execution_attempts.len(),
        before.execution_attempts.len()
    );
    for (index, attempt) in after.execution_attempts.iter().enumerate() {
        let mut expected = before.execution_attempts[index].clone();
        if index == selected_index {
            expected.continuation.review_round = Some(bounced.review_round);
            expected.continuation.state = Some("executor_gone".into());
            expected.continuation.milestone_id = Some(bounced.milestone_id.clone());
            expected.continuation.delivered_at = None;
            expected
                .closure_reason
                .get_or_insert_with(|| "review_bounce_executor_gone".into());
        }
        assert_eq!(*attempt, expected);
    }
}

fn add_unreported_generation(
    dir: &tempfile::TempDir,
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    generation: u64,
    with_runtime_binding: bool,
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
    let binding = AssignmentRuntimeBinding {
        session_id: format!("worker-session-{generation}"),
        session_binding_nonce: format!("worker-nonce-{generation}"),
        provider: Some("codex".into()),
        model: Some("codex-test".into()),
        session_model: Some("codex-test".into()),
        reasoning_effort: Some("high".into()),
    };
    if with_runtime_binding {
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
    } else {
        store
            .assign_task_with_role(plan_id, task_id, tab_id, None, Some("Worker"), assigned_at)
            .unwrap();
    }
    store
        .show_task(task_id)
        .unwrap()
        .task
        .execution_attempts
        .last()
        .unwrap()
        .id
        .clone()
}

fn report_authority(store: &PlanStore, task_id: &str, tab_id: &str) -> TaskMutationAuthority {
    let task = store.show_task(task_id).unwrap().task;
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.executor_tab_id == tab_id && attempt.closed_at.is_none())
        .unwrap();
    TaskMutationAuthority {
        tab_id: tab_id.into(),
        session_id: attempt.session_id.clone().unwrap_or_default(),
        session_binding_nonce: attempt.session_binding_nonce.clone().unwrap_or_default(),
        owner_epoch: task.interruption.owner_epoch,
    }
}

#[test]
fn review_bounce_cleaned_two_generations_returns_pending() {
    let (_dir, store, plan_id, task_id, attempt_ids) = two_reported_cleaned_generations();
    let before_plan = store.get(&plan_id).unwrap();
    let before = store.show_task(&task_id).unwrap().task;
    assert_eq!(before.status, TaskStatus::InReview);
    assert_eq!(before.execution_attempts.len(), 2);
    assert_eq!(
        before.execution_attempts[1].id, attempt_ids[1],
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
    assert_eq!(
        after.review_evidence.len(),
        before.review_evidence.len() + 1
    );
    assert_eq!(after.milestones.len(), before.milestones.len() + 1);
    assert_eq!(
        after.review_evidence.last().unwrap().note,
        "resume review after cleanup"
    );
    assert_eq!(after.milestones.last().unwrap().id, bounced.milestone_id);
    assert_attempt_history_preserved(&before, &after, 1, &bounced);
    assert_eq!(
        store.get(&plan_id).unwrap().review_policy.credit,
        before_plan.review_policy.credit + 1
    );
}

#[test]
fn review_bounce_cleaned_single_terminal_generation_returns_pending() {
    let (_dir, store, plan_id, task_id, attempt_id) = one_reported_cleaned_generation();
    let before_plan = store.get(&plan_id).unwrap();
    let before = store.show_task(&task_id).unwrap().task;
    let bounced = store
        .bounce_review(
            &plan_id,
            &task_id,
            "resume the terminal review",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            16,
        )
        .unwrap();

    assert_eq!(bounced.continuation, None);
    assert_eq!(bounced.review_round, before.review_round + 1);
    let after = store.show_task(&task_id).unwrap().task;
    assert_eq!(after.status, TaskStatus::Pending);
    assert_eq!(after.execution_attempts[0].id, attempt_id);
    assert_attempt_history_preserved(&before, &after, 0, &bounced);
    assert_eq!(
        store.get(&plan_id).unwrap().review_policy.credit,
        before_plan.review_policy.credit + 1
    );
}

#[test]
fn review_bounce_cleaned_ordinary_assigned_live_continues() {
    let (dir, store, plan_id, task_id) = setup();
    let attempt_id = add_reported_generation(&dir, &store, &plan_id, &task_id, "live-worker", 1);
    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "continue the assigned worker",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            13,
        )
        .unwrap();

    let continuation = result.continuation.unwrap();
    assert_eq!(continuation.attempt_id, attempt_id);
    assert_eq!(continuation.session_id, "worker-session-1");
    assert_eq!(continuation.state, "awaiting_continuation");
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.status_label.as_deref(), Some("awaiting_continuation"));
}

#[test]
fn review_bounce_cleaned_original_ambiguity_refuses() {
    let (_dir, store, plan_id, task_id, _) = two_reported_cleaned_generations();
    set_status_label(
        &store,
        &plan_id,
        &task_id,
        crate::plan::reconcile::TAB_GONE_LABEL,
        28,
    );
    let task = store.show_task(&task_id).unwrap().task;
    assert!(matches!(
        exact_closed_review_attempt(&task),
        ClosedReviewAttempt::Ambiguous
    ));
    assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 29);
}

#[test]
fn review_bounce_cleaned_refuses_open_attempt() {
    let (_dir, store, plan_id, task_id) = setup();
    store
        .assign_task_with_role(&plan_id, &task_id, "open-worker", None, Some("Worker"), 3)
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    let attempt_id = task.execution_attempts[0].id.clone();
    store
        .acknowledge_task_attempt(&plan_id, &task_id, "open-worker", &attempt_id, 4)
        .unwrap();
    store
        .record_task_attempt_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("open-worker"),
            "unbound open report",
            Some(ReportStatus::InReview),
            5,
        )
        .unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 6);
    store
        .update_task_lane(
            &plan_id,
            &task_id,
            TaskLaneUpdate {
                tab_id: Some(None),
                cleanup_status: Some(Some("done".into())),
                ..Default::default()
            },
            7,
        )
        .unwrap();
    set_status_label(&store, &plan_id, &task_id, "review", 8);

    assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 9);
}

#[test]
fn review_bounce_cleaned_refuses_owner_or_placement() {
    for case in 0..7 {
        let (_dir, store, plan_id, task_id, _) = two_reported_cleaned_generations();
        match case {
            0 => store
                .update_task_lane(
                    &plan_id,
                    &task_id,
                    TaskLaneUpdate {
                        tab_id: Some(Some("remaining-tab".into())),
                        ..Default::default()
                    },
                    28,
                )
                .unwrap(),
            1 => store
                .update_task_lane(
                    &plan_id,
                    &task_id,
                    TaskLaneUpdate {
                        worktree_path: Some(Some("/remaining/worktree".into())),
                        ..Default::default()
                    },
                    28,
                )
                .unwrap(),
            2 => store
                .update_task_lane(
                    &plan_id,
                    &task_id,
                    TaskLaneUpdate {
                        branch: Some(Some("lane/remaining".into())),
                        ..Default::default()
                    },
                    28,
                )
                .unwrap(),
            3 => store
                .update_task_lane(
                    &plan_id,
                    &task_id,
                    TaskLaneUpdate {
                        workspace_id: Some(Some("workspace".into())),
                        workspace_name: Some(Some("remaining workspace".into())),
                        ..Default::default()
                    },
                    28,
                )
                .unwrap(),
            4 => store
                .update_task_lane(
                    &plan_id,
                    &task_id,
                    TaskLaneUpdate {
                        cleanup_status: Some(Some("deferred".into())),
                        ..Default::default()
                    },
                    28,
                )
                .unwrap(),
            5 => store
                .assign_task(&plan_id, &task_id, "peer-worker", Some("peer"), 28)
                .unwrap(),
            _ => store
                .update_task_lane(
                    &plan_id,
                    &task_id,
                    TaskLaneUpdate {
                        tab_id: Some(Some("remaining-owner".into())),
                        worktree_path: Some(Some("/remaining/worktree".into())),
                        branch: Some(Some("lane/remaining".into())),
                        cleanup_status: Some(Some("done".into())),
                        ..Default::default()
                    },
                    28,
                )
                .unwrap(),
        }
        assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 29);
    }
}

#[test]
fn review_bounce_cleaned_refuses_competitive_history() {
    let (dir, store, plan_id, task_id, _) = two_reported_cleaned_generations();
    let draft = TaskCompeteDraft {
        plan_id: plan_id.clone(),
        task_id: task_id.clone(),
        competition_group_id: "competition-group".into(),
        lane: 1,
        provider: "codex".into(),
        planned_model: Some("codex-test".into()),
        model: Some("codex-test".into()),
        reasoning_effort: Some("high".into()),
        parent_tab_id: Some("orchestrator".into()),
        workspace_id: None,
        title: Some("competitor".into()),
        cwd: dir.path().display().to_string(),
        branch: None,
        base: None,
        destination: None,
        task_prompt: "fixture".into(),
        report_command: "fixture".into(),
        plan_skill_manifest: vec![],
    };
    let competitive = store
        .record_task_compete_attempt(&plan_id, &task_id, "competitor", &draft, None, 28)
        .unwrap();
    store
        .judge_task_attempt(
            &plan_id,
            &task_id,
            &competitive.attempt.id,
            "rejected",
            Some("synthetic competitive history"),
            false,
            29,
        )
        .unwrap();
    set_status_label(&store, &plan_id, &task_id, "review", 30);
    assert!(store
        .show_task(&task_id)
        .unwrap()
        .task
        .execution_attempts
        .last()
        .unwrap()
        .closed_at
        .is_some());

    assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 31);
}

fn incomplete_terminal_history(case: &str) -> (tempfile::TempDir, PlanStore, String, String) {
    if case == "missing_retirement" {
        let (dir, store, plan_id, task_id, attempt_id) = one_reported_cleaned_generation();
        let before = store.show_task(&task_id).unwrap().task;
        let terminal = before.execution_attempts.last().unwrap();
        assert_eq!(before.status, TaskStatus::InReview);
        assert_eq!(before.status_label.as_deref(), Some("review"));
        assert_eq!(before.assigned_tab_id, None);
        assert_eq!(before.taken_by_role, None);
        assert_eq!(before.peer, None);
        assert_eq!(before.lane.tab_id, None);
        assert_eq!(before.lane.worktree_path, None);
        assert_eq!(before.lane.branch, None);
        assert_eq!(before.lane.workspace_id, None);
        assert_eq!(before.lane.workspace_name, None);
        assert_eq!(before.lane.cleanup_status.as_deref(), Some("done"));
        assert_eq!(before.interruption.owner_tab_id, None);
        assert_eq!(before.interruption.owner_session_id, None);
        assert!(!before.interruption.handoff_required);
        assert_eq!(before.execution_attempts.len(), 1);
        assert_eq!(terminal.id, attempt_id);
        assert!(terminal
            .session_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()));
        assert!(terminal
            .session_binding_nonce
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()));
        assert!(terminal.acknowledged_at.is_some());
        assert!(terminal.reported_at.is_some());
        assert_eq!(
            terminal.report.as_ref().unwrap().status.as_deref(),
            Some("in_review")
        );
        assert!(terminal.report.as_ref().unwrap().reporter_tab_id.is_none());
        assert!(terminal.closed_at.is_some());
        assert!(terminal.retired_at.is_some());

        // Negative fixture corruption isolates missing persisted retirement.
        // Never use this mutation to repair a live task.
        store
            .with_plan(&plan_id, 16, |plan| {
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .unwrap();
                let terminal = task.execution_attempts.last_mut().unwrap();
                assert_eq!(terminal.id, attempt_id);
                assert!(terminal.closed_at.is_some());
                assert!(terminal.retired_at.is_some());
                terminal.retired_at = None;
                Ok(())
            })
            .unwrap();
        let after = store.show_task(&task_id).unwrap().task;
        let mut expected = before.execution_attempts[0].clone();
        expected.retired_at = None;
        assert_eq!(after.execution_attempts[0], expected);
        return (dir, store, plan_id, task_id);
    }

    let (dir, store, plan_id, task_id) = setup();
    let with_runtime_binding = case != "missing_session";
    let attempt_id = add_unreported_generation(
        &dir,
        &store,
        &plan_id,
        &task_id,
        "worker-one",
        1,
        with_runtime_binding,
    );
    match case {
        "missing_report" => {
            store
                .acknowledge_task_attempt(&plan_id, &task_id, "worker-one", &attempt_id, 11)
                .unwrap();
            set_status(&store, &plan_id, &task_id, "in_review", 12);
        }
        "missing_ack" => {
            store
                .ingest_bound_worker_report(
                    &plan_id,
                    &task_id,
                    &AgentRef::from_tab("worker-one"),
                    BoundWorkerReport {
                        message: "report without an ACK".into(),
                        status: Some(ReportStatus::InReview),
                        sha: Some("abcdef123456".into()),
                        authority: Some(report_authority(&store, &task_id, "worker-one")),
                        ..Default::default()
                    },
                    12,
                )
                .unwrap();
        }
        "missing_session" => {
            store
                .acknowledge_task_attempt(&plan_id, &task_id, "worker-one", &attempt_id, 11)
                .unwrap();
            store
                .ingest_bound_worker_report(
                    &plan_id,
                    &task_id,
                    &AgentRef::from_tab("worker-one"),
                    BoundWorkerReport {
                        message: "sessionless report".into(),
                        status: Some(ReportStatus::InReview),
                        sha: Some("abcdef123456".into()),
                        ..Default::default()
                    },
                    12,
                )
                .unwrap();
        }
        "non_review_report" => {
            store
                .acknowledge_task_attempt(&plan_id, &task_id, "worker-one", &attempt_id, 11)
                .unwrap();
            store
                .record_task_attempt_report(
                    &plan_id,
                    &task_id,
                    &AgentRef::from_tab("worker-one"),
                    "report did not enter review",
                    Some(ReportStatus::Partial),
                    12,
                )
                .unwrap();
            set_status(&store, &plan_id, &task_id, "in_review", 13);
        }
        _ => unreachable!(),
    }
    let retire_at = if case == "non_review_report" { 14 } else { 13 };
    retire_and_clean(&store, &plan_id, &task_id, TaskStatus::InReview, retire_at);
    set_status_label(&store, &plan_id, &task_id, "review", retire_at + 2);
    (dir, store, plan_id, task_id)
}

#[test]
fn review_bounce_cleaned_refuses_invalid_generation_order() {
    for case in 0..5 {
        let (_dir, store, plan_id, task_id, _) = two_reported_cleaned_generations();
        store
            .with_plan(&plan_id, 28, |plan| {
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .unwrap();
                match case {
                    0 => {
                        task.execution_attempts[1].assigned_at =
                            task.execution_attempts[0].assigned_at;
                    }
                    1 => {
                        task.execution_attempts[1].assigned_at =
                            Some(task.execution_attempts[0].assigned_at.unwrap() - 1);
                    }
                    2 => {
                        task.execution_attempts[1].id = task.execution_attempts[0].id.clone();
                    }
                    3 => {
                        task.execution_attempts[1].session_id =
                            task.execution_attempts[0].session_id.clone();
                    }
                    _ => {
                        task.execution_attempts[0].closed_at =
                            Some(task.execution_attempts[1].assigned_at.unwrap() + 1);
                    }
                }
                Ok(())
            })
            .unwrap();
        assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 29);
    }
}

#[test]
fn review_bounce_cleaned_refuses_incomplete_terminal_report() {
    for case in [
        "missing_report",
        "missing_ack",
        "missing_session",
        "non_review_report",
        "missing_retirement",
    ] {
        let (_dir, store, plan_id, task_id) = incomplete_terminal_history(case);
        assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 20);
    }
}

#[test]
fn review_bounce_cleaned_refuses_later_failed_generation() {
    let (dir, store, plan_id, task_id) = setup();
    let earlier_report = add_reported_generation(&dir, &store, &plan_id, &task_id, "worker-one", 1);
    retire_and_clean(&store, &plan_id, &task_id, TaskStatus::Pending, 13);
    let failed = add_unreported_generation(&dir, &store, &plan_id, &task_id, "worker-two", 2, true);
    set_status(&store, &plan_id, &task_id, "in_review", 22);
    retire_and_clean(&store, &plan_id, &task_id, TaskStatus::InReview, 25);
    set_status_label(&store, &plan_id, &task_id, "review", 27);
    let task = store.show_task(&task_id).unwrap().task;
    assert!(task.execution_attempts[0].report.is_some());
    assert_eq!(task.execution_attempts[1].id, failed);
    assert_eq!(task.execution_attempts[1].report, None);
    assert_ne!(task.execution_attempts[0].id, task.execution_attempts[1].id);
    assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 28);
    assert_eq!(
        store.show_task(&task_id).unwrap().task.execution_attempts[0].id,
        earlier_report
    );
}

#[test]
fn review_bounce_cleaned_refuses_replay_conflict() {
    let (_dir, store, plan_id, task_id, _) = one_reported_cleaned_generation();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "first cleaned bounce",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            16,
        )
        .unwrap();
    set_status_and_label(
        &store,
        &plan_id,
        &task_id,
        "in_review",
        "review bounced",
        17,
    );

    assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 18);

    let (_dir, store, plan_id, task_id, _) = one_reported_cleaned_generation();
    set_status_label(&store, &plan_id, &task_id, "awaiting_continuation", 16);
    assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 17);

    let (_dir, store, plan_id, task_id, _) = one_reported_cleaned_generation();
    store
        .with_plan(&plan_id, 16, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            let round = task.review_round + 1;
            let attempt = task.execution_attempts.last_mut().unwrap();
            attempt.continuation.epoch = 1;
            attempt.continuation.review_round = Some(round);
            attempt.continuation.state = Some("continued".into());
            attempt.continuation.milestone_id = Some("prior-review-round".into());
            attempt.continuation.delivered_at = Some(14);
            Ok(())
        })
        .unwrap();

    assert_review_bounce_refused_unchanged(&store, &plan_id, &task_id, 17);
}

#[test]
fn review_bounce_cleaned_pending_replay_preserves_state() {
    let (_dir, store, plan_id, task_id, _) = one_reported_cleaned_generation();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "first cleaned bounce",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            16,
        )
        .unwrap();
    let before = store.get(&plan_id).unwrap();
    let error = store
        .bounce_review(
            &plan_id,
            &task_id,
            "pending retry remains refused",
            true,
            false,
            AgentRef::from_tab("reviewer"),
            17,
        )
        .unwrap_err();

    assert!(matches!(error, PlanError::ReviewNotInReview));
    assert_eq!(store.get(&plan_id).unwrap(), before);
}
