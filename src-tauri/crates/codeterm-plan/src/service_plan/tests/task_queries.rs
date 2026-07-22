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

#[test]
fn active_task_query_excludes_paused_but_keeps_pending() {
    let (store, id, paused_id) = store_with_task(&["status_update"]);
    let pending_id = store
        .add_task(
            &id,
            NewTask {
                name: "dependency-waiting".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .pause_task(
            &id,
            &paused_id,
            "operator hold",
            crate::plan::AgentRef::from_pane("orch"),
            3,
        )
        .unwrap();

    let active = store.list_tasks(TaskListFilter {
        active_only: true,
        ..Default::default()
    });
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].task.id, pending_id);

    let paused = store.list_tasks(TaskListFilter {
        status: Some(TaskStatus::Paused),
        ..Default::default()
    });
    assert_eq!(paused.len(), 1);
    assert_eq!(paused[0].task.id, paused_id);
    assert_eq!(
        paused[0].task.pause_reason.as_deref(),
        Some("operator hold")
    );
}

#[test]
fn new_task_defaults_to_actionable_pending() {
    let (store, plan_id, _) = store_with_task(&["status_update"]);
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "new task".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            4,
        )
        .unwrap();

    let detail = store.show_task(&task_id).expect("new task detail");
    assert_eq!(detail.task.status, TaskStatus::Pending);
    assert_eq!(
        detail.presentation,
        crate::service_plan::TaskPresentation::ActionablePending
    );
}

#[test]
fn task_detail_marks_unfinished_dependency_as_waiting_not_paused() {
    let (store, plan_id, dependency_id) = store_with_task(&["status_update"]);
    store
        .update_task(
            &plan_id,
            &dependency_id,
            None,
            None,
            None,
            None,
            Some("blocked"),
            Some(Some("external prerequisite")),
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
            4,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "after dependency".into(),
                kind: "implement".into(),
                depends_on: vec![dependency_id.clone()],
                competition: None,
                ..Default::default()
            },
            5,
        )
        .unwrap();

    let detail = store.show_task(&task_id).expect("dependent task detail");
    assert_eq!(detail.task.status, TaskStatus::Pending);
    assert_eq!(
        detail.presentation,
        crate::service_plan::TaskPresentation::WaitingOnDependencies
    );
    assert_eq!(detail.dependency_waits.len(), 1);
    assert_eq!(detail.dependency_waits[0].task_id, dependency_id);
}
