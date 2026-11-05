use super::*;
use crate::plan::{Goal, TaskStatus};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use serde_json::json;

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
        assert!(projection.get("strides").is_none());
        let tasks = projection["tasks"].as_array().unwrap();
        assert!(tasks.iter().all(|task| task.get("body").is_none()
            && task.get("milestones").is_none()
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
                assert!(projection["goal"].get("vision").is_none());
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
                assert!(projection["sections"][0].get("body").is_none());
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
        .reference_projection(&plan.id, &["fixture-0001".into()], &["section".into()])
        .unwrap();
    assert_eq!(reference.tasks.len(), 1);
    assert_eq!(reference.tasks[0].id, "task_0001");
    assert!(serde_json::to_value(&reference.tasks[0])
        .unwrap()
        .get("body")
        .is_none());
    assert_eq!(reference.sections[0].title, "Requested title");
    assert!(serde_json::to_value(&reference.sections[0])
        .unwrap()
        .get("body")
        .is_none());
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

#[test]
fn reference_batch_sql_hot_1162_two_plans_has_invariant_bytes_and_zero_plan_loads() {
    let mut baseline = None;
    for (strides, attempts, body) in [(0, 0, 0), (2048, 2048, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[100].body.push_str(&"x".repeat(body));
        let store = crate::service_plan::PlanStore::new_for_test(root.path());
        store.database.persist_plan_rows(&plan, None).unwrap();
        let mut second = plan.clone();
        second.id = "second-hot-plan".into();
        store.database.persist_plan_rows(&second, None).unwrap();
        crate::service_plan::load_audit::start(&store);
        super::super::read_projections::take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        let ids: Vec<String> = (0..10).map(|i| format!("task_{i:04}")).collect();
        for id in [&plan.id, &second.id] {
            let selected = store.reference_projection(id, &ids, &[]).unwrap();
            assert_eq!(selected.tasks.len(), 10);
            assert_eq!(selected.tasks[0].id, ids[0]);
            assert!(selected
                .tasks
                .iter()
                .all(|task| serde_json::to_value(task).unwrap().get("body").is_none()));
        }
        let metrics = super::super::read_projections::take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 8192);
        assert!(decoded < 8192);
        assert!(store.plan_loads_for_test().is_empty());
        let measured = (metrics.queries, metrics.materialized_bytes, decoded);
        if let Some(baseline) = baseline {
            assert_eq!(measured, baseline);
        }
        baseline = Some(measured);
    }
}

#[test]
fn reference_batch_refuses_ambiguous_alias_even_when_an_exact_sibling_is_requested() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 0, 0);
    plan.tasks.truncate(3);
    plan.tasks[0].key = Some("KEY-1".into());
    plan.tasks[0].handle = Some("Mixed-Handle".into());
    plan.tasks[1].handle = Some("KEY-1".into());
    plan.tasks[1].key = Some("KEY-2".into());
    plan.tasks[2].handle = Some(plan.tasks[0].id.clone());
    let database = fixtures::database(root.path(), &plan);
    let exact = plan.tasks[0].id.clone();
    let refs = vec![
        "key-1".into(),
        exact.clone(),
        "mixed-handle".into(),
        "KEY-2".into(),
        "missing".into(),
    ];
    let read = database.reference_projection(&plan.id, &refs, &[]).unwrap();
    assert!(read.task("key-1").is_none());
    assert_eq!(read.task(&exact).unwrap().id, exact);
    assert_eq!(read.task("mixed-handle").unwrap().id, exact);
    assert_eq!(read.task("KEY-2").unwrap().id, plan.tasks[1].id);
    assert!(read.task("missing").is_none());
}

#[test]
fn ui_views_bound_selected_bytes_and_never_write_or_load_a_plan() {
    let mut baseline = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 2048, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[0].assigned_tab_id = Some("visible-worker".into());
        plan.tasks[0].status = TaskStatus::InProgress;
        let unrelated = std::mem::take(&mut plan.tasks[1].execution_attempts);
        plan.tasks.last_mut().unwrap().execution_attempts = unrelated;
        plan.tasks[100].body.push_str(&"x".repeat(body_bytes));
        let store = crate::service_plan::PlanStore::new_for_test(root.path());
        store.database.persist_plan_rows(&plan, None).unwrap();
        crate::service_plan::load_audit::start(&store);
        fixtures::audit(&store.database);
        let mut measured = Vec::new();
        for view in [
            PlanViewProjection::Summary,
            PlanViewProjection::Board,
            PlanViewProjection::Document,
            PlanViewProjection::List,
            PlanViewProjection::TaskMetadata,
        ] {
            super::super::read_projections::take_metrics();
            crate::service_plan::store_sync::take_decoded_row_bytes();
            crate::service_plan::store_rows::take_serialized_row_bytes();
            let value = store.ui_plan_projection(&plan.id, view).unwrap();
            let payload = serde_json::to_vec(&value).unwrap().len();
            let metrics = super::super::read_projections::take_metrics();
            let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
            let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
            assert!(payload < 65536, "{view:?}: payload={payload}");
            assert!(
                metrics.materialized_bytes < 65536,
                "{view:?}: materialized={}",
                metrics.materialized_bytes
            );
            assert!(decoded < 65536, "{view:?}: decoded={decoded}");
            assert_eq!(serialized, 0);
            assert_eq!(value["revision"], "1");
            assert!(store.plan_loads_for_test().is_empty());
            assert!(fixtures::audit_counts(&store.database).is_empty());
            println!("fixture={} surface={view:?} payload_bytes={payload} decoded_json_bytes={decoded} serialized_row_bytes={serialized} materialized_text_bytes={} changed_rows=0", fixtures::SEED, metrics.materialized_bytes);
            measured.push((
                metrics.queries,
                metrics.materialized_bytes,
                decoded,
                payload,
            ));
        }
        if let Some(baseline) = baseline {
            assert_eq!(measured, baseline);
        }
        baseline = Some(measured);
    }
}

#[test]
fn ui_projection_decode_errors_are_not_replaced_with_empty_views() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixtures::fixture(root.path(), 0, 0);
    let database = fixtures::database(root.path(), &plan);
    database.connection.lock().execute(
        "UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',NULL) WHERE plan_id=?1 AND task_id='task_0000'",
        [&plan.id],
    ).unwrap();
    fixtures::audit(&database);
    assert!(database
        .ui_plan_list_window(&plan.id, &Default::default())
        .is_err());
    assert!(database
        .ui_plan_metadata_window(&plan.id, &Default::default())
        .is_err());
    assert!(fixtures::audit_counts(&database).is_empty());
}
