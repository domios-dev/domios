use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanMode};
use crate::service_plan::NewTask;

fn setup() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "QA".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_qa".into();
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let first = store
        .add_task(
            &plan.id,
            NewTask {
                name: "first".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("run focused first test".into()),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let second = store
        .add_task(
            &plan.id,
            NewTask {
                name: "second".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("run focused second test".into()),
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();
    for task_id in [&first, &second] {
        store
            .update_task(
                &plan.id,
                task_id,
                None,
                None,
                None,
                None,
                Some("in_review"),
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
    }
    (dir, store, plan.id, first, second)
}

#[test]
fn enqueue_claim_and_pass_write_qa_milestones() {
    let (_dir, store, plan_id, first, _second) = setup();

    let enqueued = store
        .enqueue_qa(
            &plan_id,
            &first,
            "ready for QA",
            AgentRef::from_tab("orch"),
            10,
        )
        .unwrap();
    assert_eq!(enqueued.task_id, first);
    let claimed = store
        .claim_next_qa(
            &plan_id,
            "tester scope",
            AgentRef::from_tab("tester"),
            Some("tester"),
            11,
        )
        .unwrap()
        .unwrap();
    assert_eq!(claimed.test_plan.as_deref(), Some("run focused first test"));

    let task = store.show_task(&first).unwrap().task;
    assert_eq!(task.review_stage, ReviewStage::Qa);
    assert_eq!(task.qa_state, Some(QaState::Running));
    assert_eq!(task.assigned_tab_id.as_deref(), Some("tester"));
    assert!(task
        .milestones
        .iter()
        .any(|m| m.kind == QA_MILESTONE_KIND && m.text.contains("claim: tester scope")));

    store
        .pass_qa(
            &plan_id,
            &first,
            "verified",
            AgentRef::from_tab("tester"),
            12,
        )
        .unwrap();
    let task = store.show_task(&first).unwrap().task;
    assert_eq!(task.status, TaskStatus::Done);
    assert_eq!(task.review_stage, ReviewStage::Review);
    assert_eq!(task.qa_state, None);
    assert!(task
        .milestones
        .iter()
        .any(|m| m.kind == QA_MILESTONE_KIND && m.text.contains("pass: verified")));
}

#[test]
fn note_is_required_for_every_qa_transition() {
    let (_dir, store, plan_id, first, _second) = setup();

    let err = store
        .enqueue_qa(&plan_id, &first, "  ", AgentRef::from_tab("orch"), 10)
        .unwrap_err();
    assert!(matches!(err, PlanError::QaNoteRequired));
}

#[test]
fn only_one_running_qa_task_is_claimed_by_default() {
    let (_dir, store, plan_id, first, second) = setup();
    store
        .enqueue_qa(&plan_id, &first, "first", AgentRef::from_tab("orch"), 10)
        .unwrap();
    store
        .enqueue_qa(&plan_id, &second, "second", AgentRef::from_tab("orch"), 11)
        .unwrap();

    let first_claim = store
        .claim_next_qa(
            &plan_id,
            "claim first",
            AgentRef::from_tab("tester"),
            Some("tester"),
            12,
        )
        .unwrap();
    let second_claim = store
        .claim_next_qa(
            &plan_id,
            "claim second",
            AgentRef::from_tab("tester2"),
            Some("tester2"),
            13,
        )
        .unwrap();

    assert_eq!(first_claim.unwrap().task_id, first);
    assert!(second_claim.is_none());
    let queue = store.qa_queue(&plan_id, 14).unwrap();
    assert_eq!(queue.running.len(), 1);
    assert_eq!(queue.queued.len(), 1);
    assert_eq!(queue.queued[0].position, Some(1));
}

#[test]
fn verdict_requires_running_qa() {
    let (_dir, store, plan_id, first, _second) = setup();
    store
        .enqueue_qa(&plan_id, &first, "ready", AgentRef::from_tab("orch"), 10)
        .unwrap();

    let pass_err = store
        .pass_qa(
            &plan_id,
            &first,
            "verified",
            AgentRef::from_tab("tester"),
            11,
        )
        .unwrap_err();
    let fail_err = store
        .fail_qa(&plan_id, &first, "failed", AgentRef::from_tab("tester"), 12)
        .unwrap_err();

    assert!(matches!(pass_err, PlanError::QaNotRunning));
    assert!(matches!(fail_err, PlanError::QaNotRunning));
}

#[test]
fn fail_bounces_to_in_progress_and_restores_implementer() {
    let (_dir, store, plan_id, first, _second) = setup();
    store
        .update_task_lane(
            &plan_id,
            &first,
            crate::service_plan::TaskLaneUpdate {
                tab_id: Some(Some("impl-tab".into())),
                ..Default::default()
            },
            9,
        )
        .unwrap();
    store
        .enqueue_qa(&plan_id, &first, "ready", AgentRef::from_tab("orch"), 10)
        .unwrap();
    store
        .claim_next_qa(
            &plan_id,
            "claim",
            AgentRef::from_tab("tester"),
            Some("tester"),
            11,
        )
        .unwrap();

    let result = store
        .fail_qa(
            &plan_id,
            &first,
            "missing regression coverage",
            AgentRef::from_tab("tester"),
            12,
        )
        .unwrap();
    let task = store.show_task(&first).unwrap().task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.review_stage, ReviewStage::Review);
    assert_eq!(task.qa_state, None);
    assert_eq!(result.nudged_tab_id.as_deref(), Some("impl-tab"));
    assert!(task
        .milestones
        .iter()
        .any(|m| m.kind == QA_MILESTONE_KIND && m.text.contains("fail: missing")));
}
