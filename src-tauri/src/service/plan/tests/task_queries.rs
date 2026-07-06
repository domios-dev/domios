use super::*;

#[test]
fn task_queries_exclude_archived_plans() {
    let (store, id, _) = store_with_task(&["status_update"]);
    store.archive(&id, 99).unwrap();

    let tasks = store.list_tasks(TaskListFilter {
        active_only: false,
        ..Default::default()
    });

    assert!(tasks.is_empty());
    assert!(store.coverage_report(&id).is_none());
}

#[test]
fn task_queries_filter_by_status() {
    let (store, id, blocked_id) = store_with_task(&["status_update"]);
    store
        .add_task(
            &id,
            NewTask {
                name: "still-pending".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .update_task(
            &id,
            &blocked_id,
            None,
            None,
            None,
            None,
            Some("blocked"),
            Some(Some("waiting on dependency")),
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
            3,
        )
        .unwrap();

    let tasks = store.list_tasks(TaskListFilter {
        active_only: false,
        status: Some(TaskStatus::Blocked),
        ..Default::default()
    });

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task.id, blocked_id);
}
