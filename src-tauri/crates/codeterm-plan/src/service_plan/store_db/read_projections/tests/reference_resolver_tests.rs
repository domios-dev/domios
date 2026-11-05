use super::*;
use crate::service_plan::resolve::ResolveError;

#[test]
fn reference_ranking_preserves_exact_alias_prefix_and_global_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    plan.id = "a".into();
    plan.tasks.truncate(3);
    plan.tasks[0].id = "task_exact".into();
    plan.tasks[0].handle = Some("Mixed-Handle".into());
    plan.tasks[0].key = Some("KEY-1".into());
    plan.tasks[1].id = "task_other".into();
    plan.tasks[1].handle = Some("task_exact".into());
    plan.tasks[1].key = Some("KEY-2".into());
    plan.tasks[2].id = "task_third".into();
    plan.tasks[2].handle = Some("Mixed-Other".into());
    plan.tasks[2].key = Some("KEY-3".into());
    let database = fixtures::database(directory.path(), &plan);
    for reference in ["task_exact", "mixed-handle", "key-1", "Mixed-H"] {
        let rows = database
            .resolve_task_reference(Some("a"), reference)
            .unwrap();
        assert_eq!(rows.len(), 1, "{reference}");
        assert_eq!(rows[0].task_id, "task_exact");
        assert!(rows[0].candidate.is_none());
    }
    assert!(database
        .resolve_task_reference(Some("a"), "mixed-H")
        .unwrap()
        .is_empty());
    let ambiguous = database
        .resolve_task_reference(Some("a"), "Mixed-")
        .unwrap();
    assert_eq!(
        ambiguous
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        ["task_exact", "task_third"]
    );
    assert_eq!(
        ambiguous[0].candidate.as_ref().unwrap().handle.as_deref(),
        Some("Mixed-Handle")
    );
    let mut newer = plan.clone();
    newer.id = "z".into();
    newer.updated_at = 20;
    database.persist_plan(&newer, None).unwrap();
    let rows = database.resolve_task_reference(None, "task_exact").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].plan_id, "a");
    let store = PlanStore::open(database.path()).unwrap();
    assert!(
        matches!(store.resolve_task_ref(None,"Mixed-"), Err(ResolveError::Ambiguous {candidates,..}) if candidates.len()==4)
    );
}

#[test]
fn invalid_selected_reference_facts_are_errors_and_exact_ids_skip_unused_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let plan = fixtures::fixture(directory.path(), 0, 1);
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.handle',17) WHERE plan_id=?1 AND task_id='task_0000'",[&plan.id]).unwrap();
    assert_eq!(
        store.resolve_task_ref(None, "task_0000").unwrap().task_id,
        "task_0000"
    );
    let error = store.resolve_task_ref(None, "fixture-0000").unwrap_err();
    assert!(matches!(error, ResolveError::Read(PlanError::Io(_))));
    assert_ne!(error.to_json()["type"], "ref_not_found");
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_tasks")
        .unwrap();
    assert!(matches!(
        store.resolve_task_ref(None, "missing"),
        Err(ResolveError::Read(PlanError::Io(_)))
    ));
}

#[test]
fn reference_identity_materialization_excludes_selected_labels_bodies_and_history() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].name.push_str(&"n".repeat(body));
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[0].milestones = plan.strides.clone();
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let mut response = Vec::new();
        for reference in ["task_0000", "FIXTURE-0000", "fixture-0000"] {
            let resolved = store.resolve_task_ref(None, reference).unwrap();
            response.push((resolved.plan_id, resolved.task_id));
        }
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 3);
        assert_eq!(
            metrics.materialized_bytes,
            3 * (plan.id.len() + "task_0000".len())
        );
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(store.plan_loads_for_test().is_empty());
        let response_bytes = serde_json::to_vec(&response).unwrap().len();
        println!("fixture={} consumer=reference_identity tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} selected_extra_body_and_name={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn section_loop_element_and_milestone_storage_errors_are_not_missing_references() {
    let directory = tempfile::tempdir().unwrap();
    let plan = fixtures::fixture(directory.path(), 0, 1);
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.loops',json('[{\"id\":\"loop\",\"target\":17}]')) WHERE id=?1",[&plan.id]).unwrap();
    assert!(matches!(
        store.resolve_loop_id("loop"),
        Err(ResolveError::Read(PlanError::Io(_)))
    ));
    for table in ["plan_sections", "plan_elements", "plan_task_milestones"] {
        database
            .connection
            .lock()
            .execute_batch(&format!("DROP TABLE {table}"))
            .unwrap();
    }
    assert!(matches!(
        store.resolve_section_ref(None, "section"),
        Err(ResolveError::Read(PlanError::Io(_)))
    ));
    assert!(matches!(
        store.resolve_section_ref(Some(&plan.id), "section"),
        Err(ResolveError::Read(PlanError::Io(_)))
    ));
    assert!(matches!(
        store.resolve_element_ref(&plan.id, "element"),
        Err(ResolveError::Read(PlanError::Io(_)))
    ));
    assert!(matches!(
        store.resolve_milestone_ref(None, "task_0000", "milestone"),
        Err(ResolveError::Read(PlanError::Io(_)))
    ));
}
