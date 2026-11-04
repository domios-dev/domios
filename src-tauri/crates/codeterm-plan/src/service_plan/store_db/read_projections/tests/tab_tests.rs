use super::*;
use crate::plan::TaskStatus;

#[test]
fn tab_readers_select_constant_bytes_without_writes() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let held = database
            .report_tab_holds_task_projection(&plan.id, &plan.tasks[0].id, "worker")
            .unwrap();
        let name = database
            .linked_tab_task_name_projection(&plan.id, &plan.tasks[0].id)
            .unwrap();
        let binding = database
            .assigned_task_binding_projection("worker", false)
            .unwrap()
            .unwrap();
        let owner = database.plan_identity_projection(&binding.plan_id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert!(held);
        assert_eq!(name, plan.tasks[0].name);
        assert_eq!(binding.task_id, plan.tasks[0].id);
        assert_eq!(owner.owner, plan.owner);
        assert_eq!(metrics.queries, 4);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(held, name, binding, owner))
            .unwrap()
            .len();
        println!("fixture={} consumers=tab_adopt_live_task_plan,tab_repair_live_task_plan,resolve_draft,require_checklist_reviewer,record_checklist_event tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, metrics.queries, metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn linked_tab_task_name_preserves_exact_plan_and_task_identity_and_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Tab".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let mut task = crate::service_plan::load_audit::task("task_exact", TaskStatus::Done, None);
    task.handle = Some("named".into());
    task.name = "Exact title".into();
    plan.tasks.push(task);
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database
            .linked_tab_task_name_projection(&plan.id, "task_exact")
            .unwrap(),
        "Exact title"
    );
    assert!(matches!(
        database.linked_tab_task_name_projection(&plan.id, "named"),
        Err(PlanError::TaskNotFound)
    ));
    assert!(matches!(
        database.linked_tab_task_name_projection("missing", "task_exact"),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_tasks")
        .unwrap();
    assert!(database
        .linked_tab_task_name_projection(&plan.id, "task_exact")
        .is_err());
}
