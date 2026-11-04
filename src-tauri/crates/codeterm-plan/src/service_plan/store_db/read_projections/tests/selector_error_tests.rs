use super::*;

#[test]
fn loop_preflight_and_curator_bootstrap_refuse_unavailable_storage() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Selected".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.loops = serde_json::from_value(json!([
        {"id":"selected","target":"task","budget":5,"count":0,
         "status":"running","stagnation":0}
    ]))
    .unwrap();
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    assert_eq!(
        store.curator_bootstrap_plan_ids().unwrap(),
        vec![plan.id.clone()]
    );
    assert!(store.plan_loads_for_test().is_empty());
    database.connection.lock().execute(
        "UPDATE plans SET team_id='committed-team',header_json=json_set(header_json,'$.team_id','committed-team') WHERE id=?1",
        [&plan.id],
    ).unwrap();
    assert_eq!(
        store.active_plan_team_bindings().unwrap(),
        vec![(plan.id.clone(), Some("committed-team".into()))]
    );
    assert_eq!(
        store
            .plan_team_binding_metadata(&plan.id)
            .unwrap()
            .unwrap()
            .1
            .as_deref(),
        Some("committed-team")
    );
    database
        .connection
        .lock()
        .execute_batch("ALTER TABLE plans RENAME TO unavailable_plan_rows")
        .unwrap();
    assert!(store.curator_bootstrap_plan_ids().is_err());
    assert!(store.active_plan_team_bindings().is_err());
    assert!(store.list_index().is_err());
    assert!(store.plan_team_binding_metadata(&plan.id).is_err());
    assert!(store.plan_ids_with_unsettled_lanes().is_err());
    database
        .connection
        .lock()
        .execute_batch("ALTER TABLE plan_task_attempts RENAME TO unavailable_attempt_rows")
        .unwrap();
    assert!(store.attempt_lane_paths().is_err());
    for result in [
        store.set_loop("selected", Some(10), None, 10),
        store.stop_loop("selected", 10),
        store.advance_loop("selected", true, 10).map(|_| ()),
        store
            .record_loop("selected", "note", false, None, 10)
            .map(|_| ()),
    ] {
        assert!(result.is_err());
        assert!(!matches!(result, Err(PlanError::NotFound)));
    }
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn attempt_lane_paths_ignore_unrelated_history_and_refuse_invalid_selected_paths() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].execution_attempts[0].worktree_path = Some("/lane/selected".into());
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let rows = database.attempt_lane_paths().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].attempt_id, "current");
        assert_eq!(rows[0].worktree_path, "/lane/selected");
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response =
            serde_json::to_vec(&json!({"attempt":rows[0].attempt_id,"path":rows[0].worktree_path}))
                .unwrap()
                .len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 256);
        assert!(decoded < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=attempt_lane_paths tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
        database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.worktree_path',17) WHERE plan_id=?1 AND task_id='task_0000'",[&plan.id]).unwrap();
        assert!(matches!(
            database.attempt_lane_paths(),
            Err(PlanError::Quarantined { .. })
        ));
    }
}

#[test]
fn selected_reference_fields_refuse_invalid_types_without_missing_fallbacks() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Selected".into(),
        "project".into(),
        OrchestratorRef::default(),
    );
    plan.sections = serde_json::from_value(json!([{"id":"section","title":"Section"}])).unwrap();
    let database = fixtures::database(directory.path(), &plan);
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.canonical_project_path',null) WHERE id=?1",[&plan.id]).unwrap();
    assert!(database.plan_roots().is_err());
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.canonical_project_path','', '$.loops',json(?2)) WHERE id=?1",params![plan.id,json!([{"id":42,"target":"task"}]).to_string()]).unwrap();
    assert!(database.loop_rows().is_err());
    for (path, value) in [
        ("$.id", json!(42)),
        ("$.handle", json!({})),
        ("$.title", json!([])),
    ] {
        let original = serde_json::to_string(&plan.sections[0]).unwrap();
        database
            .connection
            .lock()
            .execute(
                "UPDATE plan_sections SET row_json=json_set(?2,?3,json(?4)) WHERE plan_id=?1",
                params![plan.id, original, path, value.to_string()],
            )
            .unwrap();
        assert!(database.section_ref_rows().is_err());
    }
}
