use super::*;
use crate::service_plan::task_brief::{render_task_brief, render_tool_less_task_brief};
use crate::service_plan::task_queries::TaskSection;

#[test]
fn sql_preparation_renders_the_same_brief_bytes_as_held_task_in_tree_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let task = &mut plan.tasks[0];
    task.name = "opaque title".into();
    task.parent_task_id = Some("parent".into());
    task.body = "\n  opaque body\n".into();
    task.personality = Some("Persona".into());
    task.role_guidance = Some("opaque guidance".into());
    task.executor_role = Some("executor".into());
    task.intent = Some(" intent ".into());
    task.correctness = Some(" correctness ".into());
    task.test_plan = Some(" tests ".into());
    task.acceptance_criteria = vec!["criterion one".into(), "criterion two".into()];
    task.speed = Some("fast".into());
    task.lane.base = Some("base".into());
    task.section_ids = vec![
        "second".into(),
        "nested".into(),
        "first".into(),
        "first".into(),
    ];
    task.milestones.push(serde_json::from_value(serde_json::json!({"id":"stride-one","from":{},"kind":"decision","text":"first stride","ts":1})).unwrap());
    task.milestones.push(serde_json::from_value(serde_json::json!({"id":"stride-two","from":{},"kind":"progress","text":"second stride","ts":2})).unwrap());
    plan.sections=serde_json::from_value(serde_json::json!([
        {"id":"first","title":"First","body":"unrequested section body","subsections":[{"id":"nested","title":"Nested"}]},
        {"id":"second","title":"Second"}
    ])).unwrap();
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .worker_task_preparation_projection(&plan.id, "NAMED")
        .unwrap();
    assert_eq!(read.personality, plan.tasks[0].personality);
    assert_eq!(read.speed, plan.tasks[0].speed);
    assert_eq!(read.lane_base, plan.tasks[0].lane.base);
    assert_eq!(
        read.brief
            .sections
            .iter()
            .map(|section| section.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "nested", "second"]
    );
    let sections = vec![
        TaskSection {
            id: "first".into(),
            title: "First".into(),
        },
        TaskSection {
            id: "nested".into(),
            title: "Nested".into(),
        },
        TaskSection {
            id: "second".into(),
            title: "Second".into(),
        },
    ];
    for instruction in [None, Some(" instruction ")] {
        for persona in [None, read.personality.as_deref()] {
            let gates = vec!["opaque-gate".into()];
            assert_eq!(
                render_task_brief(
                    &read.brief,
                    &read.brief.sections,
                    persona,
                    instruction,
                    Some(&plan.id),
                    &[],
                    &gates
                )
                .as_bytes(),
                render_task_brief(
                    &plan.tasks[0],
                    &sections,
                    persona,
                    instruction,
                    Some(&plan.id),
                    &[],
                    &gates
                )
                .as_bytes()
            );
            assert_eq!(
                render_tool_less_task_brief(
                    &read.brief,
                    &read.brief.sections,
                    persona,
                    instruction
                )
                .as_bytes(),
                render_tool_less_task_brief(&plan.tasks[0], &sections, persona, instruction)
                    .as_bytes()
            );
        }
    }
}

#[test]
fn spawn_binding_keeps_first_attempt_identity_session_nonce_and_unsigned_clocks() {
    for variant in 0..10 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        match variant {
            1 => plan.tasks[0].execution_attempts[0].executor_tab_id = "foreign".into(),
            2 => plan.tasks[0].execution_attempts[0].closed_at = Some(u64::MAX),
            3 => plan.tasks[0].execution_attempts[0].retired_at = Some(u64::MAX),
            4 => plan.tasks[0].execution_attempts[0].session_id = None,
            5 => plan.tasks[0].execution_attempts[0].session_binding_nonce = None,
            6 => plan.tasks[0].execution_attempts[0].session_binding_nonce = Some("".into()),
            7 => {
                let mut later = plan.tasks[0].execution_attempts[0].clone();
                later.executor_tab_id = "later".into();
                plan.tasks[0].execution_attempts.push(later);
            }
            8 => plan.tasks[0].execution_attempts.clear(),
            9 => plan.status = PlanStatus::Archived,
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        if variant == 9 {
            assert!(matches!(
                database.task_spawn_binding_projection(&plan.id, "NAMED", "current"),
                Err(PlanError::TaskNotFound)
            ));
            assert!(!database
                .task_attempt_exists_projection(&plan.id, "NAMED", "current")
                .unwrap());
            continue;
        }
        let read = database
            .task_spawn_binding_projection(&plan.id, "NAMED", "current")
            .unwrap();
        assert_eq!(read.lane, plan.tasks[0].lane);
        let source = plan.tasks[0]
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == "current");
        assert_eq!(
            database
                .task_attempt_exists_projection(&plan.id, "NAMED", "current")
                .unwrap(),
            source.is_some()
        );
        assert_eq!(read.attempt.is_some(), source.is_some());
        if let (Some(actual), Some(source)) = (read.attempt, source) {
            assert_eq!(actual.id, source.id);
            assert_eq!(actual.executor_tab_id, source.executor_tab_id);
            assert_eq!(actual.session_id, source.session_id);
            assert_eq!(actual.session_binding_nonce, source.session_binding_nonce);
            assert_eq!(actual.closed_at, source.closed_at);
            assert_eq!(actual.retired_at, source.retired_at);
        }
    }
}

#[test]
fn preparation_and_binding_selected_corruption_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    let plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_binding_nonce',42)",[]).unwrap();
    assert!(database
        .task_spawn_binding_projection(&plan.id, "NAMED", "current")
        .is_err());
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_set(row_json,'$.section_ids',json('[42]'))",
            [],
        )
        .unwrap();
    assert!(database
        .worker_task_preparation_projection(&plan.id, "NAMED")
        .is_err());
}

#[test]
fn spawn_preparation_fixture_ignores_unrequested_bodies_and_attempt_history() {
    let mut prior_binding = None;
    let mut prior_prompt = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        let current = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| {
                let mut old = current.clone();
                old.id = format!("old-{index}");
                old.closed_at = Some(2);
                old
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        plan.tasks[1].body.push_str(&"x".repeat(body));
        if body > 0 {
            plan.tasks[0].execution_attempts.last_mut().unwrap().report =
                Some(crate::plan::TaskExecutionReportSnapshot {
                    message: Some("x".repeat(body)),
                    ..Default::default()
                });
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        for (consumer, queries, prior) in [
            ("spawn_binding", 2, &mut prior_binding),
            ("worker_prompt", 3, &mut prior_prompt),
        ] {
            take_metrics();
            crate::service_plan::store_sync::take_decoded_row_bytes();
            crate::service_plan::store_rows::take_serialized_row_bytes();
            let response = if consumer == "spawn_binding" {
                let read = database
                    .task_spawn_binding_projection(&plan.id, "task_0000", &current.id)
                    .unwrap();
                assert_eq!(read.attempt.as_ref().unwrap().id, current.id);
                let exists = database
                    .task_attempt_exists_projection(&plan.id, "task_0000", &current.id)
                    .unwrap();
                assert!(exists);
                serde_json::to_vec(&(read, exists)).unwrap().len()
            } else {
                let read = database
                    .worker_task_preparation_projection(&plan.id, "task_0000")
                    .unwrap();
                serde_json::to_vec(&read).unwrap().len()
            };
            let metrics = take_metrics();
            let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
            let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
            assert_eq!(metrics.queries, queries);
            assert!(metrics.materialized_bytes < response + 32768);
            assert!(decoded < response + 32768);
            assert_eq!(serialized, 0);
            assert!(fixtures::audit_counts(&database).is_empty());
            println!("fixture={} consumer={consumer} tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_unrequested_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
            let measured = (metrics, decoded, response);
            if let Some(prior) = prior.as_ref() {
                assert_eq!(&measured, prior);
            }
            *prior = Some(measured);
        }
    }
}
