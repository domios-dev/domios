use super::*;

type Superseded = (String, String, String, String, String);

fn superseded_reference(plan: &Plan, tab: &str) -> Option<Superseded> {
    plan.tasks.iter().find_map(|task| {
        let (index, owned) = task
            .execution_attempts
            .iter()
            .enumerate()
            .rev()
            .find(|(_, attempt)| attempt.executor_tab_id == tab)?;
        owned.closed_at?;
        let live = task
            .execution_attempts
            .iter()
            .skip(index + 1)
            .rev()
            .find(|attempt| attempt.closed_at.is_none() && attempt.retired_at.is_none())?;
        Some((
            plan.id.clone(),
            task.id.clone(),
            owned.id.clone(),
            live.id.clone(),
            live.executor_tab_id.clone(),
        ))
    })
}

#[test]
fn lifecycle_subjects_preserve_position_boundaries_and_retired_runtime_candidates() {
    for variant in 0..12 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        plan.tasks[0].status = TaskStatus::Pending;
        let mut source = plan.tasks[0].execution_attempts[0].clone();
        source.closed_at = Some(2);
        let mut replacement = source.clone();
        replacement.id = "replacement".into();
        replacement.executor_tab_id = "replacement-worker".into();
        replacement.closed_at = None;
        plan.tasks[0].execution_attempts = vec![source.clone(), replacement.clone()];
        match variant {
            1 => plan.status = PlanStatus::Archived,
            2 => plan.tasks[0].status = TaskStatus::Done,
            3 => plan.tasks[0].execution_attempts[1].retired_at = Some(3),
            4 => plan.tasks[0].execution_attempts[1].closed_at = Some(3),
            5 => plan.tasks[0].execution_attempts[0].closed_at = None,
            6 => plan.tasks[0].execution_attempts.push(source.clone()),
            7 => {
                source.id = replacement.id.clone();
                plan.tasks[0].execution_attempts[0] = source;
            }
            8 => {
                replacement.retired_at = Some(4);
                plan.tasks[0].execution_attempts.push(replacement);
            }
            9 => {
                replacement.id = "last".into();
                plan.tasks[0].execution_attempts.push(replacement);
            }
            10 => plan.tasks[0].status = TaskStatus::Paused,
            11 => plan.tasks[0].assigned_tab_id = None,
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        assert_eq!(
            database.superseded_attempt_projection("worker").unwrap(),
            superseded_reference(&plan, "worker"),
            "variant {variant}"
        );
        for tab in ["worker", "replacement-worker", "missing"] {
            let expected = plan
                .tasks
                .iter()
                .filter(|task| {
                    plan.status == PlanStatus::Active
                        && matches!(task.status, TaskStatus::Pending | TaskStatus::InProgress)
                })
                .filter_map(|task| {
                    task.execution_attempts
                        .iter()
                        .rfind(|attempt| {
                            attempt.executor_tab_id == tab && attempt.closed_at.is_none()
                        })
                        .map(|attempt| (plan.id.clone(), task.id.clone(), attempt.id.clone()))
                })
                .collect::<Vec<_>>();
            let (count, selected) = database.runtime_ready_subject_projection(tab).unwrap();
            assert_eq!(count, expected.len() as u64, "variant {variant} tab {tab}");
            assert_eq!(
                selected,
                if expected.len() == 1 {
                    expected.first().cloned()
                } else {
                    None
                }
            );
        }
    }
}

#[test]
fn runtime_ready_counts_ambiguity_without_returning_candidate_rows() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].status = TaskStatus::InProgress;
    let mut second = plan.tasks[0].clone();
    second.id = "second".into();
    second.handle = None;
    second.key = None;
    plan.tasks.push(second);
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database.runtime_ready_subject_projection("worker").unwrap(),
        (2, None)
    );
    plan.tasks.pop();
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database.runtime_ready_subject_projection("worker").unwrap(),
        (
            1,
            Some((plan.id.clone(), "task_exact".into(), "current".into()))
        )
    );
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.id',42)",
            [],
        )
        .unwrap();
    assert!(database.runtime_ready_subject_projection("worker").is_err());
}

#[test]
fn lifecycle_subject_fixture_is_bounded_across_unrequested_history_and_body() {
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
        let mut source = original.clone();
        source.id = "previous".into();
        source.executor_tab_id = "previous-worker".into();
        source.closed_at = Some(2);
        let old = (0..history)
            .map(|index| {
                let mut attempt = source.clone();
                attempt.id = format!("old-{index}");
                attempt.executor_tab_id = "unselected-worker".into();
                attempt
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        let last = plan.tasks[0].execution_attempts.len() - 1;
        plan.tasks[0].execution_attempts.insert(last, source);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let superseded = database
            .superseded_attempt_projection("previous-worker")
            .unwrap()
            .unwrap();
        assert_eq!(superseded.3, original.id);
        let ready = database
            .runtime_ready_subject_projection("missing-worker")
            .unwrap();
        assert_eq!(ready, (0, None));
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(superseded, ready)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 512);
        assert!(decoded < 512);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=lifecycle_subject tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
