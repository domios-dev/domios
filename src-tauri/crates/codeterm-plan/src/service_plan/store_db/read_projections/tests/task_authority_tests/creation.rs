use super::*;

#[test]
fn creation_response_preserves_keys_link_order_missing_and_decode_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].section_ids = vec!["last".into(), "first".into()];
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .task_creation_projection(&plan.id, "NAMED")
        .unwrap()
        .unwrap();
    assert_eq!(read.key, "COD-1");
    assert_eq!(read.section_ids, ["last", "first"]);
    let description = database.task_description_projection("NAMED").unwrap();
    assert_eq!(description.name, plan.tasks[0].name);
    assert_eq!(description.assigned_tab_id.as_deref(), Some("worker"));
    plan.tasks[0].key = None;
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .task_creation_projection(&plan.id, "task_exact")
            .unwrap()
            .unwrap()
            .key,
        "EXACT"
    );
    assert!(database
        .task_creation_projection("missing", "task_exact")
        .unwrap()
        .is_none());
    assert!(database
        .task_creation_projection(&plan.id, "missing")
        .unwrap()
        .is_none());
    assert!(matches!(
        database.task_description_projection("missing"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.section_ids',42) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .task_creation_projection(&plan.id, "task_exact")
        .is_err());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',42) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database.task_description_projection("task_exact").is_err());
}

#[test]
fn creation_and_description_bytes_ignore_task_body_and_history_growth() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        let original = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = original.clone();
            old.id = format!("closed-{index}");
            old.closed_at = Some(2);
            plan.tasks[0].execution_attempts.push(old);
        }
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let created = database
            .task_creation_projection(&plan.id, "task_0000")
            .unwrap()
            .unwrap();
        let description = database.task_description_projection("task_0000").unwrap();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(created, description)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 1024);
        assert!(decoded < 1024);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=task_creation_description tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
