use super::*;

#[test]
fn plan_metadata_preserves_exact_project_existence_owner_and_status() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.owner.tab_id = Some(" owner ".into());
    let database = fixtures::database(directory.path(), &plan);
    assert!(database.plan_exists_projection(&plan.id).unwrap());
    assert!(!database.plan_exists_projection("missing").unwrap());
    assert_eq!(
        database
            .plan_owner_tab_projection(&plan.id)
            .unwrap()
            .as_deref(),
        Some(" owner ")
    );
    assert!(database
        .plan_owner_tab_projection("missing")
        .unwrap()
        .is_none());
    assert_eq!(
        database.plan_status_projection(&plan.id).unwrap(),
        PlanStatus::Active
    );
    assert!(matches!(
        database.plan_status_projection("missing"),
        Err(PlanError::NotFound)
    ));
    let mut other = plan.clone();
    other.id = "other".into();
    other.status = PlanStatus::Archived;
    database.persist_plan_rows(&other, None).unwrap();
    let mut expected = vec![plan.id.clone(), other.id.clone()];
    expected.sort();
    assert_eq!(
        database
            .plan_project_ids_projection(&plan.project_path)
            .unwrap(),
        expected
    );
    assert!(database
        .plan_project_ids_projection(&format!("{} ", plan.project_path))
        .unwrap()
        .is_empty());
    assert_eq!(
        database.plan_status_projection(&other.id).unwrap(),
        PlanStatus::Archived
    );
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',42) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(database.plan_owner_tab_projection(&plan.id).is_err());
    assert!(database.plan_exists_projection(&plan.id).unwrap());
    database
        .connection
        .lock()
        .execute("UPDATE plans SET status='invalid' WHERE id=?1", [&plan.id])
        .unwrap();
    assert!(database.plan_status_projection(&plan.id).is_err());
}

#[test]
fn plan_metadata_fixture_never_materializes_task_or_history_rows() {
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
        let exists = database.plan_exists_projection(&plan.id).unwrap();
        let owner = database.plan_owner_tab_projection(&plan.id).unwrap();
        let status = database.plan_status_projection(&plan.id).unwrap();
        let ids = database
            .plan_project_ids_projection(&plan.project_path)
            .unwrap();
        assert!(exists);
        assert_eq!(owner.as_deref(), Some("owner"));
        assert_eq!(ids, vec![plan.id]);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(exists, owner, status, ids))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 4);
        assert!(metrics.materialized_bytes < 256);
        assert!(decoded < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=plan_admission_metadata tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
