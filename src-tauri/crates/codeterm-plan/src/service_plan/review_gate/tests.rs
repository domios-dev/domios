use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskStatus};
use crate::service_plan::NewTask;

fn setup() -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Review".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    (dir, store, plan.id)
}

fn add_ready_task(store: &PlanStore, plan_id: &str, name: &str, now: u64) -> String {
    store
        .add_task(
            plan_id,
            NewTask {
                name: name.into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("Oracle: run cargo test x".into()),
                ..Default::default()
            },
            now,
        )
        .unwrap()
}

fn set_status(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    status: &str,
    now: u64,
) -> Result<(), PlanError> {
    store.update_task(
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
        now,
    )
}

fn dispatch(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    tab: &str,
    now: u64,
) -> Result<(), PlanError> {
    store.assign_task(plan_id, task_id, tab, None, now)?;
    set_status(store, plan_id, task_id, "in_progress", now)?;
    store.with_plan(plan_id, now, |plan| {
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        let attempt = task.execution_attempts.last_mut().unwrap();
        attempt.session_id = Some(format!("session-{tab}"));
        attempt.acknowledged_at = Some(now);
        attempt.reported_at = Some(now);
        attempt.report = Some(Default::default());
        Ok(())
    })
}

fn deliver_bounce(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    result: &ReviewTransitionResult,
    now: u64,
) {
    let continuation = result.continuation.as_ref().unwrap();
    let claimed = store
        .claim_review_continuation_delivery(
            plan_id,
            task_id,
            &continuation.attempt_id,
            continuation.epoch,
            now,
        )
        .unwrap();
    let task = store.show_task(task_id).unwrap().task;
    assert!(
        claimed,
        "status={:?} label={:?} continuation={:?}",
        task.status,
        task.status_label,
        task.execution_attempts
            .iter()
            .map(|attempt| (&attempt.id, &attempt.continuation))
            .collect::<Vec<_>>()
    );
    assert!(store
        .finish_review_continuation(
            plan_id,
            task_id,
            &continuation.attempt_id,
            continuation.epoch,
            true,
            now + 1,
        )
        .unwrap());
}

#[test]
fn evidence_required_to_enter_review() {
    let (_dir, store, plan_id) = setup();
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "bare".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let err = set_status(&store, &plan_id, &task_id, "in_review", 3).unwrap_err();
    assert!(matches!(err, PlanError::ReviewEvidenceRequired));
}

#[test]
fn exact_qa_evidence_permits_entry_and_fixes_oracle() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "ready", 2);
    set_status(&store, &plan_id, &task_id, "in_review", 3).unwrap();
    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::InReview);
    assert!(task.review_oracle.is_some());
    assert_eq!(plan.review_policy.credit, -1);
}

#[test]
fn accepted_reconcile_fix_repays_review_credit_exactly_once() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "accepted", 2);
    set_status(&store, &plan_id, &task_id, "in_review", 3).unwrap();
    let drift = [crate::plan::reconcile::DriftItem {
        plan_id: plan_id.clone(),
        task_id: task_id.clone(),
        kind: crate::plan::reconcile::DriftKind::AcceptedButNotDone,
        detail: "accepted".into(),
        suggested_fix: "set done".into(),
        not_mine: false,
    }];

    store.apply_reconcile_fixes(&plan_id, &drift, 4).unwrap();
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.review_policy.credit, 0);
    assert_eq!(
        plan.tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap()
            .status,
        TaskStatus::Done
    );

    store.apply_reconcile_fixes(&plan_id, &drift, 5).unwrap();
    assert_eq!(store.get(&plan_id).unwrap().review_policy.credit, 0);
}

#[test]
fn wip_limit_blocks_dispatch_and_closing_a_review_restores_capacity() {
    let (_dir, store, plan_id) = setup();
    store
        .with_plan(&plan_id, 1, |p| {
            p.review_policy.wip_limit = 2;
            Ok(())
        })
        .unwrap();

    let a = add_ready_task(&store, &plan_id, "a", 2);
    let b = add_ready_task(&store, &plan_id, "b", 2);
    let c = add_ready_task(&store, &plan_id, "c", 2);

    dispatch(&store, &plan_id, &a, "tab_a", 3).unwrap();
    dispatch(&store, &plan_id, &b, "tab_b", 3).unwrap();
    set_status(&store, &plan_id, &a, "in_review", 4).unwrap();
    set_status(&store, &plan_id, &b, "in_review", 4).unwrap();

    let gate = store.review_gate_status(&plan_id).unwrap();
    assert!(gate.blocked);
    assert_eq!(gate.wip_count, 2);
    assert_eq!(gate.wip_limit, 2);

    let err = dispatch(&store, &plan_id, &c, "tab_c", 5).unwrap_err();
    assert!(matches!(
        err,
        PlanError::ReviewWipExceeded {
            wip_count: 2,
            wip_limit: 2,
            ..
        }
    ));

    store
        .accept_review(
            &plan_id,
            &a,
            "looks good",
            false,
            false,
            None,
            AgentRef::from_tab("rev"),
            6,
        )
        .unwrap();

    let accepted = store.get(&plan_id).unwrap();
    let accepted_a = accepted.tasks.iter().find(|t| t.id == a).unwrap();
    assert_eq!(
        accepted_a.status,
        TaskStatus::Done,
        "accept closes the task in one step"
    );

    let gate = store.review_gate_status(&plan_id).unwrap();
    assert!(!gate.blocked, "accepting one review must free capacity");

    dispatch(&store, &plan_id, &c, "tab_c", 7).unwrap();
}

#[test]
fn stranded_debt_allows_only_explicit_review_capacity_work() {
    let (_dir, store, plan_id) = setup();
    let review = add_ready_task(&store, &plan_id, "review", 2);
    let implementation = add_ready_task(&store, &plan_id, "implementation", 2);
    store
        .with_plan(&plan_id, 3, |plan| {
            plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == review)
                .unwrap();
            task.kind = "review".into();
            task.tags.push("review-capacity".into());
            Ok(())
        })
        .unwrap();

    let spoofed = dispatch(&store, &plan_id, &review, "spoofed_tab", 4).unwrap_err();
    assert!(matches!(spoofed, PlanError::ReviewWipExceeded { .. }));
    store
        .grant_review_capacity(
            &plan_id,
            &review,
            ReviewCapacityClass::Review,
            AgentRef::from_tab("orch"),
            5,
        )
        .unwrap();
    dispatch(&store, &plan_id, &review, "review_tab", 6).unwrap();
    let error = dispatch(&store, &plan_id, &implementation, "implementation_tab", 7).unwrap_err();
    assert!(matches!(error, PlanError::ReviewWipExceeded { .. }));
}

#[test]
fn capacity_classification_does_not_bypass_actual_review_saturation() {
    let (_dir, store, plan_id) = setup();
    let capacity_task = add_ready_task(&store, &plan_id, "capacity", 2);
    let active_reviews = (0..5)
        .map(|index| add_ready_task(&store, &plan_id, &format!("review-{index}"), 2))
        .collect::<Vec<_>>();
    store
        .grant_review_capacity(
            &plan_id,
            &capacity_task,
            ReviewCapacityClass::CanonicalGate,
            AgentRef::from_tab("orch"),
            3,
        )
        .unwrap();
    store
        .with_plan(&plan_id, 4, |plan| {
            for task_id in &active_reviews {
                plan.tasks
                    .iter_mut()
                    .find(|task| task.id == *task_id)
                    .unwrap()
                    .status = TaskStatus::InReview;
            }
            plan.review_policy.credit = -(active_reviews.len() as i64);
            Ok(())
        })
        .unwrap();

    let error = dispatch(&store, &plan_id, &capacity_task, "review_tab", 5).unwrap_err();
    assert!(matches!(
        error,
        PlanError::ReviewWipExceeded {
            wip_count: 5,
            wip_limit: 5,
            ..
        }
    ));
}

#[test]
fn assignment_status_transition_uses_the_same_capacity_classification() {
    let (_dir, store, plan_id) = setup();
    let review = add_ready_task(&store, &plan_id, "review", 2);
    let implementation = add_ready_task(&store, &plan_id, "implementation", 2);
    store
        .with_plan(&plan_id, 3, |plan| {
            plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
            Ok(())
        })
        .unwrap();
    store
        .grant_review_capacity(
            &plan_id,
            &review,
            ReviewCapacityClass::Review,
            AgentRef::from_tab("orch"),
            4,
        )
        .unwrap();

    store
        .set_task_assignment(&review, Some("review_tab"), Some("worker"), 5)
        .unwrap();
    let error = store
        .set_task_assignment(
            &implementation,
            Some("implementation_tab"),
            Some("worker"),
            6,
        )
        .unwrap_err();
    assert!(matches!(error, PlanError::ReviewWipExceeded { .. }));
}

#[test]
fn generic_status_mutation_cannot_exit_review_or_leak_credit() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "reviewed", 2);
    dispatch(&store, &plan_id, &task_id, "implementation_tab", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();

    for status in ["in_progress", "cancelled", "done"] {
        let error = set_status(&store, &plan_id, &task_id, status, 5).unwrap_err();
        assert!(
            matches!(error, PlanError::TransitionDenied { .. }),
            "{status}: {error}"
        );
    }
    let blocked = store.update_task(
        &plan_id,
        &task_id,
        None,
        None,
        None,
        None,
        Some("blocked"),
        Some(Some("reason")),
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
        5,
    );
    assert!(matches!(blocked, Err(PlanError::TransitionDenied { .. })));
    assert!(matches!(
        store.pause_task(
            &plan_id,
            &task_id,
            "operator pause",
            AgentRef::from_tab("orch"),
            5,
        ),
        Err(PlanError::ReviewHoldRequired)
    ));
    assert!(matches!(
        store.rm_task(&plan_id, &task_id, 5),
        Err(PlanError::TransitionDenied { .. })
    ));
    assert!(matches!(
        store.rm_all_tasks(&plan_id, 5),
        Err(PlanError::TransitionDenied { .. })
    ));
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.review_policy.credit, -1);
    assert_eq!(
        plan.tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap()
            .status,
        TaskStatus::InReview
    );
}

#[test]
fn explicit_credit_reconcile_repairs_debt_and_records_a_stride() {
    let (_dir, store, plan_id) = setup();
    store
        .with_plan(&plan_id, 2, |plan| {
            plan.review_policy.credit = -5;
            Ok(())
        })
        .unwrap();

    let result = store
        .reconcile_review_credit(
            &plan_id,
            "repair historical leaked exits",
            AgentRef::from_tab("orch"),
            3,
        )
        .unwrap();
    assert_eq!(result.previous_credit, -5);
    assert_eq!(result.reconciled_credit, 0);
    assert_eq!(result.active_reviews, 0);
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.review_policy.credit, 0);
    let stride_id = result.stride_id.as_deref().unwrap();
    let stride = plan
        .strides
        .iter()
        .find(|stride| stride.id == stride_id)
        .unwrap();
    assert_eq!(stride.kind, REVIEW_CREDIT_RECONCILE_MILESTONE_KIND);
    assert_eq!(stride.from.tab_id.as_deref(), Some("orch"));
    assert!(stride.text.contains("-5 -> 0"));

    let repeated = store
        .reconcile_review_credit(&plan_id, "same incident", AgentRef::from_tab("orch"), 4)
        .unwrap();
    assert_eq!(repeated.stride_id, None);
    assert_eq!(store.get(&plan_id).unwrap().strides.len(), 1);
}

#[test]
fn credit_reconcile_preserves_active_review_floor_and_is_idempotent() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "active review", 2);
    dispatch(&store, &plan_id, &task_id, "worker", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .with_plan(&plan_id, 5, |plan| {
            plan.review_policy.credit = -5;
            Ok(())
        })
        .unwrap();

    let first = store
        .reconcile_review_credit(
            &plan_id,
            "repair leaked exits",
            AgentRef::from_tab("orch"),
            6,
        )
        .unwrap();
    assert_eq!(first.reconciled_credit, -1);
    assert!(first.stride_id.is_some());
    let second = store
        .reconcile_review_credit(&plan_id, "same incident", AgentRef::from_tab("orch"), 7)
        .unwrap();
    assert_eq!(second.reconciled_credit, -1);
    assert_eq!(second.stride_id, None);
    assert_eq!(store.get(&plan_id).unwrap().strides.len(), 1);
}

#[test]
fn capacity_grant_is_audited_immutable_and_idempotent() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "review", 2);
    let first = store
        .grant_review_capacity(
            &plan_id,
            &task_id,
            ReviewCapacityClass::Review,
            AgentRef::from_tab("orch"),
            3,
        )
        .unwrap();
    assert!(first.created);
    assert!(first.stride_id.is_some());

    let repeated = store
        .grant_review_capacity(
            &plan_id,
            &task_id,
            ReviewCapacityClass::Review,
            AgentRef::from_tab("orch"),
            4,
        )
        .unwrap();
    assert!(!repeated.created);
    assert_eq!(repeated.stride_id, None);
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.review_policy.capacity_grants.len(), 1);
    assert_eq!(
        plan.strides
            .iter()
            .filter(|stride| stride.kind == REVIEW_CAPACITY_GRANT_MILESTONE_KIND)
            .count(),
        1
    );
    assert!(store
        .grant_review_capacity(
            &plan_id,
            &task_id,
            ReviewCapacityClass::CanonicalGate,
            AgentRef::from_tab("orch"),
            5,
        )
        .is_err());
}

#[test]
fn bounce_review_atomically_exits_in_review_and_leaves_no_stale_qa_claim() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "bouncy", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();

    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "needs fixes",
            false,
            false,
            AgentRef::from_tab("rev"),
            5,
        )
        .unwrap();
    assert_eq!(result.review_round, 1);
    assert!(serde_json::to_value(&result)
        .unwrap()
        .get("strategy_warning")
        .is_none());

    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.status_label.as_deref(), Some("awaiting_continuation"));
    assert_eq!(task.review_stage, crate::plan::ReviewStage::Review);
    assert_eq!(task.qa_state, None);
    assert_eq!(task.review_round, 1);
    assert_eq!(task.review_evidence.len(), 1);
    assert_eq!(
        task.review_evidence[0].verdict,
        crate::plan::ReviewVerdict::Bounce
    );
    // Entering review spent one credit; the bounce closes that round and repays it.
    assert_eq!(plan.review_policy.credit, 0);
}

#[test]
fn review_hold_verdict_records_reason_and_keeps_review_open() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "held", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let result = store
        .hold_review(
            &plan_id,
            &task_id,
            "re-cut the scope before continuing",
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();

    let after = store.show_task(&task_id).unwrap().task;
    assert_eq!(result.verdict, ReviewVerdict::Hold);
    assert_eq!(after.status, TaskStatus::InReview);
    assert_eq!(after.status_label.as_deref(), Some("review held"));
    assert_eq!(
        after.review_evidence.last().unwrap().verdict,
        ReviewVerdict::Hold
    );
    assert_eq!(
        after.review_evidence.last().unwrap().note,
        "re-cut the scope before continuing"
    );
}

#[test]
fn review_hold_verdict_preserves_assignment_and_lane() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "held", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let before = store.show_task(&task_id).unwrap().task;

    store
        .hold_review(
            &plan_id,
            &task_id,
            "re-cut the scope before continuing",
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();

    let after = store.show_task(&task_id).unwrap().task;
    assert_eq!(after.assigned_tab_id, before.assigned_tab_id);
    assert_eq!(after.lane.tab_id, before.lane.tab_id);
}

#[test]
fn review_hold_verdict_can_bounce_to_the_existing_assignment() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "held", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .hold_review(
            &plan_id,
            &task_id,
            "awaiting scope recut",
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();

    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "scope recut is ready",
            false,
            false,
            AgentRef::from_tab("reviewer"),
            6,
        )
        .unwrap();

    assert_eq!(
        result.continuation.as_ref().unwrap().executor_tab_id,
        "tab_impl"
    );
    assert_eq!(
        store
            .show_task(&task_id)
            .unwrap()
            .task
            .assigned_tab_id
            .as_deref(),
        Some("tab_impl")
    );
}

#[test]
fn review_hold_verdict_pause_guard_survives() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "held", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();

    let pause_error = store
        .pause_task(
            &plan_id,
            &task_id,
            "operator pause",
            AgentRef::from_tab("orch"),
            5,
        )
        .unwrap_err();
    assert!(matches!(pause_error, PlanError::ReviewHoldRequired));
    assert!(pause_error.to_string().contains("hold"));
    assert!(matches!(
        set_status(&store, &plan_id, &task_id, "paused", 6),
        Err(PlanError::ReviewHoldRequired)
    ));
}

#[test]
fn review_bounce_transition_keeps_task_attempt_epoch_atomic() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "continuation", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();

    let before = store.show_task(&task_id).unwrap().task;
    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "repair exact defect",
            true,
            true,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    let awaiting = store.show_task(&task_id).unwrap().task;
    let continuation = result.continuation.unwrap();
    assert_eq!(awaiting.status, TaskStatus::InReview);
    assert_eq!(
        awaiting.status_label.as_deref(),
        Some("awaiting_continuation")
    );
    assert_eq!(
        awaiting.execution_attempts.len(),
        before.execution_attempts.len()
    );
    assert_eq!(continuation.attempt_id, before.execution_attempts[0].id);
    assert_eq!(
        awaiting.execution_attempts[0].acknowledged_at,
        before.execution_attempts[0].acknowledged_at
    );
    assert_eq!(awaiting.execution_attempts[0].continuation.epoch, 1);
    assert_eq!(
        awaiting.execution_attempts[0].continuation.review_round,
        Some(1)
    );
}

#[test]
fn review_bounce_replay_is_idempotent_for_same_review_round() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "continuation replay", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let first = store
        .bounce_review(
            &plan_id,
            &task_id,
            "fix",
            true,
            true,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    let replay = store
        .bounce_review(
            &plan_id,
            &task_id,
            "fix",
            true,
            true,
            AgentRef::from_tab("reviewer"),
            6,
        )
        .unwrap();
    assert_eq!(first.milestone_id, replay.milestone_id);
    assert_eq!(first.continuation, replay.continuation);
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.review_evidence.len(), 1);
    assert_eq!(task.execution_attempts[0].continuation.epoch, 1);
}

#[test]
fn third_trailing_stagnant_bounce_returns_strategy_warning_atomically() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "stagnant", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .with_plan(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            for at in [2, 3] {
                task.review_evidence.push(ReviewEvidence {
                    id: format!("rev_{at}"),
                    from: AgentRef::from_tab("reviewer"),
                    verdict: ReviewVerdict::Bounce,
                    note: "same approach".into(),
                    at,
                    changed_executable_surfaces: false,
                    distinct_oracle: false,
                });
                task.review_round += 1;
            }
            Ok(())
        })
        .unwrap();

    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "same approach still fails",
            false,
            false,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    let warning = result.strategy_warning.unwrap();
    assert_eq!(warning.consecutive_bounces, 3);
    assert_eq!(warning.review_round, 3);
    assert!(warning.message.contains("codeterm docs BOUNCE"));
}

#[test]
fn strategy_warning_resets_on_accept_changed_surface_or_distinct_oracle() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "resets", 2);
    let base = ReviewEvidence {
        id: "rev".into(),
        from: AgentRef::from_tab("reviewer"),
        verdict: ReviewVerdict::Bounce,
        note: "finding".into(),
        at: 1,
        changed_executable_surfaces: false,
        distinct_oracle: false,
    };
    let mut task = store.show_task(&task_id).unwrap().task;
    task.review_round = 4;
    task.review_evidence = vec![base.clone(), base.clone(), base.clone()];
    assert!(strategy_warning(&task).is_some());

    for reset in [
        ReviewEvidence {
            verdict: ReviewVerdict::Accept,
            ..base.clone()
        },
        ReviewEvidence {
            changed_executable_surfaces: true,
            ..base.clone()
        },
        ReviewEvidence {
            distinct_oracle: true,
            ..base.clone()
        },
    ] {
        task.review_evidence.push(reset);
        assert_eq!(strategy_warning(&task), None);
        task.review_evidence.pop();
    }
}

#[test]
fn review_round_bound_rejects_third_round_without_override_and_accepts_with_one() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "deep-review", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();

    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let first = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1 findings",
            false,
            false,
            AgentRef::from_tab("rev"),
            5,
        )
        .unwrap();
    deliver_bounce(&store, &plan_id, &task_id, &first, 6);
    set_status(&store, &plan_id, &task_id, "in_review", 8).unwrap();
    let second = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 2 findings",
            false,
            false,
            AgentRef::from_tab("rev"),
            9,
        )
        .unwrap();
    deliver_bounce(&store, &plan_id, &task_id, &second, 10);

    // Third round: default max is 2, so this must be rejected without an override.
    let err = set_status(&store, &plan_id, &task_id, "in_review", 12).unwrap_err();
    assert!(matches!(
        err,
        PlanError::ReviewRoundExceeded {
            round: 2,
            max_rounds: 2
        }
    ));

    let err = store.record_review_override(&plan_id, &task_id, "", "reason", "repro", 13);
    assert!(matches!(err, Err(PlanError::ReviewOverrideIncomplete)));

    store
        .record_review_override(
            &plan_id,
            &task_id,
            "andrey",
            "P0 hotfix",
            "steps to repro",
            14,
        )
        .unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 15).unwrap();

    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.review_override, None, "override is consumed on use");
    assert_eq!(task.review_audit.len(), 1);
    assert_eq!(task.review_audit[0].owner, "andrey");
    assert_eq!(task.review_audit[0].round, 2);
}

#[test]
fn oracle_scope_expansion_rejected_without_override() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "oracle-lock", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1 findings",
            false,
            false,
            AgentRef::from_tab("rev"),
            5,
        )
        .unwrap();

    // Expand scope: add a new acceptance criterion after the oracle was fixed.
    store
        .with_plan(&plan_id, 6, |p| {
            let task = p.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
            task.acceptance_criteria
                .push("also handles edge case Y".into());
            Ok(())
        })
        .unwrap();

    let before_rejection = store.show_task(&task_id).unwrap().task;
    let err = set_status(&store, &plan_id, &task_id, "in_review", 7).unwrap_err();
    assert!(matches!(err, PlanError::ReviewOracleScopeExpansion));
    let after_rejection = store.show_task(&task_id).unwrap().task;
    assert_eq!(after_rejection.status, before_rejection.status);
    assert_eq!(
        after_rejection.review_oracle,
        before_rejection.review_oracle
    );

    store
        .record_review_override(
            &plan_id,
            &task_id,
            "andrey",
            "release-impact fix",
            "repro steps",
            8,
        )
        .unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 9).unwrap();
}

#[test]
fn authorized_correction_rebuilds_oracle_without_rewriting_history() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "authorized-correction", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let bounce = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1 findings",
            false,
            false,
            AgentRef::from_tab("rev"),
            5,
        )
        .unwrap();
    deliver_bounce(&store, &plan_id, &task_id, &bounce, 6);

    store
        .with_plan(&plan_id, 8, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.acceptance_criteria = vec![
                "corrected acceptance".into(),
                "preserves immutable history".into(),
            ];
            task.test_plan = Some("cargo test corrected_oracle".into());
            Ok(())
        })
        .unwrap();
    store
        .record_review_override(
            &plan_id,
            &task_id,
            "andrey",
            "authorized correction",
            "reproduced stale oracle",
            9,
        )
        .unwrap();

    let before = store.show_task(&task_id).unwrap().task;
    let stored = before.review_oracle.as_ref().unwrap();
    let current = oracle_signature(&before);
    assert_ne!(stored, &current);
    let reports_before = before.execution_attempts.clone();
    let milestones_before = before.milestones.clone();
    let audit_before = before.review_audit.clone();

    set_status(&store, &plan_id, &task_id, "in_review", 10).unwrap();

    let after = store.show_task(&task_id).unwrap().task;
    assert_eq!(after.status, TaskStatus::InReview);
    assert_eq!(
        after.review_oracle.as_deref(),
        Some(oracle_signature(&after).as_str())
    );
    assert_eq!(after.execution_attempts, reports_before);
    assert_eq!(after.milestones, milestones_before);
    assert_eq!(after.review_audit, audit_before);
}

#[test]
fn accept_review_past_round_bound_creates_follow_up_and_closes_task() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "residual-risk", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let first = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1",
            false,
            false,
            AgentRef::from_tab("rev"),
            5,
        )
        .unwrap();
    deliver_bounce(&store, &plan_id, &task_id, &first, 6);
    set_status(&store, &plan_id, &task_id, "in_review", 8).unwrap();
    let second = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 2",
            false,
            false,
            AgentRef::from_tab("rev"),
            9,
        )
        .unwrap();
    deliver_bounce(&store, &plan_id, &task_id, &second, 10);
    store
        .record_review_override(&plan_id, &task_id, "andrey", "P0", "repro", 12)
        .unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 13).unwrap();

    let result = store
        .accept_review(
            &plan_id,
            &task_id,
            "tests pass",
            false,
            false,
            Some("edge case Z untested"),
            AgentRef::from_tab("rev"),
            14,
        )
        .unwrap();
    assert!(result.follow_up_task_id.is_some());

    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(
        task.status,
        TaskStatus::Done,
        "accept closes the task even past the round bound"
    );
    let follow_up_id = result.follow_up_task_id.unwrap();
    let follow_up = plan.tasks.iter().find(|t| t.id == follow_up_id).unwrap();
    assert_eq!(
        follow_up.status,
        TaskStatus::Pending,
        "residual risk stays open as a separate follow-up"
    );
    assert_eq!(follow_up.parent_task_id.as_deref(), Some(task_id.as_str()));
    assert!(follow_up.body.contains("edge case Z untested"));
}

#[test]
fn stale_review_escalation_is_idempotent() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "aging", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();

    let escalated_first = store
        .escalate_stale_review(&plan_id, &task_id, 1000, AgentRef::from_tab("orch"), 2000)
        .unwrap();
    assert!(escalated_first);

    let escalated_again = store
        .escalate_stale_review(&plan_id, &task_id, 1000, AgentRef::from_tab("orch"), 2500)
        .unwrap();
    assert!(
        !escalated_again,
        "already escalated since this review entry"
    );

    let not_stale_yet = store
        .escalate_stale_review(
            &plan_id,
            &task_id,
            100_000,
            AgentRef::from_tab("orch"),
            5000,
        )
        .unwrap();
    assert!(!not_stale_yet);
}

mod bounce_ref_resolution;
mod qa_waves;
