use super::*;
use crate::plan::{Goal, TaskStatus};
use crate::service_plan::store_db::hot_write::tests::fixtures;

#[test]
fn ui_projections_expose_only_the_requested_document_or_visible_task_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.goal = Some(Goal {
        vector: "Visible goal".into(),
        vision: "Document vision".into(),
        ..Default::default()
    });
    plan.sections = serde_json::from_value(
        json!([{"id":"section","title":"Visible section","body":"Document section body"}]),
    )
    .unwrap();
    plan.tasks[0].assigned_tab_id = Some("worker".into());
    plan.tasks[0].status = TaskStatus::InProgress;
    let database = fixtures::database(directory.path(), &plan);
    fixtures::audit(&database);
    for view in [
        PlanViewProjection::Summary,
        PlanViewProjection::Board,
        PlanViewProjection::Document,
        PlanViewProjection::List,
    ] {
        let projection = database.ui_plan_projection(&plan.id, view).unwrap();
        assert_eq!(projection["revision"], "1");
        assert_eq!(projection["strides"], json!([]));
        let tasks = projection["tasks"].as_array().unwrap();
        assert!(tasks.iter().all(|task| task["body"] == ""
            && task["milestones"] == json!([])
            && task["execution_attempts"] == json!([])));
        match view {
            PlanViewProjection::Summary => {
                assert!(tasks.is_empty());
                assert!(projection["goal"].is_null());
            }
            PlanViewProjection::Board => {
                assert_eq!(tasks.len(), 1);
                assert_eq!(tasks[0]["id"], "task_0000");
                assert_eq!(projection["goal"]["vector"], "Visible goal");
                assert_eq!(projection["goal"]["vision"], "");
            }
            PlanViewProjection::Document => {
                assert!(tasks.is_empty());
                assert_eq!(projection["sections"][0]["body"], "Document section body");
                assert_eq!(projection["goal"]["vision"], "Document vision");
            }
            PlanViewProjection::List => {
                assert_eq!(tasks.len(), fixtures::TASKS);
                assert_eq!(projection["sections"][0]["title"], "Visible section");
                assert_eq!(projection["sections"][0]["body"], "");
            }
        }
    }
    assert!(database
        .ui_plan_summaries(Some("unrelated-project"))
        .unwrap()
        .is_empty());
    assert!(fixtures::audit_counts(&database).is_empty());
}
