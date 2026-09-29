use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanningState, TaskLaneMetadata, TaskStatus};
use tempfile::TempDir;

fn approved_plan_with_in_progress_parent() -> (TempDir, PlanStore, String, String, String, String) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "Child dispatch".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.mode = PlanMode::Approved;
    plan.planning_state = PlanningState::Approved;
    store.save(&mut plan, 1).unwrap();
    let parent_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Parent".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let parent_lane = TaskLaneMetadata {
        tab_id: Some("parent-worker".into()),
        worktree_path: Some("parent-lane".into()),
        branch: Some("parent-branch".into()),
        base: Some("canonical-base".into()),
        ..Default::default()
    };
    let parent_attempt_id = store
        .begin_task_dispatch(
            &plan.id,
            &parent_id,
            "parent-worker",
            "Nova",
            parent_lane.clone(),
            3,
        )
        .unwrap();
    store
        .finish_task_dispatch(
            &plan.id,
            &parent_id,
            "parent-worker",
            &parent_attempt_id,
            "parent-session",
            "parent-nonce",
            parent_lane,
            4,
        )
        .unwrap();
    store
        .acknowledge_task_attempt(&plan.id, &parent_id, "parent-worker", &parent_attempt_id, 5)
        .unwrap();
    let child_ids = store
        .add_child_tasks_atomic(
            &plan.id,
            &parent_id,
            vec![NewTask {
                name: "Child".into(),
                ..Default::default()
            }],
            5,
        )
        .unwrap();
    assert_eq!(child_ids.len(), 1);
    let child_id = child_ids.into_iter().next().expect("split child");
    (root, store, plan.id, parent_id, child_id, parent_attempt_id)
}

fn dispatch_child(store: &PlanStore, plan_id: &str, child_id: &str) -> String {
    let lane = TaskLaneMetadata {
        tab_id: Some("child-worker".into()),
        worktree_path: Some("child-lane".into()),
        branch: Some("child-branch".into()),
        base: Some("canonical-base".into()),
        ..Default::default()
    };
    let attempt_id = store
        .begin_task_dispatch(plan_id, child_id, "child-worker", "Nova", lane.clone(), 5)
        .unwrap();
    store
        .finish_task_dispatch(
            plan_id,
            child_id,
            "child-worker",
            &attempt_id,
            "child-session",
            "child-nonce",
            lane,
            6,
        )
        .unwrap();
    attempt_id
}

#[test]
fn child_of_in_progress_task_dispatches_into_its_own_lane() {
    let (_root, store, plan_id, parent_id, child_id, parent_attempt_id) =
        approved_plan_with_in_progress_parent();
    let parent_before = store.show_task(&parent_id).unwrap().task;

    let child_attempt_id = dispatch_child(&store, &plan_id, &child_id);

    let parent_after = store.show_task(&parent_id).unwrap().task;
    let child = store.show_task(&child_id).unwrap().task;
    assert_eq!(parent_after.status, TaskStatus::InProgress);
    assert_eq!(parent_after.assigned_tab_id, parent_before.assigned_tab_id);
    assert_eq!(parent_after.lane, parent_before.lane);
    assert_eq!(
        parent_after.execution_attempts,
        parent_before.execution_attempts
    );
    assert!(parent_after
        .execution_attempts
        .iter()
        .any(|attempt| attempt.id == parent_attempt_id));
    assert_eq!(child.parent_task_id.as_deref(), Some(parent_id.as_str()));
    assert_eq!(child.status, TaskStatus::Pending);
    assert_eq!(child.lane.tab_id.as_deref(), Some("child-worker"));
    assert_eq!(child.lane.worktree_path.as_deref(), Some("child-lane"));
    assert_eq!(child.assigned_tab_id.as_deref(), Some("child-worker"));
    let child_attempt = child
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == child_attempt_id)
        .expect("child dispatch attempt");
    assert_eq!(child_attempt.executor_tab_id, "child-worker");
    assert_eq!(child_attempt.session_id.as_deref(), Some("child-session"));
    assert!(child_attempt.acknowledged_at.is_none());
    assert!(child_attempt.closed_at.is_none());
}

#[test]
fn children_of_approved_task_are_dispatchable_without_replan() {
    let (_root, store, plan_id, _, child_id, _) = approved_plan_with_in_progress_parent();
    assert_eq!(store.get(&plan_id).unwrap().mode, PlanMode::Execution);
    assert_eq!(
        store.get(&plan_id).unwrap().planning_state,
        PlanningState::Approved
    );

    let child_attempt_id = dispatch_child(&store, &plan_id, &child_id);

    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.mode, PlanMode::Execution);
    assert_eq!(plan.planning_state, PlanningState::Approved);
    let child = store.show_task(&child_id).unwrap().task;
    assert_eq!(child.status, TaskStatus::Pending);
    assert!(child.execution_attempts.iter().any(|attempt| {
        attempt.id == child_attempt_id
            && attempt.acknowledged_at.is_none()
            && attempt.closed_at.is_none()
    }));
}
