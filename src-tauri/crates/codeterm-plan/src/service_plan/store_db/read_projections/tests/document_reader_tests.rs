use super::*;

fn decorate(plan: &mut Plan) {
    plan.sections = sections();
    plan.loops = serde_json::from_value(json!([
        {"id":"selected","target":"task_0000","budget":5,"count":2,"status":"running","stagnation":0},
        {"id":"unselected","target":"task_0001","budget":5,"count":1,"status":"running","stagnation":0}
    ])).unwrap();
}

#[test]
fn document_markdown_and_loop_reads_preserve_output_and_selection_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = Plan::new(
        "First".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    first.id = "pl_first".into();
    first.updated_at = 2;
    decorate(&mut first);
    let database = fixtures::database(directory.path(), &first);
    let store = PlanStore::open(database.path()).unwrap();
    assert_eq!(
        store.spec_markdown(&first.id).unwrap(),
        crate::service_plan::body_files::spec_markdown(&first)
    );
    assert_eq!(store.loop_status("selected").unwrap(), first.loops[0]);
    assert_eq!(
        store.loop_plan_id("selected").unwrap(),
        Some(first.id.clone())
    );
    let metadata = store.document_metadata(&first.id).unwrap();
    assert_eq!(metadata.id, first.id);
    assert_eq!(metadata.project_path, first.project_path);
    assert!(store.loop_plan_id("missing").unwrap().is_none());
    assert!(matches!(
        store.loop_status("missing"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        store.spec_markdown("missing"),
        Err(PlanError::NotFound)
    ));
    let mut second = first.clone();
    second.id = "pl_second".into();
    second.updated_at = 3;
    second.loops[0].budget = 9;
    database.persist_plan_rows(&second, None).unwrap();
    assert_eq!(
        store.loop_plan_id("selected").unwrap(),
        Some(second.id.clone())
    );
    assert_eq!(store.loop_status("selected").unwrap().budget, 9);
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.loops[0].budget','invalid') WHERE id=?1",[&second.id]).unwrap();
    assert!(matches!(
        store.loop_status("selected"),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn document_and_loop_reads_ignore_task_bodies_attempts_strides_and_unselected_loops() {
    let mut prior = None;
    for (strides, attempts, extra, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        decorate(&mut plan);
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        plan.loops[1].deviation = Some("x".repeat(extra));
        for index in 0..history {
            let mut attempt =
                crate::service_plan::load_audit::attempt(&format!("closed-{index}"), "other");
            attempt.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(attempt);
        }
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = crate::service_plan::store_sync::take_decoded_row_bytes();
        take_metrics();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let markdown = store.spec_markdown(&plan.id).unwrap();
        let loop_spec = store.loop_status("selected").unwrap();
        let loop_plan = store.loop_plan_id("selected").unwrap();
        let metadata = store.document_metadata(&plan.id).unwrap();
        assert_eq!(
            markdown,
            crate::service_plan::body_files::spec_markdown(&plan)
        );
        assert_eq!(loop_spec, plan.loops[0]);
        assert_eq!(metadata.status, plan.status);
        assert_eq!(metadata.canonical_project_path, plan.canonical_project_path);
        assert!(store.plan_loads_for_test().is_empty());
        let response_bytes = serde_json::to_vec(&json!([markdown, loop_spec, loop_plan]))
            .unwrap()
            .len();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 5);
        assert!(metrics.materialized_bytes < 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=document_loop tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
