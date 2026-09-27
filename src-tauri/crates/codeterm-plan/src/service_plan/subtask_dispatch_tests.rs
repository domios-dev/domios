use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskLaneMetadata, TaskStatus};
use tempfile::TempDir;

fn approved_plan_with_in_progress_parent() -> (TempDir, PlanStore, String, String, String) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "Child dispatch".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.mode = PlanMode::Approved;
    store.save(&mut plan, 1).unwrap();
    let parent_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Parent".into(),
                assigned_tab_id: Some("parent-worker".into()),
                lane: TaskLaneMetadata {
                    tab_id: Some("parent-worker".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 3, |plan| {
            let parent = plan.tasks.iter_mut().find(|task| task.id == parent_id).unwrap();
            parent.status = TaskStatus::InProgress;
            parent.status_changed_at = 3;
            Ok(())
        })
        .unwrap();
    let child_id = store
        .add_child_tasks_atomic(
            &plan.id,
            &parent_id,
            vec![NewTask {
                name: "Child".into(),
                ..Default::default()
            }],
            4,
        )
        .unwrap().remove(0);
    (root, store, plan.id, parent_id, child_id)
}

fn dispatch_child(store: &PlanStore, plan_id: &str, child_id: &str) {
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
}

#[test]
fn child_of_in_progress_task_dispatches_into_its_own_lane() {
    let (_root, store, plan_id, parent_id, child_id) = approved_plan_with_in_progress_parent();
    let parent_before = store.show_task(&parent_id).unwrap().task;

    dispatch_child(&store, &plan_id, &child_id);

    let parent_after = store.show_task(&parent_id).unwrap().task;
    let child = store.show_task(&child_id).unwrap().task;
    assert_eq!(parent_after.status, TaskStatus::InProgress);
    assert_eq!(parent_after.assigned_tab_id, parent_before.assigned_tab_id);
    assert_eq!(parent_after.lane, parent_before.lane);
    assert_eq!(child.parent_task_id.as_deref(), Some(parent_id.as_str()));
    assert_eq!(child.status, TaskStatus::InProgress);
    assert_eq!(child.lane.tab_id.as_deref(), Some("child-worker"));
    assert_eq!(child.lane.worktree_path.as_deref(), Some("child-lane"));
}

#[test]
fn children_of_approved_task_are_dispatchable_without_replan() {
    let (_root, store, plan_id, _, child_id) = approved_plan_with_in_progress_parent();
    assert_eq!(store.get(&plan_id).unwrap().mode, PlanMode::Approved);

    dispatch_child(&store, &plan_id, &child_id);

    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.mode, PlanMode::Approved);
    assert_eq!(store.show_task(&child_id).unwrap().task.status, TaskStatus::InProgress);
}
