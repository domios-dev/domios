use super::*;
use codeterm_service_core::plan::OrchestratorRef;
use codeterm_service_core::service::plan::{NewTask, PlanStore};

fn continuation_store() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "continuation".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                acceptance_criteria: vec!["fixed".into()],
                test_plan: Some("oracle".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "worker", None, 3)
        .unwrap();
    let mut stored = store.get(&plan.id).unwrap();
    let task = stored
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = codeterm_service_core::plan::TaskStatus::InReview;
    let attempt = task.execution_attempts.last_mut().unwrap();
    let attempt_id = attempt.id.clone();
    attempt.session_id = Some("worker-session".into());
    attempt.acknowledged_at = Some(3);
    attempt.reported_at = Some(4);
    attempt.report = Some(Default::default());
    store.save(&mut stored, 4).unwrap();
    (dir, store, plan.id, task_id, attempt_id)
}

#[test]
fn review_bounce_finish_false_is_an_uncertain_completion_refusal() {
    let error = finish_continuation_result(false, Ok(()), "plan-1", "task-1", "attempt-1", 2)
        .expect_err("a no-op finish must never serialize success");
    assert_eq!(error.code, -32603);
    let data = error.data.unwrap();
    assert_eq!(data["type"], "bounce_continuation_finish_refused");
    assert_eq!(data["delivery_succeeded"], true);
    assert_eq!(data["uncertain_completion"], true);
}

#[test]
fn review_bounce_finish_success_and_delivery_failure_keep_distinct_results() {
    assert!(finish_continuation_result(true, Ok(()), "plan-1", "task-1", "attempt-1", 1,).is_ok());

    let error = finish_continuation_result(
        true,
        Err("delivery unavailable".into()),
        "plan-1",
        "task-1",
        "attempt-1",
        1,
    )
    .expect_err("failed delivery must remain an explicit failure");
    let data = error.data.unwrap();
    assert_eq!(data["type"], "bounce_continuation_failed");
    assert_eq!(data["error"], "delivery unavailable");
    assert!(data.get("uncertain_completion").is_none());
}

#[test]
fn review_bounce_reopens_exact_acknowledged_attempt_and_continues_once() {
    let (_dir, store, plan_id, task_id, attempt_id) = continuation_store();
    let before = store.inspect_task_for_test(&task_id).unwrap().task;
    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "repair only this defect",
            true,
            true,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    let continuation = result.continuation.unwrap();
    assert_eq!(continuation.attempt_id, attempt_id);
    assert!(store
        .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 6)
        .is_ok());
    let refusal = store
        .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 7)
        .expect_err("second claim must be an explicit refusal");
    assert!(matches!(
        refusal,
        codeterm_service_core::service::plan::PlanError::ReviewContinuationDeliveryRefused {
            expected_epoch: 1,
            actual_epoch: Some(1),
            continuation_state: Some(ref state),
            ..
        } if state == "continuation_delivering"
    ));
    assert!(store
        .finish_review_continuation(&plan_id, &task_id, &attempt_id, 1, true, 8)
        .unwrap());
    let after = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(
        after.status,
        codeterm_service_core::plan::TaskStatus::InProgress
    );
    assert_eq!(
        after.execution_attempts.len(),
        before.execution_attempts.len()
    );
    assert_eq!(after.execution_attempts[0].acknowledged_at, Some(3));
    assert_eq!(
        after.execution_attempts[0].continuation.delivered_at,
        Some(8)
    );
}

#[test]
fn review_bounce_delivery_failure_stays_visible_and_never_fakes_in_progress() {
    let (_dir, store, plan_id, task_id, attempt_id) = continuation_store();
    store
        .bounce_review(
            &plan_id,
            &task_id,
            "repair",
            true,
            true,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    assert!(store
        .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 6)
        .is_ok());
    assert!(store
        .finish_review_continuation(&plan_id, &task_id, &attempt_id, 1, false, 7)
        .unwrap());
    let failed = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(
        failed.status,
        codeterm_service_core::plan::TaskStatus::InReview
    );
    assert_eq!(
        failed.status_label.as_deref(),
        Some("bounce_continuation_failed")
    );
    assert_eq!(
        failed.execution_attempts[0].continuation.state.as_deref(),
        Some("bounce_continuation_failed")
    );
}

#[test]
fn review_bounce_fences_stale_rebound_replacement_and_competition_attempts() {
    let (_dir, store, plan_id, task_id, _attempt_id) = continuation_store();
    let mut plan = store.get(&plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.assigned_tab_id = Some("replacement".into());
    store.save(&mut plan, 5).unwrap();
    let before = store.inspect_task_for_test(&task_id).unwrap().task;
    assert!(store
        .bounce_review(
            &plan_id,
            &task_id,
            "must not deliver",
            true,
            true,
            AgentRef::from_tab("reviewer"),
            6,
        )
        .is_err());
    assert_eq!(store.inspect_task_for_test(&task_id).unwrap().task, before);
}

#[test]
fn review_bounce_replay_is_idempotent_for_same_review_round() {
    let (_dir, store, plan_id, task_id, attempt_id) = continuation_store();
    let (first, first_recorded) = store
        .bounce_review_with_recorded_status(
            &plan_id,
            &task_id,
            "repair",
            true,
            true,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    assert!(first_recorded);
    let (replay, replay_recorded) = store
        .bounce_review_with_recorded_status(
            &plan_id,
            &task_id,
            "repair",
            true,
            true,
            AgentRef::from_tab("reviewer"),
            6,
        )
        .unwrap();
    assert!(!replay_recorded);
    assert_eq!(first.milestone_id, replay.milestone_id);
    assert_eq!(first.continuation, replay.continuation);
    assert!(store
        .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 7)
        .is_ok());
    let refusal = store
        .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 8)
        .expect_err("replayed claim must be an explicit refusal");
    assert!(matches!(
        refusal,
        codeterm_service_core::service::plan::PlanError::ReviewContinuationDeliveryRefused { .. }
    ));
}

pub(super) fn socket_bounce_fixture(
    worker_present: bool,
) -> (
    codeterm_service_core::service::test_fixture::ServiceFixture,
    String,
    String,
    String,
) {
    let (mut mux, default_tab_id) =
        codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let owner_tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&owner_tab_id).unwrap();
    let owner_team_id = mux
        .resolve_or_mint_team_for_parent(&owner_tab_id)
        .unwrap()
        .team_id;
    {
        let owner = mux.get_tab_mut(&owner_tab_id).unwrap();
        owner.agent_session_id = Some("review-owner-session".into());
        owner.session_binding_nonce = Some("review-owner-nonce".into());
        owner.reported_session_binding_nonce = Some("review-owner-nonce".into());
        owner.team_id = Some(owner_team_id.clone());
    }
    let worker_tab_id = if worker_present {
        let tab = mux.get_tab_mut(&default_tab_id).unwrap();
        tab.agent_type = Some("codex".into());
        tab.agent_session_id = Some("worker-session-rotated".into());
        tab.pty_id = Some("worker-pty".into());
        tab.exit_suspected_at = Some(std::time::Instant::now());
        default_tab_id
    } else {
        "worker-tab-absent".into()
    };
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "socket bounce".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner_tab_id),
            session_id: Some("review-owner-session".into()),
            title: None,
        },
    );
    plan.team_id = Some(owner_team_id);
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker task".into(),
                acceptance_criteria: vec!["fixed".into()],
                test_plan: Some("verify the retry".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, &worker_tab_id, None, 3)
        .unwrap();
    let lane_path = fixture.temp.path().join("worker-lane");
    let lane_path = lane_path.to_string_lossy().into_owned();
    let mut stored = store.get(&plan.id).unwrap();
    let task = stored
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = codeterm_service_core::plan::TaskStatus::InReview;
    task.lane = codeterm_service_core::plan::TaskLaneMetadata {
        tab_id: Some(worker_tab_id),
        worktree_path: Some(lane_path.clone()),
        branch: Some("lane/socket-bounce".into()),
        ..Default::default()
    };
    let attempt = task.execution_attempts.last_mut().unwrap();
    attempt.session_id = Some("worker-session".into());
    attempt.acknowledged_at = Some(3);
    attempt.reported_at = Some(4);
    attempt.report = Some(Default::default());
    attempt.worktree_path = Some(lane_path.clone());
    store.save(&mut stored, 4).unwrap();
    (fixture, plan.id, task_id, lane_path)
}

fn dispatch_bounce(
    fixture: &codeterm_service_core::service::test_fixture::ServiceFixture,
    plan_id: &str,
    task_id: &str,
) -> Result<Value, JsonRpcError> {
    let owner_tab_id = fixture
        .service
        .plan_store
        .get(plan_id)
        .and_then(|plan| plan.owner.tab_id)
        .expect("fixture Plan owner");
    let params = Some(json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "note": "retry the same lane",
        "caller_tab_id": owner_tab_id,
        "session_id": "review-owner-session",
        "session_binding_nonce": "review-owner-nonce",
    }));

    let svc = std::sync::Arc::new(fixture.service.clone());
    dispatch("plan.review.bounce", &params, &svc)
}

#[test]
fn plan_review_reopens_when_executor_tab_is_absent() {
    let (fixture, plan_id, task_id, lane_path) = socket_bounce_fixture(false);
    dispatch_bounce(&fixture, &plan_id, &task_id).unwrap();

    let task = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    assert_eq!(
        task.status,
        codeterm_service_core::plan::TaskStatus::Pending
    );
    assert_eq!(task.lane.worktree_path.as_deref(), Some(lane_path.as_str()));
    assert!(task.execution_attempts[0].closed_at.is_some());
    assert_eq!(
        task.execution_attempts[0].closure_reason.as_deref(),
        Some("review_bounce_executor_gone")
    );
    assert_eq!(
        task.execution_attempts[0].continuation.state.as_deref(),
        Some("executor_gone")
    );
}

#[test]
fn plan_review_reopens_after_tab_close_retires_executor_attempt() {
    let (fixture, plan_id, task_id, lane_path) = socket_bounce_fixture(true);
    let task_before_close = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    let worker_tab_id = task_before_close.assigned_tab_id.unwrap();

    fixture.service.close_tab(&worker_tab_id).unwrap();

    let task_after_close = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    assert_eq!(
        task_after_close.status,
        codeterm_service_core::plan::TaskStatus::InReview
    );
    assert_eq!(task_after_close.assigned_tab_id, None);
    assert_eq!(task_after_close.lane.tab_id, None);
    assert_eq!(
        task_after_close.lane.worktree_path.as_deref(),
        Some(lane_path.as_str())
    );
    assert_eq!(
        task_after_close.lane.branch.as_deref(),
        Some("lane/socket-bounce")
    );
    assert!(task_after_close.execution_attempts[0].retired_at.is_some());
    assert!(task_after_close.execution_attempts[0].closed_at.is_some());
    assert_eq!(task_after_close.execution_attempts[0].closure_reason, None);

    dispatch_bounce(&fixture, &plan_id, &task_id).unwrap();

    let reopened = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    assert_eq!(
        reopened.status,
        codeterm_service_core::plan::TaskStatus::Pending
    );
    assert_eq!(
        reopened.lane.worktree_path.as_deref(),
        Some(lane_path.as_str())
    );
    assert_eq!(reopened.lane.branch.as_deref(), Some("lane/socket-bounce"));
    assert_eq!(reopened.execution_attempts.len(), 1);
    assert!(reopened.execution_attempts[0].retired_at.is_some());
    assert!(reopened.execution_attempts[0].closed_at.is_some());
    assert_eq!(
        reopened.execution_attempts[0].closure_reason.as_deref(),
        Some("review_bounce_executor_gone")
    );
    assert!(reopened
        .review_evidence
        .last()
        .unwrap()
        .note
        .contains("retry the same lane"));
    assert!(reopened
        .milestones
        .last()
        .unwrap()
        .text
        .contains("retry the same lane"));
}

#[test]
fn plan_review_keeps_live_unbound_worker_in_review() {
    let (fixture, plan_id, task_id, lane_path) = socket_bounce_fixture(true);
    let error = dispatch_bounce(&fixture, &plan_id, &task_id).unwrap_err();
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|data| data.get("type"))
            .and_then(Value::as_str),
        Some("bounce_continuation_failed")
    );

    let stored_plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let task = stored_plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(
        task.status,
        codeterm_service_core::plan::TaskStatus::InReview
    );
    assert_eq!(
        task.status_label.as_deref(),
        Some("bounce_continuation_failed")
    );
    assert_eq!(task.execution_attempts[0].closed_at, None);
    assert_eq!(task.lane.worktree_path.as_deref(), Some(lane_path.as_str()));
    let occupancy = super::super::plan_task_dispatch::lane_safety::preflight_checkout_occupancy(
        &stored_plan,
        "retry-task",
        &lane_path,
        codeterm_service_core::service::LaneProtection::Enforced,
        &std::collections::HashMap::new(),
    )
    .expect_err("the unclosed Worker attempt continues to occupy its lane");
    assert_eq!(
        occupancy
            .data
            .as_ref()
            .and_then(|data| data.get("type"))
            .and_then(Value::as_str),
        Some("checkout_occupied")
    );
}

#[test]
fn plan_review_does_not_reopen_closed_attempt_while_executor_tab_is_present() {
    let (fixture, plan_id, task_id, lane_path) = socket_bounce_fixture(true);
    let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.execution_attempts[0].closed_at = Some(5);
    fixture.service.plan_store.save(&mut plan, 5).unwrap();

    let error = dispatch_bounce(&fixture, &plan_id, &task_id).unwrap_err();
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|data| data.get("type"))
            .and_then(Value::as_str),
        Some("bounce_continuation_failed")
    );
    let task = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    assert_eq!(
        task.status,
        codeterm_service_core::plan::TaskStatus::InReview
    );
    assert_eq!(task.lane.worktree_path.as_deref(), Some(lane_path.as_str()));
    assert_eq!(task.execution_attempts[0].closed_at, Some(5));
}

#[test]
fn plan_review_keeps_missing_remote_worker_attempt_open() {
    let (fixture, plan_id, task_id, _) = socket_bounce_fixture(false);
    let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.execution_attempts[0].mesh_peer = Some("mesh-peer".into());
    fixture.service.plan_store.save(&mut plan, 5).unwrap();

    let error = dispatch_bounce(&fixture, &plan_id, &task_id).unwrap_err();
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|data| data.get("type"))
            .and_then(Value::as_str),
        Some("bounce_continuation_failed")
    );
    let task = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    assert_eq!(
        task.status,
        codeterm_service_core::plan::TaskStatus::InReview
    );
    assert_eq!(task.execution_attempts[0].closed_at, None);
    assert_eq!(task.execution_attempts[0].closure_reason, None);
}
