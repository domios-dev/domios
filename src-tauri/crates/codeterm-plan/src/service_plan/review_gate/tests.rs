use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskStatus};
use crate::service_plan::NewTask;

fn setup() -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Review".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("orch"),
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
        now,
    )
}

fn dispatch(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    pane: &str,
    now: u64,
) -> Result<(), PlanError> {
    store.assign_task(plan_id, task_id, pane, None, now)?;
    set_status(store, plan_id, task_id, "in_progress", now)
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

    dispatch(&store, &plan_id, &a, "pane_a", 3).unwrap();
    dispatch(&store, &plan_id, &b, "pane_b", 3).unwrap();
    set_status(&store, &plan_id, &a, "in_review", 4).unwrap();
    set_status(&store, &plan_id, &b, "in_review", 4).unwrap();

    let gate = store.review_gate_status(&plan_id).unwrap();
    assert!(gate.blocked);
    assert_eq!(gate.wip_count, 2);
    assert_eq!(gate.wip_limit, 2);

    let err = dispatch(&store, &plan_id, &c, "pane_c", 5).unwrap_err();
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
            AgentRef::from_pane("rev"),
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

    dispatch(&store, &plan_id, &c, "pane_c", 7).unwrap();
}

#[test]
fn stranded_debt_allows_only_explicit_review_capacity_work() {
    let (_dir, store, plan_id) = setup();
    let review = add_ready_task(&store, &plan_id, "review", 2);
    let implementation = add_ready_task(&store, &plan_id, "implementation", 2);
    store
        .with_plan(&plan_id, 3, |plan| {
            plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
            plan.tasks
                .iter_mut()
                .find(|task| task.id == review)
                .unwrap()
                .tags
                .push(REVIEW_CAPACITY_TAG.into());
            Ok(())
        })
        .unwrap();

    dispatch(&store, &plan_id, &review, "review_tab", 4).unwrap();
    let error = dispatch(&store, &plan_id, &implementation, "implementation_tab", 5).unwrap_err();
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
        .with_plan(&plan_id, 3, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == capacity_task)
                .unwrap()
                .kind = "review".into();
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

    let error = dispatch(&store, &plan_id, &capacity_task, "review_tab", 4).unwrap_err();
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
            plan.tasks
                .iter_mut()
                .find(|task| task.id == review)
                .unwrap()
                .tags
                .push(REVIEW_CAPACITY_TAG.into());
            Ok(())
        })
        .unwrap();

    store
        .set_task_assignment(&review, Some("review_tab"), Some("worker"), 4)
        .unwrap();
    let error = store
        .set_task_assignment(
            &implementation,
            Some("implementation_tab"),
            Some("worker"),
            5,
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
            AgentRef::from_pane("orch"),
            5,
        ),
        Err(PlanError::TransitionDenied { .. })
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
            AgentRef::from_pane("orch"),
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
    assert!(stride.text.contains("-5 -> 0"));

    let repeated = store
        .reconcile_review_credit(&plan_id, "same incident", AgentRef::from_pane("orch"), 4)
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
            AgentRef::from_pane("orch"),
            6,
        )
        .unwrap();
    assert_eq!(first.reconciled_credit, -1);
    assert!(first.stride_id.is_some());
    let second = store
        .reconcile_review_credit(&plan_id, "same incident", AgentRef::from_pane("orch"), 7)
        .unwrap();
    assert_eq!(second.reconciled_credit, -1);
    assert_eq!(second.stride_id, None);
    assert_eq!(store.get(&plan_id).unwrap().strides.len(), 1);
}

#[test]
fn bounce_review_atomically_exits_in_review_and_leaves_no_stale_qa_claim() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "bouncy", 2);
    dispatch(&store, &plan_id, &task_id, "pane_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();

    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "needs fixes",
            false,
            false,
            AgentRef::from_pane("rev"),
            5,
        )
        .unwrap();
    assert_eq!(result.review_round, 1);

    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::InProgress);
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
fn review_round_bound_rejects_third_round_without_override_and_accepts_with_one() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "deep-review", 2);
    dispatch(&store, &plan_id, &task_id, "pane_impl", 3).unwrap();

    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1 findings",
            false,
            false,
            AgentRef::from_pane("rev"),
            5,
        )
        .unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 6).unwrap();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 2 findings",
            false,
            false,
            AgentRef::from_pane("rev"),
            7,
        )
        .unwrap();

    // Third round: default max is 2, so this must be rejected without an override.
    let err = set_status(&store, &plan_id, &task_id, "in_review", 8).unwrap_err();
    assert!(matches!(
        err,
        PlanError::ReviewRoundExceeded {
            round: 2,
            max_rounds: 2
        }
    ));

    let err = store.record_review_override(&plan_id, &task_id, "", "reason", "repro", 9);
    assert!(matches!(err, Err(PlanError::ReviewOverrideIncomplete)));

    store
        .record_review_override(
            &plan_id,
            &task_id,
            "andrey",
            "P0 hotfix",
            "steps to repro",
            9,
        )
        .unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 10).unwrap();

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
    dispatch(&store, &plan_id, &task_id, "pane_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1 findings",
            false,
            false,
            AgentRef::from_pane("rev"),
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

    let err = set_status(&store, &plan_id, &task_id, "in_review", 7).unwrap_err();
    assert!(matches!(err, PlanError::ReviewOracleScopeExpansion));

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
fn accept_review_past_round_bound_creates_follow_up_and_closes_task() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "residual-risk", 2);
    dispatch(&store, &plan_id, &task_id, "pane_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1",
            false,
            false,
            AgentRef::from_pane("rev"),
            5,
        )
        .unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 6).unwrap();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 2",
            false,
            false,
            AgentRef::from_pane("rev"),
            7,
        )
        .unwrap();
    store
        .record_review_override(&plan_id, &task_id, "andrey", "P0", "repro", 8)
        .unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 9).unwrap();

    let result = store
        .accept_review(
            &plan_id,
            &task_id,
            "tests pass",
            false,
            false,
            Some("edge case Z untested"),
            AgentRef::from_pane("rev"),
            10,
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
    dispatch(&store, &plan_id, &task_id, "pane_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();

    let escalated_first = store
        .escalate_stale_review(&plan_id, &task_id, 1000, AgentRef::from_pane("orch"), 2000)
        .unwrap();
    assert!(escalated_first);

    let escalated_again = store
        .escalate_stale_review(&plan_id, &task_id, 1000, AgentRef::from_pane("orch"), 2500)
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
            AgentRef::from_pane("orch"),
            5000,
        )
        .unwrap();
    assert!(!not_stale_yet);
}

#[test]
fn qa_waves_group_compatible_tasks_by_shared_group() {
    let (_dir, store, plan_id) = setup();
    store
        .with_plan(&plan_id, 1, |p| {
            p.task_groups = vec![crate::plan::TaskGroup {
                key: "phase-1".into(),
                label: "Phase 1".into(),
                color: None,
            }];
            Ok(())
        })
        .unwrap();
    let a = store
        .add_task(
            &plan_id,
            NewTask {
                name: "a".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("Oracle: x".into()),
                group: Some("phase-1".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let b = store
        .add_task(
            &plan_id,
            NewTask {
                name: "b".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("Oracle: y".into()),
                group: Some("phase-1".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    for t in [&a, &b] {
        dispatch(&store, &plan_id, t, &format!("pane_{t}"), 3).unwrap();
        set_status(&store, &plan_id, t, "in_review", 4).unwrap();
        store
            .enqueue_qa(&plan_id, t, "ready", AgentRef::from_pane("orch"), 5)
            .unwrap();
    }
    let waves = store.qa_waves(&plan_id).unwrap();
    assert_eq!(waves.len(), 1);
    assert_eq!(waves[0].group_key, "phase-1");
    assert_eq!(waves[0].task_ids.len(), 2);
    assert!(waves[0].task_ids.contains(&a));
    assert!(waves[0].task_ids.contains(&b));
}
