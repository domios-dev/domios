use super::*;
use crate::service_plan::TaskSplitDefaultsRead;

#[test]
fn split_defaults_preserve_inherited_fields_and_materialize_body_only_on_request() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let task = &mut plan.tasks[0];
    task.body = "inherited body".into();
    task.acceptance_criteria = vec!["criterion".into()];
    task.section_ids = vec!["second".into(), "first".into()];
    task.test_plan = Some("test".into());
    task.qa_plan = Some("qa".into());
    task.correctness = Some("correct".into());
    task.executor_role = Some("Worker".into());
    task.personality = Some("Nova".into());
    task.role_guidance = Some("role".into());
    task.model = Some("model".into());
    task.model_rationale = Some("why".into());
    task.intent = Some("intent".into());
    let expected = TaskSplitDefaultsRead {
        parent_task_id: task.parent_task_id.clone(),
        status: task.status.clone(),
        kind: task.kind.clone(),
        body: Some(task.body.clone()),
        acceptance_criteria: task.acceptance_criteria.clone(),
        section_ids: task.section_ids.clone(),
        test_plan: task.test_plan.clone(),
        qa_plan: task.qa_plan.clone(),
        correctness: task.correctness.clone(),
        executor_role: task.executor_role.clone(),
        personality: task.personality.clone(),
        role_guidance: task.role_guidance.clone(),
        model: task.model.clone(),
        model_rationale: task.model_rationale.clone(),
        intent: task.intent.clone(),
    };
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database
            .task_split_defaults_projection("NAMED", true)
            .unwrap(),
        expected
    );
    assert_eq!(
        database
            .task_split_defaults_projection("task_exact", false)
            .unwrap()
            .body,
        None
    );
    for status in [TaskStatus::Done, TaskStatus::Cancelled] {
        plan.tasks[0].status = status;
        database.persist_plan_rows(&plan, None).unwrap();
        assert_eq!(
            database
                .task_split_defaults_projection("task_exact", true)
                .unwrap()
                .body,
            None
        );
    }
    plan.tasks[0].status = TaskStatus::Pending;
    plan.tasks[0].parent_task_id = Some("parent".into());
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .task_split_defaults_projection("task_exact", true)
            .unwrap()
            .body,
        None
    );
    assert!(matches!(
        database.task_split_defaults_projection("missing", true),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.acceptance_criteria',42) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .task_split_defaults_projection("task_exact", false)
        .is_err());
}

#[test]
fn split_defaults_bytes_ignore_unrequested_body_and_history_growth() {
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
        let read = database
            .task_split_defaults_projection("task_0000", false)
            .unwrap();
        assert!(read.body.is_none());
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=task_split_defaults tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
