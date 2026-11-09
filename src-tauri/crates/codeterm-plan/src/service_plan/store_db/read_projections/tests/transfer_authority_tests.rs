use super::*;

#[test]
fn transfer_authority_selects_constant_bytes_without_writes() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[0].lane.worktree_path = Some("lane".into());
        plan.tasks[0].execution_attempts[0].worktree_path = Some("lane".into());
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database
            .transfer_task_authority_projection(&plan.id, &plan.tasks[0].id, "current")
            .unwrap();
        let identity = database.plan_identity_projection(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(read.lane.worktree_path, plan.tasks[0].lane.worktree_path);
        assert_eq!(
            read.interruption.owner_epoch,
            plan.tasks[0].interruption.owner_epoch
        );
        assert_eq!(read.attempt.as_ref().unwrap().executor_tab_id, "worker");
        assert_eq!(identity.owner, plan.owner);
        assert_eq!(metrics.queries, 2);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(read, identity)).unwrap().len();
        println!("fixture={} consumer=transfer_task_bound_worker tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn transfer_authority_keeps_first_duplicate_attempt_and_missing_task_attempt_distinction() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    let mut duplicate = plan.tasks[0].execution_attempts[0].clone();
    duplicate.executor_tab_id = "later".into();
    plan.tasks[0].execution_attempts.push(duplicate);
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database
            .transfer_task_authority_projection(&plan.id, &plan.tasks[0].id, "current")
            .unwrap()
            .attempt
            .unwrap()
            .executor_tab_id,
        "worker"
    );
    assert!(database
        .transfer_task_authority_projection(&plan.id, &plan.tasks[0].id, "missing")
        .unwrap()
        .attempt
        .is_none());
    assert!(matches!(
        database.transfer_task_authority_projection(&plan.id, "missing", "current"),
        Err(PlanError::TaskNotFound)
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_task_attempts")
        .unwrap();
    assert!(database
        .transfer_task_authority_projection(&plan.id, &plan.tasks[0].id, "current")
        .is_err());
}
