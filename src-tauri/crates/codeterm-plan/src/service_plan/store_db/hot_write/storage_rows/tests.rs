use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};

#[test]
fn path_mutations_materialize_only_paths_under_unrelated_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        drop(fixtures::database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let moved = root.path().join("moved").display().to_string();
        let update = store
            .set_project_paths(&plan.id, &moved, &moved, 20)
            .unwrap();
        assert!(update.changed);
        let work = super::super::take_work();
        let writes = fixtures::audit_counts(&store.database);
        let response_bytes = serde_json::to_vec(&update).unwrap().len();
        println!("fixture={} tasks={} base_content_bytes={} operation=path_repair strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} writes={writes:?} work={work:?} response_bytes={response_bytes}",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES);
        assert_eq!(
            (work[0], work[2], work[3], work[4], work[5]),
            (0, 0, 0, 0, 0)
        );
        assert!(work[6] == 0 && work[7] < 2048);
        assert!(store.plan_loads_for_test().is_empty());
        let measured = (work, writes, response_bytes);
        if let Some(prior) = prior {
            assert_eq!(prior, measured);
        }
        prior = Some(measured);
        let before = store.database.operation_count_for_test(Some(&plan.id));
        assert!(
            !store
                .set_project_paths(&plan.id, &moved, &moved, 21)
                .unwrap()
                .changed
        );
        assert_eq!(
            store.database.operation_count_for_test(Some(&plan.id)),
            before
        );
        fixtures::remove_observers(&store.database);
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.tasks, plan.tasks);
        assert_eq!(stored.strides, plan.strides);
        assert_eq!(stored.project_path, moved);
    }
}

#[test]
fn multi_plan_relocation_failure_rolls_back_every_path() {
    let root = tempfile::tempdir().unwrap();
    let mut first = Plan::new(
        "first".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    first.id = "a".into();
    let mut second = first.clone();
    second.id = "b".into();
    let database = fixtures::database(root.path(), &first);
    database.persist_plan_rows(&second, None).unwrap();
    fixtures::audit(&database);
    database.connection.lock().execute_batch("CREATE TRIGGER reject_second BEFORE UPDATE ON plans WHEN NEW.id='b' BEGIN SELECT RAISE(ABORT,'injected relocation failure'); END;").unwrap();
    assert!(database
        .relocate_plan_rows(&["owner".into()], &[], |_| Some("changed".into()), 20)
        .is_err());
    assert!(fixtures::audit_counts(&database).is_empty());
    assert_eq!(
        database.load_plan("a").unwrap().unwrap().project_path,
        first.project_path
    );
    assert_eq!(
        database.load_plan("b").unwrap().unwrap().project_path,
        second.project_path
    );
}

#[test]
fn explicit_import_and_retention_need_no_stored_plan_preimage() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixtures::fixture(root.path(), 2048, 2048);
    drop(fixtures::database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    load_audit::start(&store);
    let mut payload = plan.clone();
    payload.title = "explicit import".into();
    store.import_plan(&mut payload, 20).unwrap();
    assert!(store.plan_loads_for_test().is_empty());
    assert!(store
        .delete_if_untouched_since(&plan.id, 20)
        .unwrap()
        .is_none());
    assert!(store.plan_loads_for_test().is_empty());
    let deleted = store
        .delete_if_untouched_since(&plan.id, 21)
        .unwrap()
        .unwrap();
    assert_eq!(deleted.title, "explicit import");
    assert!(store.plan_loads_for_test().is_empty());
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert!(reopened.load_plan(&plan.id).unwrap().is_none());
    assert!(reopened.load_specs().unwrap().is_empty());
    println!("fixture={} operation=explicit_import_and_retention requested_payload_bytes={} unrelated_strides=2048 unrelated_attempts=2048 stored_plan_loads=0 rollback_boundary=one_entity_transaction",fixtures::SEED,serde_json::to_vec(&payload).unwrap().len());
}
