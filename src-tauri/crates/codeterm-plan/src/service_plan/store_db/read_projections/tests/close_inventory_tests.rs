use super::*;

#[test]
fn close_restore_metadata_excludes_tasks_history_and_unmatched_plans() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.team_id = Some("closing".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        let mut unmatched = Plan::new(
            "Unmatched".into(),
            plan.project_path.clone(),
            Default::default(),
        );
        unmatched.id = "unmatched".into();
        unmatched.team_id = Some("staying".into());
        database.persist_plan_rows(&unmatched, None).unwrap();
        database
            .connection
            .lock()
            .execute(
                "UPDATE plans SET header_json=json_set(header_json,'$.owner',17) WHERE id=?1",
                [&unmatched.id],
            )
            .unwrap();
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let rows = database
            .close_restore_projection(&["closing".into()])
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, plan.id);
        assert_eq!(rows[0].owner, plan.owner);
        assert_eq!(rows[0].planner, plan.planner);
        assert_eq!(rows[0].status, plan.status);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&rows).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=close_restore tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} selected_extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
        database
            .connection
            .lock()
            .execute(
                "UPDATE plans SET header_json=json_set(header_json,'$.owner',17) WHERE id=?1",
                [&plan.id],
            )
            .unwrap();
        assert!(matches!(
            database.close_restore_projection(&["closing".into()]),
            Err(PlanError::Quarantined { .. })
        ));
    }
}
