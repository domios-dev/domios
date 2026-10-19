use super::*;

#[test]
fn tab_todo_link_is_one_idempotent_audit_event_without_task_mutation() {
    let (store, plan_id, task_id) = store_with_task(&[]);
    let before = store.get(&plan_id).unwrap();
    let task_before = before
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap()
        .clone();
    let event_id = store
        .record_tab_todo_linked(
            &plan_id,
            &task_id,
            "todo-item-1",
            "Review source",
            "worker",
            30,
        )
        .unwrap();
    assert_eq!(event_id, "tab_todo_todo-item-1");
    assert_eq!(
        store
            .record_tab_todo_linked(
                &plan_id,
                &task_id,
                "todo-item-1",
                "Review source",
                "worker",
                31
            )
            .unwrap(),
        event_id
    );
    let after = store.get(&plan_id).unwrap();
    let task_after = after.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task_after.status, task_before.status);
    assert_eq!(task_after.assigned_tab_id, task_before.assigned_tab_id);
    assert_eq!(task_after.review_stage, task_before.review_stage);
    assert_eq!(
        task_after.execution_attempts,
        task_before.execution_attempts
    );
    assert_eq!(
        task_after
            .milestones
            .iter()
            .filter(|milestone| milestone.kind == "tab_todo_linked")
            .count(),
        1
    );
    let event_log = crate::service_plan::state::mission_control::build_plan_event_log(&after);
    assert!(event_log.entries.iter().any(|event| {
        event.kind == "tab_todo_linked" && event.task_id.as_deref() == Some(task_id.as_str())
    }));
}
