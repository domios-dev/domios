use super::*;
use crate::service_plan::ports::{TaskKeyPort, TaskKeyScope};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

struct RecordingPort(AtomicUsize);
impl TaskKeyPort for RecordingPort {
    fn issue_task_keys(&self, _: &TaskKeyScope, _: usize, _: &HashMap<String, u64>) -> Vec<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        vec!["AUD-1".into()]
    }
}

#[test]
fn key_read_failures_refuse_allocation_before_host_or_plan_writes() {
    let directory = tempfile::tempdir().unwrap();
    let plan = fixtures::fixture(directory.path(), 0, 1);
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    let port = Arc::new(RecordingPort(AtomicUsize::new(0)));
    store.bind_task_key_port(port.clone());
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.canonical_project_path',17) WHERE id=?1",[&plan.id]).unwrap();
    fixtures::audit(&database);
    assert!(matches!(
        store.issue_task_keys(&plan.id, 1),
        Err(PlanError::Io(_))
    ));
    assert_eq!(port.0.load(Ordering::SeqCst), 0);
    assert!(fixtures::audit_counts(&database).is_empty());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.key',17) WHERE plan_id=?1 AND task_id='task_0000'",[&plan.id]).unwrap();
    assert!(matches!(
        store.task_display_key(&plan.id, "task_0000"),
        Err(PlanError::Io(_))
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_tasks")
        .unwrap();
    assert!(matches!(store.task_key_high_water(), Err(PlanError::Io(_))));
    assert!(matches!(store.mint_id("ms"), Err(PlanError::Io(_))));
    assert_eq!(port.0.load(Ordering::SeqCst), 0);
}

#[test]
fn key_high_water_and_display_materialize_only_selected_keys_and_prefix_maxima() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].key = Some(format!("AUD-{}", u64::MAX));
        plan.tasks[1].key = Some("AUD-2".into());
        plan.tasks[2].key = Some("ZZ-4".into());
        plan.tasks[3].key = Some("AUD-0".into());
        plan.tasks[4].key = Some("AUD-18446744073709551616".into());
        plan.tasks[0].name.push_str(&"n".repeat(body));
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[0].milestones = plan.strides.clone();
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let high_water = store.task_key_high_water().unwrap();
        assert_eq!(
            high_water,
            HashMap::from([("AUD".into(), u64::MAX), ("ZZ".into(), 4)])
        );
        let mut keys = Vec::new();
        for reference in ["task_0000", "fixture-0000"] {
            keys.push(
                store
                    .task_display_key(&plan.id, reference)
                    .unwrap()
                    .unwrap(),
            );
        }
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 256);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(store.plan_loads_for_test().is_empty());
        let response = serde_json::to_vec(&(high_water, keys)).unwrap().len();
        println!("fixture={} consumer=task_key_read tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} selected_extra_body_and_name={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}
