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
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        let unrelated = std::mem::take(&mut plan.tasks[1].execution_attempts);
        plan.tasks.last_mut().unwrap().execution_attempts = unrelated;
        let database = fixtures::database(directory.path(), &plan);
        take(&database, &plan.id);
        fixtures::audit(&database);
        super::super::read_projections::take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let summaries = database.ui_plan_summaries(None).unwrap();
        let board = database
            .ui_plan_projection(
                &plan.id,
                crate::service_plan::ui_projections::PlanViewProjection::Board,
            )
            .unwrap();
        let page = database.task_board_page(&plan.id, None, 25, None).unwrap();
        let initial_bytes = serde_json::to_vec(&(summaries, board, page)).unwrap().len();
        let projection_metrics = super::super::read_projections::take_metrics();
        let initial_decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let initial_serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(initial_bytes < 64 * 1024);
        assert!(initial_decoded < 64 * 1024 && initial_serialized == 0);
        let receipt = database
            .commit_hot_changes(&plan.id, vec![role("updated")], 10)
            .unwrap();
        let change = take(&database, &plan.id);
        let event_bytes = serde_json::to_vec(&change).unwrap().len();
        let rows = fixtures::audit_counts(&database);
        assert_eq!(rows, [("plan_tasks".into(), 1), ("plans".into(), 1)]);
        assert!(receipt.decoded_bytes < 32768 && receipt.serialized_bytes < 32768);
        assert!(event_bytes < 256);
        super::super::read_projections::take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let updated_summary = database
            .ui_plan_projection(
                &plan.id,
                crate::service_plan::ui_projections::PlanViewProjection::Summary,
            )
            .unwrap();
        let updated_detail = database.task_detail(&plan.id, "task_0000").unwrap();
        let update_payload_bytes = serde_json::to_vec(&(&change, updated_summary, updated_detail))
            .unwrap()
            .len();
        let update_decoded_bytes = crate::service_plan::store_sync::take_decoded_row_bytes();
        let update_serialized_rows = crate::service_plan::store_rows::take_serialized_row_bytes();
        let update_materialized_bytes =
            super::super::read_projections::take_metrics().materialized_bytes;
        assert!(
            update_payload_bytes < 64 * 1024
                && update_decoded_bytes < 64 * 1024
                && update_serialized_rows == 0
        );
        println!("fixture={} tasks={} base_bytes={} strides={strides} unrelated_attempts={attempts} unrelated_history_task=task_1161 page_size=25 initial_bytes={initial_bytes} initial_decoded_json_bytes={initial_decoded} initial_serialized_row_bytes={initial_serialized} materialized_projection_text_bytes={} decoded_json_bytes={} serialized_row_bytes={} event_bytes={event_bytes} changed_rows={rows:?} update_payload_bytes={update_payload_bytes} update_decoded_json_bytes={update_decoded_bytes} update_serialized_row_bytes={update_serialized_rows} update_materialized_text_bytes={update_materialized_bytes}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, projection_metrics.materialized_bytes, receipt.decoded_bytes, receipt.serialized_bytes);
        let measured = (
            receipt.decoded_bytes,
            receipt.serialized_bytes,
            event_bytes,
            rows,
            initial_bytes,
            initial_decoded,
            initial_serialized,
            projection_metrics,
            update_payload_bytes,
            update_decoded_bytes,
            update_serialized_rows,
            update_materialized_bytes,
        );
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
