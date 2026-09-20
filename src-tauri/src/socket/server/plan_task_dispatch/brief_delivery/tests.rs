use super::*;

use crate::plan::{OrchestratorRef, PlanMode, TaskLaneMetadata, TaskStatus};
use crate::service::plan::{NewTask, PlanStore};
use crate::service::SpawnAgentParams;

#[test]
fn atomic_task_dispatch_launch_has_authority_but_withholds_the_brief() {
    let mut spawn_params = SpawnAgentParams {
        role: Some("Worker".into()),
        plan_id: Some("plan-atomic".into()),
        task_id: Some("task-atomic".into()),
        ..Default::default()
    };

    super::super::defer_dispatch_brief(
        &mut spawn_params,
        "plan-atomic",
        "task-atomic",
        "attempt-atomic",
        &TaskLaneMetadata::default(),
        "opaque brief payload".into(),
    )
    .unwrap();

    assert!(spawn_params.task.is_none());
    let launch_env = spawn_params.task_launch_env();
    assert_eq!(
        launch_env.get("CODETERM_PLAN_ID").map(String::as_str),
        Some("plan-atomic")
    );
    assert_eq!(
        launch_env.get("CODETERM_TASK_ID").map(String::as_str),
        Some("task-atomic")
    );
    assert_eq!(
        launch_env.get("CODETERM_ATTEMPT_ID").map(String::as_str),
        Some("attempt-atomic")
    );
    discard_deferred_task_brief("attempt-atomic");
}

#[test]
fn task_brief_waits_for_the_same_attempts_durable_binding_and_ack() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Deferred brief".into(),
            project,
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut approved = store.get(&plan.id).unwrap();
    approved.mode = PlanMode::Approved;
    store.save(&mut approved, 2).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let tab_id = "worker-tab";
    let lane = store.show_task(&task_id).unwrap().task.lane;
    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, tab_id, "Ada", lane.clone(), 4)
        .unwrap();
    let launch_assignment =
        defer_task_brief_before_launch(&plan.id, &task_id, &attempt_id, "opaque task brief".into())
            .unwrap();
    assert!(launch_assignment.is_none());
    bind_deferred_task_brief_to_tab(&attempt_id, tab_id).unwrap();

    let starting = store.show_task(&task_id).unwrap().task;
    let starting_attempt = starting
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert!(!task_brief_is_ready(
        &starting.status,
        starting.assigned_tab_id.as_deref(),
        starting_attempt,
        tab_id,
    ));

    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            tab_id,
            &attempt_id,
            "session-worker",
            "nonce-worker",
            lane,
            5,
        )
        .unwrap();
    let bound = store.show_task(&task_id).unwrap().task;
    let bound_attempt = bound
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert_eq!(bound.status, TaskStatus::Pending);
    assert!(!task_brief_is_ready(
        &bound.status,
        bound.assigned_tab_id.as_deref(),
        bound_attempt,
        tab_id,
    ));

    store
        .acknowledge_live_task_attempt_for_tab(tab_id, Some(&plan.id), Some(&task_id), 6)
        .unwrap();
    let acknowledged = store.show_task(&task_id).unwrap().task;
    let acknowledged_attempt = acknowledged
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert!(task_brief_is_ready(
        &acknowledged.status,
        acknowledged.assigned_tab_id.as_deref(),
        acknowledged_attempt,
        tab_id,
    ));

    let claimed = claim_deferred_task_brief(
        &attempt_id,
        &plan.id,
        &task_id,
        tab_id,
        acknowledged_attempt,
    )
    .unwrap()
    .unwrap();
    assert_eq!(claimed.0, "opaque task brief");
    assert!(claim_deferred_task_brief(
        &attempt_id,
        &plan.id,
        &task_id,
        tab_id,
        acknowledged_attempt,
    )
    .unwrap()
    .is_none());
    discard_deferred_task_brief(&attempt_id);
}

#[test]
fn early_ack_and_late_binding_open_the_brief_gate_only_after_both_commits() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Early ACK".into(),
            project,
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut approved = store.get(&plan.id).unwrap();
    approved.mode = PlanMode::Approved;
    store.save(&mut approved, 2).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let tab_id = "early-ack-worker";
    let lane: TaskLaneMetadata = store.show_task(&task_id).unwrap().task.lane;
    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, tab_id, "Ada", lane.clone(), 4)
        .unwrap();
    let launch_assignment =
        defer_task_brief_before_launch(&plan.id, &task_id, &attempt_id, "early task brief".into())
            .unwrap();
    assert!(launch_assignment.is_none());
    bind_deferred_task_brief_to_tab(&attempt_id, tab_id).unwrap();

    store
        .acknowledge_live_task_attempt_for_tab(tab_id, Some(&plan.id), Some(&task_id), 5)
        .unwrap();
    let acknowledged_first = store.show_task(&task_id).unwrap().task;
    let attempt = acknowledged_first
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert!(!task_brief_is_ready(
        &acknowledged_first.status,
        acknowledged_first.assigned_tab_id.as_deref(),
        attempt,
        tab_id,
    ));

    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            tab_id,
            &attempt_id,
            "session-early-worker",
            "nonce-early-worker",
            lane,
            6,
        )
        .unwrap();
    let bound_after_ack = store.show_task(&task_id).unwrap().task;
    let attempt = bound_after_ack
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert!(task_brief_is_ready(
        &bound_after_ack.status,
        bound_after_ack.assigned_tab_id.as_deref(),
        attempt,
        tab_id,
    ));
    discard_deferred_task_brief(&attempt_id);
}
