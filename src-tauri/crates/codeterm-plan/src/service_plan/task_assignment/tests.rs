use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, PlanMode, Task};
use crate::service_plan::task_attempts::{BoundWorkerReport, TaskRespawnRequest};
use crate::service_plan::NewTask;
use codeterm_types::agent::ReportStatus;

fn fixture() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "reassignment".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "lifecycle".into(),
                kind: "implement".into(),
                model: Some("codex:test-model".into()),
                acceptance_criteria: vec!["binding survives review".into()],
                test_plan: Some("focused lifecycle oracle".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, plan.id, task)
}

fn binding(session: &str) -> AssignmentRuntimeBinding {
    AssignmentRuntimeBinding {
        session_id: session.into(),
        session_binding_nonce: "test-nonce".into(),
        provider: None,
        model: None,
        session_model: None,
    }
}

#[test]
fn assignment_attempt_retains_planned_provider_and_model() {
    let (_dir, store, plan, task) = fixture();
    store
        .with_plan(&plan, 3, |plan| {
            plan.tasks[0].model = Some("codex/gpt-6-luna".into());
            Ok(())
        })
        .unwrap();

    store
        .assign_task_with_runtime_binding(
            &plan,
            &task,
            "worker",
            None,
            None,
            Some(&binding("worker-session")),
            None,
            4,
        )
        .unwrap();

    let mut detail = store.show_task(&task).unwrap();
    let attempt = detail.task.execution_attempts.remove(0);
    assert_eq!(attempt.provider.as_deref(), Some("codex"));
    assert_eq!(attempt.agent_type.as_deref(), Some("codex"));
    assert_eq!(attempt.model.as_deref(), Some("gpt-6-luna"));
}

fn authority(task: &Task, tab: &str) -> TaskMutationAuthority {
    TaskMutationAuthority {
        tab_id: tab.into(),
        session_id: task.interruption.owner_session_id.clone().unwrap(),
        session_binding_nonce: "test-nonce".into(),
        owner_epoch: task.interruption.owner_epoch,
    }
}

fn report(authority: TaskMutationAuthority) -> BoundWorkerReport {
    BoundWorkerReport {
        message: "owned candidate".into(),
        status: Some(ReportStatus::InReview),
        sha: Some("123456789abcdef".into()),
        branch: Some("lane/test".into()),
        authority: Some(authority),
        ..Default::default()
    }
}

fn assigned_review(store: &PlanStore, plan: &str, task: &str, authorized: bool) -> String {
    let owner = TaskMutationAuthority {
        tab_id: "owner".into(),
        session_id: "owner-session".into(),
        session_binding_nonce: "owner-nonce".into(),
        owner_epoch: 0,
    };
    store
        .assign_task_with_runtime_binding(
            plan,
            task,
            "worker",
            None,
            Some("worker"),
            Some(&binding("worker-session")),
            authorized.then_some(&owner),
            3,
        )
        .unwrap();
    let assigned = store.show_task(task).unwrap().task;
    let attempt = assigned.execution_attempts.last().unwrap();
    assert_eq!(assigned.status, TaskStatus::Pending);
    assert_eq!(attempt.session_id.as_deref(), Some("worker-session"));
    assert!(attempt.acknowledged_at.is_none());
    let attempt_id = attempt.id.clone();
    store
        .acknowledge_task_attempt(plan, task, "worker", &attempt_id, 4)
        .unwrap();
    assert!(store
        .ingest_bound_worker_report(
            plan,
            task,
            &AgentRef::from_tab("worker"),
            report(authority(&assigned, "worker")),
            5
        )
        .unwrap());
    assert_eq!(
        store.show_task(task).unwrap().task.status,
        TaskStatus::InReview
    );
    attempt_id
}

fn roundtrip(authorized: bool) {
    let (_dir, store, plan, task) = fixture();
    let attempt = assigned_review(&store, &plan, &task, authorized);
    let result = store
        .bounce_review(
            &plan,
            &task,
            "repair existing oracle",
            false,
            false,
            AgentRef::from_tab("owner"),
            6,
        )
        .unwrap();
    let continuation = result.continuation.unwrap();
    assert_eq!(continuation.attempt_id, attempt);
    assert_eq!(continuation.session_id, "worker-session");
    store
        .claim_review_continuation_delivery(&plan, &task, &attempt, continuation.epoch, 7)
        .expect("valid continuation must be claimed");
    let claimed = store.show_task(&task).unwrap().task;
    assert_eq!(claimed.status, TaskStatus::InReview);
    let claimed_attempt = claimed
        .execution_attempts
        .iter()
        .find(|current| current.id == attempt)
        .expect("same attempt remains persisted");
    assert_eq!(
        claimed_attempt.session_id.as_deref(),
        Some("worker-session")
    );
    assert_eq!(claimed_attempt.continuation.epoch, continuation.epoch);
    assert_eq!(
        claimed_attempt.continuation.state.as_deref(),
        Some("continuation_delivering")
    );
    assert_eq!(claimed_attempt.acknowledged_at, Some(4));
    assert_eq!(claimed_attempt.reported_at, Some(5));
    let before_refusal = serde_json::to_value(store.get(&plan).unwrap()).unwrap();
    let refusal = store
        .claim_review_continuation_delivery(&plan, &task, &attempt, continuation.epoch, 7)
        .expect_err("a claimed continuation cannot be claimed twice");
    assert!(matches!(
        refusal,
        PlanError::ReviewContinuationDeliveryRefused { .. }
    ));
    assert_eq!(
        serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
        before_refusal
    );
    assert!(store
        .finish_review_continuation(&plan, &task, &attempt, continuation.epoch, true, 8)
        .unwrap());
    let current = store.show_task(&task).unwrap().task;
    assert_eq!(current.status, TaskStatus::InProgress);
    assert_eq!(current.execution_attempts[0].acknowledged_at, Some(4));
    assert_eq!(current.execution_attempts[0].reported_at, Some(5));
}

#[test]
fn lifecycle_reassignment_ordinary_assign_ack_report_bounce() {
    roundtrip(false);
}

#[test]
fn lifecycle_reassignment_authorized_assign_ack_report_bounce() {
    roundtrip(true);
}

#[test]
fn lifecycle_reassignment_review_respawn_ack_preserves_history_and_fences_retired() {
    let (_dir, store, plan, task) = fixture();
    let previous_id = assigned_review(&store, &plan, &task, true);
    store
        .hold_review(
            &plan,
            &task,
            "preserved owner review",
            AgentRef::from_tab("owner"),
            5,
        )
        .unwrap();
    let previous = store.show_task(&task).unwrap().task;
    assert!(!previous.review_evidence.is_empty());
    let draft = store
        .build_task_respawn_draft(
            &plan,
            &task,
            TaskRespawnRequest {
                attempt_id: previous_id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let replacement = store
        .record_task_respawn_attempt(&plan, &task, "replacement", &draft, Some("worker"), 6)
        .unwrap();
    let starting = store.show_task(&task).unwrap().task;
    assert_eq!(starting.status, TaskStatus::Pending);
    assert_eq!(starting.status_label.as_deref(), Some("starting"));
    assert!(replacement.attempt.acknowledged_at.is_none());
    assert_eq!(starting.execution_attempts[0].closed_at, Some(6));
    assert_eq!(starting.execution_attempts[0].reported_at, Some(5));
    assert_eq!(
        serde_json::to_value(&starting.execution_attempts[0].report).unwrap(),
        serde_json::to_value(&previous.execution_attempts[0].report).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&starting.review_audit).unwrap(),
        serde_json::to_value(&previous.review_audit).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&starting.review_evidence).unwrap(),
        serde_json::to_value(&previous.review_evidence).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&starting.milestones).unwrap(),
        serde_json::to_value(&previous.milestones).unwrap()
    );
    assert_eq!(
        starting
            .execution_attempts
            .iter()
            .filter(|a| a.closed_at.is_none())
            .count(),
        1
    );
    store
        .finish_task_dispatch(
            &plan,
            &task,
            "replacement",
            &replacement.attempt.id,
            "replacement-session",
            "test-nonce",
            starting.lane.clone(),
            7,
        )
        .unwrap();
    let ready = store.show_task(&task).unwrap().task;
    assert_eq!(ready.status, TaskStatus::Pending);
    assert_eq!(ready.status_label.as_deref(), Some("awaiting_worker_ack"));
    assert!(store
        .acknowledge_task_attempt(&plan, &task, "worker", &previous_id, 8)
        .is_err());
    assert!(!store
        .ingest_bound_worker_report(
            &plan,
            &task,
            &AgentRef::from_tab("worker"),
            report(authority(&previous, "worker")),
            8
        )
        .unwrap());
    let ack = store
        .acknowledge_task_attempt(&plan, &task, "replacement", &replacement.attempt.id, 9)
        .unwrap();
    assert_eq!(ack.attempt.acknowledged_at, Some(9));
    assert!(store
        .ingest_bound_worker_report(
            &plan,
            &task,
            &AgentRef::from_tab("replacement"),
            report(authority(&ready, "replacement")),
            10
        )
        .unwrap());
}

#[test]
fn lifecycle_reassignment_replacement_retires_same_tab_epoch() {
    let (_dir, store, plan, task) = fixture();
    store
        .assign_task_with_runtime_binding(
            &plan,
            &task,
            "worker",
            None,
            Some("worker"),
            Some(&binding("worker-session")),
            None,
            3,
        )
        .unwrap();
    let old = store.show_task(&task).unwrap().task;
    let old_id = old.execution_attempts[0].id.clone();
    let owner = TaskMutationAuthority {
        tab_id: "owner".into(),
        owner_epoch: old.interruption.owner_epoch,
        session_id: "owner-session".into(),
        session_binding_nonce: "owner-nonce".into(),
    };
    store
        .assign_task_with_runtime_binding(
            &plan,
            &task,
            "worker",
            None,
            Some("worker"),
            Some(&binding("new-session")),
            Some(&owner),
            6,
        )
        .unwrap();
    let new = store.show_task(&task).unwrap().task;
    assert_eq!(
        new.execution_attempts
            .iter()
            .filter(|a| a.closed_at.is_none())
            .count(),
        1
    );
    assert!(store
        .acknowledge_task_attempt(&plan, &task, "worker", &old_id, 7)
        .is_err());
    assert!(store
        .ingest_bound_worker_report(
            &plan,
            &task,
            &AgentRef::from_tab("worker"),
            report(authority(&old, "worker")),
            7
        )
        .is_err());
    assert!(new
        .execution_attempts
        .last()
        .unwrap()
        .acknowledged_at
        .is_none());
}

#[test]
fn lifecycle_reassignment_invalid_binding_and_remote_binding_do_not_mutate() {
    let (_dir, store, plan, task) = fixture();
    let before = serde_json::to_value(store.get(&plan).unwrap()).unwrap();
    assert!(store
        .assign_task_with_runtime_binding(&plan, &task, "worker", None, None, None, None, 3)
        .is_err());
    assert_eq!(
        serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
        before
    );
    for (session, peer) in [("", None), ("local-session", Some("peer"))] {
        assert!(store
            .assign_task_with_runtime_binding(
                &plan,
                &task,
                "worker",
                peer,
                None,
                Some(&binding(session)),
                None,
                3
            )
            .is_err());
        assert_eq!(
            serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
            before
        );
    }
}

#[test]
fn lifecycle_reassignment_assignment_cannot_clear_review_or_active_state() {
    let (_dir, store, plan, task) = fixture();
    store
        .assign_task_with_runtime_binding(
            &plan,
            &task,
            "worker",
            None,
            None,
            Some(&binding("worker-session")),
            None,
            3,
        )
        .unwrap();
    let initial = store.show_task(&task).unwrap().task;
    store
        .acknowledge_task_attempt(&plan, &task, "worker", &initial.execution_attempts[0].id, 4)
        .unwrap();
    for in_review in [false, true] {
        if in_review {
            store
                .ingest_bound_worker_report(
                    &plan,
                    &task,
                    &AgentRef::from_tab("worker"),
                    report(authority(&initial, "worker")),
                    5,
                )
                .unwrap();
            store
                .hold_review(
                    &plan,
                    &task,
                    "held candidate",
                    AgentRef::from_tab("owner"),
                    6,
                )
                .unwrap();
        }
        let before = serde_json::to_value(store.get(&plan).unwrap()).unwrap();
        assert!(store
            .assign_task_with_runtime_binding(
                &plan,
                &task,
                "replacement",
                None,
                None,
                Some(&binding("replacement-session")),
                None,
                7
            )
            .is_err());
        assert_eq!(
            serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
            before
        );
    }
}
