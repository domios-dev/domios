use super::super::preflight_checkout_occupancy;
use crate::plan::{AgentRef, OrchestratorRef, TaskLaneMetadata, TaskStatus};
use crate::service::plan::task_attempts::{
    BoundWorkerReport, TaskCompeteRequest, TaskRespawnRequest,
};
use crate::service::plan::{NewTask, PlanError, PlanStore};
use crate::service::LaneProtection;
use codeterm_types::agent::ReportStatus;
use std::collections::HashSet;

fn reported_task() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "QA acceptance".into(),
            checkout.clone(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "accepted task".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["focused oracle passes".into()],
                test_plan: Some("run focused oracle".into()),
                lane: TaskLaneMetadata {
                    worktree_path: Some(checkout.clone()),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "executor", None, 3)
        .unwrap();
    assert!(store
        .ingest_bound_worker_report(
            &plan.id,
            &task_id,
            &AgentRef::from_tab("executor"),
            BoundWorkerReport {
                message: "implementation ready".into(),
                status: Some(ReportStatus::InReview),
                ..Default::default()
            },
            4,
        )
        .unwrap());
    (root, store, plan.id, task_id, checkout)
}

fn claim_qa(store: &PlanStore, plan_id: &str, task_id: &str) {
    store
        .enqueue_qa(plan_id, task_id, "ready", AgentRef::from_tab("orch"), 5)
        .unwrap();
    let claimed = store
        .claim_next_qa(
            plan_id,
            "canonical QA",
            AgentRef::from_tab("tester"),
            Some("tester"),
            6,
        )
        .unwrap()
        .unwrap();
    assert_eq!(claimed.task_id, task_id);
}

#[test]
fn qa_pass_atomically_closes_attempt_checkout_binding_and_task() {
    let (_root, store, plan_id, task_id, checkout) = reported_task();
    claim_qa(&store, &plan_id, &task_id);
    let claimed_plan = store.get(&plan_id).unwrap();
    let claimed_task = claimed_plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    let attempt_id = claimed_task.execution_attempts[0].id.clone();
    assert_eq!(claimed_task.execution_attempts[0].reviewed_at, Some(6));
    preflight_checkout_occupancy(
        &claimed_plan,
        "probe-task",
        &checkout,
        LaneProtection::Enforced,
        &HashSet::new(),
    )
    .unwrap_err();

    store
        .pass_qa(
            &plan_id,
            &task_id,
            "focused oracle passed",
            AgentRef::from_tab("tester"),
            10,
        )
        .unwrap();

    let accepted_plan = store.get(&plan_id).unwrap();
    let accepted_task = accepted_plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    let accepted_attempt = accepted_task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert_eq!(accepted_attempt.judge_verdict.as_deref(), Some("accepted"));
    assert_eq!(
        accepted_attempt.judge_reason.as_deref(),
        Some("focused oracle passed")
    );
    assert_eq!(accepted_attempt.judged_at, Some(10));
    assert_eq!(accepted_attempt.closed_at, Some(10));
    preflight_checkout_occupancy(
        &accepted_plan,
        "probe-task",
        &checkout,
        LaneProtection::Enforced,
        &HashSet::new(),
    )
    .unwrap();
    assert_eq!(accepted_task.assigned_tab_id, None);
    assert_eq!(accepted_task.lane.tab_id, None);
    assert_eq!(accepted_task.status, TaskStatus::Done);
    assert_eq!(store.task_binding_for_tab("tester"), None);
    assert!(store.task_bindings_for_tab("executor").is_empty());
}

#[test]
fn qa_pass_failure_preserves_attempt_checkout_binding_and_task() {
    let (_root, store, plan_id, task_id, checkout) = reported_task();
    claim_qa(&store, &plan_id, &task_id);
    store
        .add_task(
            &plan_id,
            NewTask {
                name: "open child".into(),
                parent_task_id: Some(task_id.clone()),
                ..Default::default()
            },
            7,
        )
        .unwrap();
    let before = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();

    let error = store
        .pass_qa(
            &plan_id,
            &task_id,
            "focused oracle passed",
            AgentRef::from_tab("tester"),
            10,
        )
        .unwrap_err();

    assert!(matches!(error, PlanError::OpenSubtasks { .. }));
    let after_plan = store.get(&plan_id).unwrap();
    let after = after_plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(after, &before);
    preflight_checkout_occupancy(
        &after_plan,
        "probe-task",
        &checkout,
        LaneProtection::Enforced,
        &HashSet::new(),
    )
    .unwrap_err();
}

#[test]
fn qa_pass_rejects_live_competition_and_superseded_replacement_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Competitive QA".into(),
            root.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "competitive task".into(),
                acceptance_criteria: vec!["winner passes".into()],
                test_plan: Some("run winner oracle".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let drafts = store
        .build_task_compete_drafts(&plan.id, &task_id, TaskCompeteRequest::default())
        .unwrap();
    let winner = store
        .record_task_compete_attempt(
            &plan.id,
            &task_id,
            "winner-tab",
            &drafts[0],
            Some("worker"),
            3,
        )
        .unwrap();
    let peer = store
        .record_task_compete_attempt(
            &plan.id,
            &task_id,
            "peer-tab",
            &drafts[1],
            Some("worker"),
            3,
        )
        .unwrap();
    store
        .judge_task_attempt(
            &plan.id,
            &task_id,
            &winner.attempt.id,
            "winner",
            None,
            true,
            4,
        )
        .unwrap();
    claim_qa(&store, &plan.id, &task_id);
    let live_competition = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    let competition_error = store
        .pass_qa(
            &plan.id,
            &task_id,
            "claimed winner accepted",
            AgentRef::from_tab("tester"),
            7,
        )
        .unwrap_err();
    assert!(matches!(
        competition_error,
        PlanError::ReadbackMismatch { operation, .. } if operation == "qa_pass"
    ));
    let after_competition_error = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(after_competition_error, live_competition);

    store
        .judge_task_attempt(
            &plan.id,
            &task_id,
            &winner.attempt.id,
            "superseded",
            None,
            false,
            7,
        )
        .unwrap();
    let respawn = store
        .build_task_respawn_draft(
            &plan.id,
            &task_id,
            TaskRespawnRequest {
                attempt_id: winner.attempt.id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let replacement = store
        .record_task_respawn_attempt(
            &plan.id,
            &task_id,
            "replacement-tab",
            &respawn,
            Some("worker"),
            8,
        )
        .unwrap();

    let superseded = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    let superseded_error = store
        .pass_qa(
            &plan.id,
            &task_id,
            "claimed winner accepted",
            AgentRef::from_tab("tester"),
            10,
        )
        .unwrap_err();
    // Judging the winner superseded voids the claim, so this is refused a guard
    // earlier than the live-competition case above, which readback still owns.
    assert!(
        matches!(&superseded_error, PlanError::QaNotRunning),
        "a superseded replacement must be refused, got {superseded_error:?}"
    );

    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task, superseded);
    let claimed = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == winner.attempt.id)
        .unwrap();
    assert_eq!(claimed.judge_verdict.as_deref(), Some("superseded"));
    assert_eq!(claimed.judged_at, Some(7));
    assert_eq!(claimed.closed_at, Some(7));
    for untouched_id in [&peer.attempt.id, &replacement.attempt.id] {
        let untouched = task
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == untouched_id.as_str())
            .unwrap();
        assert_eq!(untouched.judge_verdict, None);
        assert_eq!(untouched.judged_at, None);
        assert_eq!(untouched.closed_at, None);
    }
    assert_eq!(task.assigned_tab_id.as_deref(), Some("replacement-tab"));
    assert_eq!(task.lane.tab_id.as_deref(), Some("replacement-tab"));
    // A respawn hands the work back: the replacement owes an ack before this is
    // in progress again, and it is certainly no longer in review.
    assert_eq!(task.status, TaskStatus::Pending);
}
