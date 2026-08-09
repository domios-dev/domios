use super::{
    live_competitive_executor, validate_competitive_attempt_status_transition,
    validate_task_status_transition,
};
use crate::plan::{OrchestratorRef, Plan, TaskExecutionAttempt, TaskStatus};
use crate::service_plan::{NewTask, PlanError, PlanStore};

fn attempt(id: &str, group: &str) -> TaskExecutionAttempt {
    TaskExecutionAttempt {
        id: id.into(),
        executor_tab_id: format!("tab-{id}"),
        competition_group_id: Some(group.into()),
        session_id: Some(format!("session-{id}")),
        ..Default::default()
    }
}

fn competitive_task(attempts: Vec<TaskExecutionAttempt>) -> (tempfile::TempDir, Plan, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Competition invariant".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "competitive".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&plan.id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.execution_attempts = attempts;
    (dir, plan, task_id)
}

fn progresses(plan: &Plan, task_id: &str, attempt_id: &str) -> Result<(), PlanError> {
    validate_competitive_attempt_status_transition(
        plan,
        task_id,
        &TaskStatus::Pending,
        &TaskStatus::InProgress,
        attempt_id,
    )
}

#[test]
fn exact_live_competitive_attempt_is_the_progression_authority_without_a_task_tab() {
    let (_dir, plan, task_id) =
        competitive_task(vec![attempt("a", "group"), attempt("b", "group")]);
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();

    assert_eq!(task.assigned_tab_id, None);
    assert_eq!(task.lane.tab_id, None);
    assert!(live_competitive_executor(task, "a"));
    assert!(live_competitive_executor(task, "b"));
    assert!(progresses(&plan, &task_id, "a").is_ok());
    assert!(progresses(&plan, &task_id, "b").is_ok());
    assert!(matches!(
        validate_task_status_transition(
            &plan,
            &task_id,
            &TaskStatus::Pending,
            &TaskStatus::InProgress,
            None,
        ),
        Err(PlanError::InProgressRequiresTab)
    ));
}

#[test]
fn missing_closed_unbound_or_foreign_group_attempts_never_progress_the_task() {
    let superseded = attempt("superseded", "old-group");
    let mut closed = attempt("closed", "group");
    closed.closed_at = Some(9);
    let mut unbound = attempt("unbound", "group");
    unbound.session_id = None;
    let (_dir, plan, task_id) =
        competitive_task(vec![superseded, closed, unbound, attempt("live", "group")]);
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();

    assert!(live_competitive_executor(task, "live"));
    assert!(progresses(&plan, &task_id, "live").is_ok());
    for rejected in ["absent", "closed", "unbound", "superseded"] {
        assert!(!live_competitive_executor(task, rejected), "{rejected}");
        assert!(
            matches!(
                progresses(&plan, &task_id, rejected),
                Err(PlanError::InProgressRequiresTab)
            ),
            "{rejected}"
        );
    }
}

#[test]
fn an_acknowledged_attempt_stays_the_executor_before_its_session_is_bound() {
    let mut acknowledged = attempt("acknowledged", "group");
    acknowledged.session_id = None;
    acknowledged.acknowledged_at = Some(7);
    let (_dir, plan, task_id) = competitive_task(vec![acknowledged]);
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();

    assert!(live_competitive_executor(task, "acknowledged"));
    assert!(progresses(&plan, &task_id, "acknowledged").is_ok());
}
