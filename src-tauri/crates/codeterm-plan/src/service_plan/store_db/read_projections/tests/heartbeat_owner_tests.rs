use super::*;
use crate::plan::{PlanMode, PlanStatus, TaskStatus};

#[test]
fn owner_resolution_preserves_mode_work_update_ties_and_explicit_ids() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    plan.id = "a".into();
    plan.team_id = None;
    plan.owner.tab_id = Some("owner".into());
    plan.owner.session_id = Some("session".into());
    plan.mode = PlanMode::Execution;
    plan.updated_at = 10;
    plan.tasks.truncate(1);
    plan.tasks[0].status = TaskStatus::Pending;
    let database = fixtures::database(directory.path(), &plan);
    let mut newer = plan.clone();
    newer.id = "z".into();
    newer.updated_at = 20;
    newer.mode = PlanMode::Planning;
    database.persist_plan(&newer, None).unwrap();
    let resolve = || {
        database
            .resolve_heartbeat_plan(None, Some("owner"), None)
            .unwrap()
    };
    assert_eq!(resolve(), "a");
    newer.mode = PlanMode::Execution;
    database.persist_plan(&newer, None).unwrap();
    assert_eq!(resolve(), "z");
    newer.updated_at = 10;
    database.persist_plan(&newer, None).unwrap();
    assert_eq!(resolve(), "a");
    newer.updated_at = 20;
    newer.tasks[0].status = TaskStatus::Done;
    database.persist_plan(&newer, None).unwrap();
    assert_eq!(resolve(), "a");
    plan.mode = PlanMode::Approved;
    database.persist_plan(&plan, None).unwrap();
    assert_eq!(resolve(), "z");
    newer.mode = PlanMode::Paused;
    database.persist_plan(&newer, None).unwrap();
    assert_eq!(resolve(), "a");
    plan.status = PlanStatus::Archived;
    database.persist_plan(&plan, None).unwrap();
    assert!(matches!(
        database.resolve_heartbeat_plan(None, Some("owner"), None),
        Err(PlanError::NotFound)
    ));
    assert_eq!(
        database
            .resolve_heartbeat_plan(Some("a"), None, None)
            .unwrap(),
        "a"
    );
    plan.status = PlanStatus::Active;
    database.persist_plan(&plan, None).unwrap();
    assert_eq!(
        database
            .resolve_heartbeat_plan(None, None, Some("session"))
            .unwrap(),
        "a"
    );
    assert!(matches!(
        database.resolve_heartbeat_plan(None, Some("absent"), None),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.mode',17) WHERE id='z'",
            [],
        )
        .unwrap();
    assert!(matches!(
        database.resolve_heartbeat_plan(None, Some("owner"), None),
        Err(PlanError::Io(_))
    ));
    assert_eq!(
        database
            .resolve_heartbeat_plan(Some("z"), None, None)
            .unwrap(),
        "z"
    );
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.mode','planning','$.owner.tab_id',17) WHERE id='z'",[]).unwrap();
    assert!(matches!(
        database.resolve_heartbeat_plan(None, Some("17"), None),
        Err(PlanError::Io(_))
    ));
}

#[test]
fn owner_resolution_materializes_only_the_selected_id_under_fixture_growth() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.owner.tab_id = Some("owner".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        let before = fixtures::audit_counts(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let id = database
            .resolve_heartbeat_plan(None, Some("owner"), None)
            .unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(id, plan.id);
        assert_eq!(metrics.queries, 1);
        assert_eq!(metrics.materialized_bytes, id.len());
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert_eq!(fixtures::audit_counts(&database), before);
        let response = serde_json::to_vec(&id).unwrap().len();
        println!("fixture={} consumer=heartbeat_owner tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}
