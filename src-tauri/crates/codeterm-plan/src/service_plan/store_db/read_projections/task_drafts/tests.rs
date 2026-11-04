use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;
use crate::service_plan::task_attempts::{build_compete_prompt, build_prompt, build_prompt_read};

fn milestone(id: &str, kind: &str, tab: &str, ts: u64) -> Milestone {
    serde_json::from_value(serde_json::json!({"id":id,"kind":kind,"from":{"tab_id":tab},"ts":ts,"text":format!("STATUS: blocked\n{id}")})).unwrap()
}

#[test]
fn draft_projection_preserves_prompt_fields_latest_report_and_status_before_source_refusal() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.tasks[0].model = Some("provider/model-token".into());
    plan.tasks[0].personality = Some("identity-token".into());
    plan.tasks[0].role_guidance = Some("guidance-token".into());
    plan.tasks[0].competition = Some(TaskCompetitionMetadata {
        attempts: Some(3),
        models: vec!["model-token".into()],
        judge_hint: Some("hint-token".into()),
        ..Default::default()
    });
    plan.tasks[0].milestones = (0..8)
        .map(|index| milestone(&format!("decision-{index}"), "decision", "context", index))
        .collect();
    plan.tasks[0]
        .milestones
        .push(milestone("old-report", "status_update", "worker", u64::MAX));
    plan.tasks[0]
        .milestones
        .push(milestone("new-report", "status_update", "worker", u64::MAX));
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .task_draft_projection(&plan.id, "FIXTURE-0000", Some("current"))
        .unwrap();
    assert_eq!(read.task.milestones.len(), 7);
    let original = &plan.tasks[0];
    let source = read.source.as_ref().unwrap();
    assert_eq!(
        build_prompt_read(
            &plan.id,
            &read.task,
            source,
            "provider",
            Some("model"),
            "report",
            &[],
            &[]
        ),
        build_prompt(
            &plan.id,
            original,
            &original.execution_attempts[0],
            "provider",
            Some("model"),
            "report",
            &[],
            &[]
        )
    );
    let compete = database
        .task_draft_projection(&plan.id, "task_0000", None)
        .unwrap();
    assert_eq!(compete.task.milestones.len(), 6);
    assert_eq!(
        build_compete_prompt(&plan.id, &compete.task, "group", 1, "model", "report", &[]),
        build_compete_prompt(
            &plan.id,
            &TaskDraftPromptRead::from(original),
            "group",
            1,
            "model",
            "report",
            &[]
        )
    );
    assert!(matches!(
        database.task_draft_projection(&plan.id, "task_0000", Some("missing")),
        Err(PlanError::TaskNotFound)
    ));
    assert!(matches!(
        database.task_draft_projection("missing", "task_0000", None),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.task_draft_projection(&plan.id, "missing", None),
        Err(PlanError::TaskNotFound)
    ));
    plan.tasks[0].status = TaskStatus::Done;
    database.persist_plan_rows(&plan, None).unwrap();
    let expected = crate::service_plan::task_dispatch::task_respawn_status(&TaskStatus::Done)
        .unwrap_err()
        .to_string();
    assert_eq!(
        database
            .task_draft_projection(&plan.id, "task_0000", Some("missing"))
            .unwrap_err()
            .to_string(),
        expected
    );
    assert!(database
        .task_draft_projection(&plan.id, "task_0000", None)
        .is_ok());
    for field in [
        "$.body",
        "$.acceptance_criteria",
        "$.lane",
        "$.interruption.execution_intent",
    ] {
        database.persist_plan_rows(&plan, None).unwrap();
        database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,?1,json('null')) WHERE task_id='task_0000'",[field]).unwrap();
        assert!(
            database
                .task_draft_projection(&plan.id, "task_0000", None)
                .is_err(),
            "{field}"
        );
    }
    database.persist_plan_rows(&plan, None).unwrap();
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',42) WHERE task_id='task_0000'",[]).unwrap();
    assert!(database
        .task_draft_projection(&plan.id, "task_0000", None)
        .is_err());
}

fn fixture(consumer: &str, source: Option<&str>) {
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
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let original = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| TaskExecutionAttempt {
                id: format!("closed-{index}"),
                closed_at: Some(2),
                ..original.clone()
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        plan.tasks[0].milestones = (0..history)
            .map(|index| {
                milestone(
                    &format!("noise-{index}"),
                    "status_update",
                    "unrelated",
                    index as u64,
                )
            })
            .collect();
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = database
            .task_draft_projection(&plan.id, "task_0000", source)
            .unwrap();
        assert_eq!(read.task.body, plan.tasks[0].body);
        assert_eq!(read.source.is_some(), source.is_some());
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, if source.is_some() { 3 } else { 2 });
        assert!(metrics.materialized_bytes < 32768);
        assert!(decoded < 32768);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer={consumer} tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_unselected_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn build_task_compete_drafts_fixture_selects_prompt_facts_without_serialization_or_writes() {
    fixture("build_task_compete_drafts", None);
}

#[test]
fn build_task_respawn_draft_fixture_selects_source_and_prompt_facts_without_serialization_or_writes(
) {
    fixture("build_task_respawn_draft", Some("current"));
}
