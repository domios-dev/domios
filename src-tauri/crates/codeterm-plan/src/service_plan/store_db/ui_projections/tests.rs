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
            && task["execution_attempts"].as_array().unwrap().len() <= 1));
        match view {
            PlanViewProjection::Chat | PlanViewProjection::TaskMetadata => {
                panic!("chat is covered by its own projection oracle")
            }
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
                assert_eq!(tasks.len(), 25);
                assert!(projection["task_window"]["next_cursor"].is_object());
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

#[test]
fn references_materialize_only_requested_labels_without_bodies_or_histories() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 2048, 2048);
    plan.sections = serde_json::from_value(
        json!([{"id":"section","title":"Requested title","body":"Unrequested body"}]),
    )
    .unwrap();
    let database = fixtures::database(root.path(), &plan);
    fixtures::audit(&database);
    super::super::read_projections::take_metrics();
    let reference = database
        .reference_projection(&plan.id, Some("fixture-0001"), Some("section"))
        .unwrap();
    assert_eq!(reference.tasks.len(), 1);
    assert_eq!(reference.tasks[0].id, "task_0001");
    assert!(reference.tasks[0].body.is_empty() && reference.tasks[0].execution_attempts.is_empty());
    assert_eq!(reference.sections[0].title, "Requested title");
    assert!(reference.sections[0].body.is_empty() && reference.strides.is_empty());
    assert!(super::super::read_projections::take_metrics().materialized_bytes < 4096);
    assert!(fixtures::audit_counts(&database).is_empty());
}

#[test]
fn task_reference_metadata_scopes_keys_and_preserves_global_canonical_identity() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 0, 0);
    plan.tasks[0].key = Some("CT-1".into());
    let database = fixtures::database(root.path(), &plan);
    let project = plan.project_path.as_str();
    let by_key = database
        .task_reference("ct-1", Some(project), None)
        .unwrap();
    assert_eq!(by_key["task_id"], "task_0000");
    assert!(by_key.get("body").is_none());
    assert!(database.task_reference("CT-1", None, None).is_err());
    assert_eq!(
        database
            .task_reference("TASK_0000", Some("foreign"), None)
            .unwrap()["task_id"],
        "task_0000"
    );
    let mut other = plan.clone();
    other.id = "other-plan".into();
    other.tasks.truncate(1);
    other.tasks[0].id = "other-task".into();
    database.persist_plan_rows(&other, None).unwrap();
    assert!(database
        .task_reference("ct-1", Some(project), None)
        .is_err());
    other.tasks[0].key = Some("task_0000".into());
    database.persist_plan_rows(&other, None).unwrap();
    assert_eq!(
        database
            .task_reference("task_0000", Some(project), None)
            .unwrap()["plan_id"],
        plan.id
    );
}
