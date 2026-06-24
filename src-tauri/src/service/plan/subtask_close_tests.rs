use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskStatus};

fn store_with_task() -> (PlanStore, String, String) {
    let root = tempfile::tempdir().unwrap().keep();
    let store = PlanStore::new_for_test(&root);
    let mut plan = Plan::new(
        "T".into(),
        root.display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "parent".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (store, plan.id, task_id)
}

fn add_child(store: &PlanStore, plan_id: &str, parent_id: &str) -> String {
    store
        .add_task(
            plan_id,
            NewTask {
                name: "child".into(),
                kind: "implement".into(),
                parent_task_id: Some(parent_id.into()),
                ..Default::default()
            },
            10,
        )
        .unwrap()
}

#[test]
fn close_task_with_open_subtask_errors() {
    let (store, plan_id, parent_id) = store_with_task();
    let child_id = add_child(&store, &plan_id, &parent_id);

    let err = store
        .close_task(&plan_id, &parent_id, None, AgentRef::from_pane("w"), 30)
        .unwrap_err();

    match err {
        PlanError::OpenSubtasks { subtasks } => {
            assert_eq!(subtasks.len(), 1);
            assert_eq!(subtasks[0].id, child_id);
            assert_eq!(subtasks[0].name, "child");
            assert_eq!(subtasks[0].status, TaskStatus::Pending);
        }
        other => panic!("expected open subtasks, got {other:?}"),
    }
    let parent = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == parent_id)
        .unwrap();
    assert_eq!(parent.status, TaskStatus::Pending);
}

#[test]
fn close_task_after_subtasks_done_succeeds() {
    let (store, plan_id, parent_id) = store_with_task();
    let child_id = add_child(&store, &plan_id, &parent_id);
    store
        .close_task(&plan_id, &child_id, None, AgentRef::from_pane("w"), 20)
        .unwrap();

    store
        .close_task(
            &plan_id,
            &parent_id,
            Some("done"),
            AgentRef::from_pane("w"),
            30,
        )
        .unwrap();

    let parent = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == parent_id)
        .unwrap();
    assert_eq!(parent.status, TaskStatus::Done);
}

#[test]
fn update_task_status_done_with_open_subtask_errors() {
    let (store, plan_id, parent_id) = store_with_task();
    add_child(&store, &plan_id, &parent_id);

    let err = store
        .update_task(
            &plan_id,
            &parent_id,
            None,
            None,
            None,
            None,
            Some("done"),
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
            30,
        )
        .unwrap_err();

    assert!(matches!(err, PlanError::OpenSubtasks { .. }));
}
