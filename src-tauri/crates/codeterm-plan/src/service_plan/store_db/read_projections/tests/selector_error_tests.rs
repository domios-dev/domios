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
    database
        .connection
        .lock()
        .execute_batch("ALTER TABLE plans RENAME TO unavailable_plan_rows")
        .unwrap();
    assert!(store.curator_bootstrap_plan_ids().is_err());
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
