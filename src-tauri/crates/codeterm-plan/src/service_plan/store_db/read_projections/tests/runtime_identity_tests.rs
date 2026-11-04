use super::*;
use crate::plan::{PlanStatus, PlannerRef};

#[test]
fn runtime_identity_reads_select_constant_bytes_without_writes() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.team_id = Some("team".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let identity = database.runtime_plan_identity_projection(&plan.id).unwrap();
        let bound = database
            .runtime_bound_plan_projection("member", Some("team"))
            .unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(identity.owner, plan.owner);
        assert_eq!(identity.team_id, plan.team_id);
        assert_eq!(identity.project_path, plan.project_path);
        assert_eq!(identity.id, plan.id);
        assert_eq!(bound, Some(plan.id));
        assert_eq!(metrics.queries, 2);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(identity, bound)).unwrap().len();
        println!("fixture={} consumers=prepare_runtime_identity,bound_plan_id tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, metrics.queries, metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn runtime_bound_plan_keeps_newest_plan_owner_planner_and_team_rules() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Runtime".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.id = "a".into();
    plan.team_id = Some("team".into());
    plan.status = PlanStatus::Archived;
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: None,
        title: None,
    });
    let database = fixtures::database(directory.path(), &plan);
    for (tab, team, expected) in [
        ("owner", None, Some("a")),
        ("planner", None, Some("a")),
        ("member", Some("team"), Some("a")),
        ("member", None, None),
        ("member", Some("other"), None),
    ] {
        assert_eq!(
            database
                .runtime_bound_plan_projection(tab, team)
                .unwrap()
                .as_deref(),
            expected
        );
    }
    let mut newer = plan.clone();
    newer.id = "z".into();
    newer.updated_at += 1;
    database.persist_plan_rows(&newer, None).unwrap();
    assert_eq!(
        database
            .runtime_bound_plan_projection("owner", None)
            .unwrap(),
        Some("z".into())
    );
    newer.updated_at = plan.updated_at;
    database.persist_plan_rows(&newer, None).unwrap();
    assert_eq!(
        database
            .runtime_bound_plan_projection("owner", None)
            .unwrap(),
        Some("a".into())
    );
}

#[test]
fn runtime_identity_propagates_storage_and_selected_field_errors() {
    let directory = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "Runtime".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let database = fixtures::database(directory.path(), &plan);
    assert!(matches!(
        database.runtime_plan_identity_projection("missing"),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',17) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.runtime_plan_identity_projection(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plans")
        .unwrap();
    assert!(database.runtime_plan_identity_projection(&plan.id).is_err());
    assert!(database
        .runtime_bound_plan_projection("owner", None)
        .is_err());
}
