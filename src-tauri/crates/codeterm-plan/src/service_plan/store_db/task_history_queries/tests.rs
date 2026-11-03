use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;

#[test]
fn selected_history_is_cursor_bounded_ordered_scoped_and_read_only() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 75, 20);
    plan.tasks[1].milestones = std::mem::take(&mut plan.strides);
    let database = fixtures::database(root.path(), &plan);
    fixtures::audit(&database);
    let detail = database.task_detail(&plan.id, "fixture-0001").unwrap();
    assert_eq!(detail["task_id"], "task_0001");
    assert_eq!(detail["task"]["milestones"].as_array().unwrap().len(), 50);
    assert_eq!(
        detail["task"]["execution_attempts"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    let cursor = detail["history"]["milestones"].as_i64().unwrap();
    let older = database
        .task_history_page(
            &plan.id,
            "task_0001",
            TaskHistoryKind::Milestones,
            Some(cursor),
            50,
        )
        .unwrap();
    assert_eq!(older["revision"], detail["revision"]);
    assert_eq!(older["items"].as_array().unwrap().len(), 25);
    assert_eq!(older["items"][0]["id"], "history-0");
    assert_eq!(older["items"][24]["id"], "history-24");
    assert!(older["next_cursor"].is_null());
    let cursor = detail["history"]["attempts"].as_i64().unwrap();
    let older = database
        .task_history_page(
            &plan.id,
            "fixture-0001",
            TaskHistoryKind::Attempts,
            Some(cursor),
            50,
        )
        .unwrap();
    assert_eq!(older["items"].as_array().unwrap().len(), 12);
    assert_eq!(older["items"][0]["id"], "old-0");
    assert_eq!(older["items"][11]["id"], "old-11");
    assert!(older["next_cursor"].is_null());
    assert!(database
        .task_history_page("other", "task_0001", TaskHistoryKind::Milestones, None, 50)
        .is_err());
    assert!(fixtures::audit_counts(&database).is_empty());
}
