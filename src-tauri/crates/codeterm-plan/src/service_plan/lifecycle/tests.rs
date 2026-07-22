use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskStatus};
use crate::service_plan::{NewTask, PlanError, PlanStore};

fn store_with_task() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Lifecycle".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "worker lane".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, p.id.clone(), task_id)
}

#[test]
fn pause_requires_reason() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    let err = store
        .pause_task(&plan_id, &task_id, "   ", AgentRef::from_pane("orch"), 3)
        .unwrap_err();
    assert!(matches!(err, PlanError::TaskPauseRequiresReason));
}

#[test]
fn pause_sets_status_reason_and_records_stride() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    store
        .pause_task(
            &plan_id,
            &task_id,
            "waiting on upstream API",
            AgentRef::from_pane("orch"),
            10,
        )
        .unwrap();
    let task = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|t| t.id == task_id)
        .unwrap();
    assert_eq!(task.status, TaskStatus::Paused);
    assert_eq!(
        task.pause_reason.as_deref(),
        Some("waiting on upstream API")
    );
    assert!(task
        .milestones
        .iter()
        .any(|m| m.text.contains("paused: waiting on upstream API")));
}

#[test]
fn pause_clears_assignment_and_closes_attempt() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    store
        .assign_task(&plan_id, &task_id, "worker-pane", None, 5)
        .unwrap();
    store
        .pause_task(
            &plan_id,
            &task_id,
            "operator hold",
            AgentRef::from_pane("orch"),
            10,
        )
        .unwrap();
    let task = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert!(task.assigned_pane_id.is_none());
    assert!(task.lane.pane_id.is_none());
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].closed_at, Some(10));
}

#[test]
fn resume_returns_to_pending_and_records_stride() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    store
        .pause_task(
            &plan_id,
            &task_id,
            "deferred",
            AgentRef::from_pane("orch"),
            10,
        )
        .unwrap();
    store
        .resume_task(&plan_id, &task_id, AgentRef::from_pane("orch"), 20)
        .unwrap();
    let task = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|t| t.id == task_id)
        .unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.pause_reason.is_none());
    assert!(task.milestones.iter().any(|m| m.text.contains("resumed")));
}

#[test]
fn resume_rejects_non_paused_task() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    let err = store
        .resume_task(&plan_id, &task_id, AgentRef::from_pane("orch"), 20)
        .unwrap_err();
    assert!(matches!(err, PlanError::TransitionDenied { .. }));
}

#[test]
fn paused_task_is_not_open_work() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    store
        .pause_task(
            &plan_id,
            &task_id,
            "deferred",
            AgentRef::from_pane("orch"),
            10,
        )
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    assert!(!plan.tasks.iter().any(|t| t.status.counts_as_open_work()));
}
