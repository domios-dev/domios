use super::*;
use crate::service_plan::change_feed::PlanChangeNotice;
use crate::service_plan::store_db::hot_write::{HotChange, TaskMetadata};
use crate::service_plan::store_db::{hot_write::tests::fixtures, PlanDatabase};

fn take(database: &PlanDatabase, plan_id: &str) -> CommittedPlanChange {
    match database.pending_changes.lock().take(plan_id).unwrap() {
        PlanChangeNotice::Committed(change) => change,
        PlanChangeNotice::Resync => panic!("unexpected resync"),
    }
}

fn role(value: &str) -> HotChange {
    HotChange::Task {
        selector: "task_0000".into(),
        change: TaskMetadata::RoleGuidance(Some(Some(value.into()))),
    }
}

#[test]
fn committed_revision_orders_hot_whole_rollback_restart_retention_and_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    let database = fixtures::database(directory.path(), &plan);
    let first = take(&database, &plan.id);
    assert!(first.reset);
    let hot = database
        .commit_hot_changes(&plan.id, vec![role("updated")], 0)
        .unwrap();
    let changed = take(&database, &plan.id);
    assert_eq!(changed.previous_revision, first.revision);
    assert_eq!(changed.task_ids, ["task_0000"]);
    assert_eq!(changed.revision, hot.revision.to_string());
    let board = database.task_board_page(&plan.id, None, 1, None).unwrap();
    let detail = database.task_detail(&plan.id, "task_0000").unwrap();
    assert_eq!(board["revision"], changed.revision);
    assert_eq!(detail["revision"], changed.revision);
    database.connection.lock().execute_batch(
        "CREATE TRIGGER refuse_header BEFORE UPDATE ON plans BEGIN SELECT RAISE(ABORT, 'refused header'); END;"
    ).unwrap();
    assert!(database
        .commit_hot_changes(&plan.id, vec![role("rolled back")], 999)
        .is_err());
    plan.title = "Rolled back title".into();
    assert!(database.persist_plan_rows(&plan, None).is_err());
    assert!(database.pending_changes.lock().take(&plan.id).is_none());
    assert_eq!(database.task_detail(&plan.id, "task_0000").unwrap(), detail);
    database
        .connection
        .lock()
        .execute_batch("DROP TRIGGER refuse_header;")
        .unwrap();
    database.prune_operations(1 << 62).unwrap();
    drop(database);
    let database = PlanDatabase::open(directory.path().join("plans.db")).unwrap();
    assert_eq!(
        database.task_board_page(&plan.id, None, 1, None).unwrap(),
        board
    );
    plan.title = "Committed title".into();
    database.persist_plan_rows(&plan, None).unwrap();
    let whole = take(&database, &plan.id);
    assert_eq!(whole.previous_revision, changed.revision);
    assert!(whole.revision.parse::<i64>().unwrap() > hot.revision);
    database.delete_plan(&plan.id, 0).unwrap();
    let deleted = take(&database, &plan.id);
    assert!(deleted.deleted);
    assert_eq!(deleted.previous_revision, whole.revision);
    assert!(database.task_board_page(&plan.id, None, 1, None).is_err());
    database.persist_plan_rows(&plan, None).unwrap();
    let recreated = take(&database, &plan.id);
    assert!(recreated.revision.parse::<i64>().unwrap() > deleted.revision.parse::<i64>().unwrap());
}

#[test]
fn committed_identity_and_existing_hot_write_work_are_bounded_by_changed_rows() {
    let mut prior = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let directory = tempfile::tempdir().unwrap();
        let plan = fixtures::fixture(directory.path(), strides, attempts);
        let database = fixtures::database(directory.path(), &plan);
        take(&database, &plan.id);
        fixtures::audit(&database);
        let receipt = database
            .commit_hot_changes(&plan.id, vec![role("updated")], 10)
            .unwrap();
        let change = take(&database, &plan.id);
        let event_bytes = serde_json::to_vec(&change).unwrap().len();
        let rows = fixtures::audit_counts(&database);
        assert_eq!(rows, [("plan_tasks".into(), 1), ("plans".into(), 1)]);
        assert!(receipt.decoded_bytes < 32768 && receipt.serialized_bytes < 32768);
        assert!(event_bytes < 256);
        println!("fixture={} tasks={} base_bytes={} strides={strides} unrelated_attempts={attempts} decoded_json_bytes={} serialized_row_bytes={} event_bytes={event_bytes} changed_rows={rows:?}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, receipt.decoded_bytes, receipt.serialized_bytes);
        let measured = (
            receipt.decoded_bytes,
            receipt.serialized_bytes,
            event_bytes,
            rows,
        );
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
