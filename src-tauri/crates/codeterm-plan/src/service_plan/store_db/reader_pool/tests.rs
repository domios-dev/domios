use crate::plan::{OrchestratorRef, Plan, PlanStatus, TaskStatus};
use crate::service_plan::{NewTask, PlanStore};
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn seed_plan(store: &PlanStore, id: &str, tab: &str) {
    let project = crate::service_plan::test_support::test_data_dir().join(id);
    let mut plan = Plan::new(
        format!("Plan {id}"),
        project.to_string_lossy().into_owned(),
        OrchestratorRef::from_tab("owner-tab"),
    );
    plan.id = id.into();
    plan.status = PlanStatus::Active;
    store.save(&mut plan, 100).unwrap();
    let open_id = store
        .add_task(
            id,
            NewTask {
                name: "open task".into(),
                kind: "work".into(),
                ..Default::default()
            },
            101,
        )
        .unwrap();
    let done_id = store
        .add_task(
            id,
            NewTask {
                name: "done task".into(),
                kind: "work".into(),
                ..Default::default()
            },
            102,
        )
        .unwrap();
    let mut saved = store.get(id).unwrap();
    for task in &mut saved.tasks {
        if task.id == open_id {
            task.status = TaskStatus::InProgress;
            task.assigned_tab_id = Some(tab.to_string());
        } else if task.id == done_id {
            task.status = TaskStatus::Done;
            task.assigned_tab_id = Some("finished-tab".to_string());
        }
    }
    store.save(&mut saved, 103).unwrap();
}

#[test]
fn wal_reader_pool_serves_plan_list_index_while_writer_connection_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    seed_plan(&store, "pl_wal_concurrent", "worker-tab-1");

    let writer_guard = store.database.connection.lock();
    let reader_store = store.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let entries = reader_store
            .database
            .plan_list_index(None, None, false, None)
            .unwrap();
        done_tx.send(entries).unwrap();
    });

    let entries = done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("WAL reader pool must serve plan_list_index without waiting on the writer mutex");
    drop(writer_guard);
    reader.join().unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].plan_id, "pl_wal_concurrent");
    assert_eq!(entries[0].task_count, 2);
    assert_eq!(entries[0].open_task_count, 1);
    assert_eq!(entries[0].assigned_tab_ids, vec!["worker-tab-1"]);
}

#[test]
fn plan_list_index_uses_indexed_assigned_tabs_and_covering_status_summary() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    seed_plan(&store, "pl_indexed_eqp", "worker-tab-2");

    let connection = store.database.read_connection();
    let mut stmt = connection
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT t.plan_id, trim(t.assigned_tab_id)
             FROM plan_tasks t INDEXED BY plan_tasks_assigned_tab_id
             WHERE t.assigned_tab_id IS NOT NULL
               AND t.status NOT IN ('done', 'cancelled')
               AND trim(t.assigned_tab_id) <> ''
             ORDER BY t.plan_id, t.position",
        )
        .unwrap();
    let assigned_plan: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        assigned_plan
            .iter()
            .any(|detail| detail.contains("plan_tasks_assigned_tab_id")),
        "expected plan_tasks_assigned_tab_id in query plan: {assigned_plan:?}"
    );
    assert!(
        !assigned_plan.iter().any(|detail| detail.contains("SCAN t")),
        "assigned tab query must not full-scan plan_tasks: {assigned_plan:?}"
    );

    let mut stmt = connection
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT p.id,
                (SELECT count(*) FROM plan_tasks t INDEXED BY plan_tasks_status_summary WHERE t.plan_id = p.id),
                (SELECT count(*) FROM plan_tasks t INDEXED BY plan_tasks_status_summary WHERE t.plan_id = p.id AND json_extract(t.row_json, '$.status') IN ('pending', 'in_progress', 'in_review', 'blocked'))
             FROM plans p",
        )
        .unwrap();
    let counts_plan: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        counts_plan
            .iter()
            .any(|detail| detail.contains("plan_tasks_status_summary")),
        "expected plan_tasks_status_summary in query plan: {counts_plan:?}"
    );
    assert!(
        !counts_plan.iter().any(|detail| detail.contains("SCAN t")),
        "open task count query must not full-scan plan_tasks: {counts_plan:?}"
    );
}
