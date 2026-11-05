use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;

#[test]
fn competition_lane_facts_preserve_canonical_owner_intent_foreign_order_and_handle_skip() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.canonical_project_path = "\u{2003}".into();
    plan.tasks[0].interruption.execution_intent = ExecutionIntent::Mutating;
    plan.tasks[1].lane.worktree_path = Some("/foreign-first".into());
    plan.tasks[2].lane.worktree_path = Some("/skipped".into());
    plan.tasks[2].handle = Some("task_0000".into());
    plan.tasks[3].lane.worktree_path = Some("/foreign-last".into());
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .competition_lane_projection(&plan.id, "task_0000")
        .unwrap();
    assert_eq!(read.canonical_path, plan.project_path);
    assert_eq!(read.dispatch_intent, ExecutionIntent::Mutating);
    assert_eq!(read.owner_tab_id, plan.owner.tab_id);
    assert_eq!(
        read.foreign_lanes
            .iter()
            .map(|lane| lane.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_0001", "task_0003"]
    );
    assert_eq!(
        database
            .competition_lane_projection(&plan.id, "FIXTURE-0000")
            .unwrap(),
        read
    );
    assert!(matches!(
        database.competition_lane_projection("missing", "task_0000"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.competition_lane_projection(&plan.id, "missing"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.execution_intent','broken') WHERE task_id='task_0000'",[]).unwrap();
    assert!(database
        .competition_lane_projection(&plan.id, "task_0000")
        .is_err());
}

#[test]
fn compete_fixture_selects_lane_safety_without_serialization_or_writes() {
    let mut prior = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].lane.worktree_path = Some("/foreign".into());
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = database
            .competition_lane_projection(&plan.id, "task_0000")
            .unwrap();
        assert_eq!(read.foreign_lanes.len(), 1);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=compete tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
