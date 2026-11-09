use crate::service_plan::locate::LocateTarget;
use crate::service_plan::store_db::{hot_write::tests::fixtures, read_projections::take_metrics};
use crate::service_plan::{load_audit, store_rows, store_sync, PlanStore};

#[test]
fn body_snapshot_bytes_ignore_unrelated_history_and_bodies() {
    let directory = tempfile::tempdir().unwrap();
    let mut prior = None;
    for (index, (strides, attempts, extra_body)) in
        [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)]
            .into_iter()
            .enumerate()
    {
        let root = directory.path().join(format!("case{index}"));
        std::fs::create_dir(&root).unwrap();
        let mut plan = fixtures::fixture(&root, strides, attempts);
        plan.tasks[0].body = "Requested body.\nSecond line.".into();
        plan.tasks[1].body.push_str(&"z".repeat(extra_body));
        let actual_bytes = serde_json::to_vec(&plan).unwrap().len();
        let database = fixtures::database(&root, &plan);
        drop(database);
        let store = PlanStore::open(root.join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        take_metrics();
        store_sync::take_decoded_row_bytes();
        store_rows::take_serialized_row_bytes();
        let location = store
            .locate_body(&plan.id, LocateTarget::Task("task_0000"))
            .unwrap();
        let metrics = take_metrics();
        let decoded = store_sync::take_decoded_row_bytes();
        let serialized = store_rows::take_serialized_row_bytes();
        let body_bytes = std::fs::read(&location.file).unwrap().len();
        let response_bytes = serde_json::to_vec(&location).unwrap().len();
        assert!(!location.editable);
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 128 && decoded < 128);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&store.database).is_empty());
        assert!(store.plan_loads_for_test().is_empty());
        println!("fixture={} tasks={} base_content_bytes={} actual_bytes={actual_bytes} consumer=locate_body strides={strides} unrelated_attempts={attempts} extra_body={extra_body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} snapshot_bytes={body_bytes} response_bytes={response_bytes} changed_rows=0 whole_plan_loads=0", fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, serialized, body_bytes, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(prior, &measured);
        }
        prior = Some(measured);
    }
}
