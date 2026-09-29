use super::*;

#[test]
fn a_task_assigned_group_is_declared_and_closable() {
    let (store, id, task_id) = store_with_task(&["status_update"]);
    store
        .update_task(
            &id,
            &task_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("release-1.11.10")),
            &[],
            None,
            None,
            3,
        )
        .unwrap();
    let plan = store.get(&id).unwrap();
    assert!(plan
        .task_groups
        .iter()
        .any(|group| { group.key == "release-1.11.10" && group.label == "release-1.11.10" }));
    store
        .with_plan(&id, 4, |plan| {
            plan.tasks[0].status = TaskStatus::Done;
            plan.tasks[0].status_changed_at = 4;
            Ok(())
        })
        .unwrap();
    store
        .close_group(
            &id,
            "release-1.11.10",
            "regression recorded",
            AgentRef::from_tab("owner"),
            5,
        )
        .unwrap();
}
