use super::*;
use crate::service_plan::{BatchTaskRefusalRead, TaskSelectorRead};

fn resolve<'a>(plan: &'a Plan, selector: &str) -> Option<&'a crate::plan::Task> {
    if selector == "@latest" {
        plan.tasks
            .iter()
            .max_by_key(|task| (task.updated_at, task.created_at))
    } else {
        crate::plan::task_key::find_task(&plan.tasks, selector)
    }
}

fn refusal(
    plan: &Plan,
    selectors: &[String],
    caller: Option<&str>,
) -> Option<BatchTaskRefusalRead> {
    let owner = plan
        .owner
        .tab_id
        .as_deref()
        .map(str::trim)
        .filter(|tab| !tab.is_empty());
    if caller.is_some() && caller == owner {
        return None;
    }
    for selector in selectors {
        let Some(task) = resolve(plan, selector) else {
            continue;
        };
        let competition = |attempt: &&crate::plan::TaskExecutionAttempt| {
            attempt.closed_at.is_none()
                && attempt.retired_at.is_none()
                && attempt.competition_group_id.is_some()
        };
        let held = task
            .assigned_tab_id
            .as_deref()
            .is_some_and(|tab| !tab.trim().is_empty())
            || task
                .execution_attempts
                .iter()
                .any(|attempt| competition(&attempt));
        let owns = caller.is_some_and(|tab| {
            task.assigned_tab_id.as_deref() == Some(tab)
                || task
                    .execution_attempts
                    .iter()
                    .any(|attempt| competition(&attempt) && attempt.executor_tab_id == tab)
        });
        if held && !owns {
            return Some(BatchTaskRefusalRead {
                task_id: task.id.clone(),
                name: task.name.clone(),
                assigned_tab_id: task.assigned_tab_id.clone(),
            });
        }
    }
    None
}

#[test]
fn batch_projection_preserves_refusal_alias_competition_and_operation_order() {
    for variant in 0..14 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        plan.owner.tab_id = Some("\u{2003}owner\u{2009}".into());
        let mut other = plan.tasks[0].clone();
        other.id = "task_other".into();
        other.handle = Some("other".into());
        other.key = None;
        other.assigned_tab_id = Some("other-worker".into());
        plan.tasks.push(other);
        match variant {
            1 => plan.tasks[0].assigned_tab_id = None,
            2 => plan.tasks[0].assigned_tab_id = Some("\u{2003}".into()),
            3 => plan.tasks[0].assigned_tab_id = Some(" worker ".into()),
            4..=8 => {
                plan.tasks[0].assigned_tab_id = None;
                let attempt = &mut plan.tasks[0].execution_attempts[0];
                attempt.competition_group_id = Some("competition".into());
                match variant {
                    5 => attempt.closed_at = Some(1),
                    6 => attempt.retired_at = Some(1),
                    7 => attempt.executor_tab_id = "other-worker".into(),
                    8 => attempt.competition_group_id = None,
                    _ => {}
                }
            }
            9 => plan.tasks[1].handle = Some("NaMeD".into()),
            10 => plan.tasks[1].key = Some("COD-1".into()),
            11 => plan.tasks[1].handle = Some("task_exact".into()),
            12 => {
                plan.tasks[0].updated_at = u64::MAX;
                plan.tasks[1].updated_at = u64::MAX;
                plan.tasks[1].created_at = u64::MAX;
            }
            13 => {
                plan.tasks[0].updated_at = u64::MAX;
                plan.tasks[1].updated_at = u64::MAX;
                plan.tasks[0].created_at = u64::MAX;
                plan.tasks[1].created_at = u64::MAX;
            }
            _ => {}
        }
        let selectors = [
            "missing",
            "named",
            "cod-1",
            "task_exact",
            "OTHER",
            "@latest",
        ]
        .map(str::to_string)
        .to_vec();
        let database = fixtures::database(directory.path(), &plan);
        let expected = selectors
            .iter()
            .enumerate()
            .filter_map(|(index, selector)| {
                resolve(&plan, selector).map(|task| TaskSelectorRead {
                    index,
                    task_id: task.id.clone(),
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            database
                .batch_task_resolution_projection(&plan.id, &selectors)
                .unwrap(),
            expected,
            "variant {variant}"
        );
        for caller in [
            None,
            Some("worker"),
            Some("wrong"),
            Some("owner"),
            Some("other-worker"),
        ] {
            assert_eq!(
                database
                    .batch_task_refusal_projection(&plan.id, &selectors, caller)
                    .unwrap(),
                refusal(&plan, &selectors, caller),
                "variant {variant} caller {caller:?}"
            );
        }
        assert!(database
            .batch_task_refusal_projection("missing", &selectors, None)
            .unwrap()
            .is_none());
        assert!(matches!(
            database.batch_task_resolution_projection("missing", &selectors),
            Err(PlanError::NotFound)
        ));
    }
}

#[test]
fn batch_projection_reads_committed_assignment_and_propagates_selected_corruption() {
    let directory = tempfile::tempdir().unwrap();
    let plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let selectors = vec!["task_exact".into()];
    assert!(database
        .batch_task_refusal_projection(&plan.id, &selectors, Some("worker"))
        .unwrap()
        .is_none());
    let writer = rusqlite::Connection::open(directory.path().join("plans.db")).unwrap();
    writer.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.assigned_tab_id','new-worker') WHERE task_id='task_exact'",[]).unwrap();
    assert_eq!(
        database
            .batch_task_refusal_projection(&plan.id, &selectors, Some("worker"))
            .unwrap()
            .unwrap()
            .assigned_tab_id
            .as_deref(),
        Some("new-worker")
    );
    writer.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',42) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .batch_task_refusal_projection(&plan.id, &selectors, Some("worker"))
        .is_err());
    writer.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.updated_at',-1) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .batch_task_resolution_projection(&plan.id, &["@latest".into()])
        .is_err());
}

#[test]
fn batch_projection_fixture_returns_fixed_selected_bytes_and_writes_nothing() {
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
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].updated_at = 10;
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
        let selectors = vec!["@latest".into(), "task_0000".into()];
        let refusal = database
            .batch_task_refusal_projection(&plan.id, &selectors, Some("wrong"))
            .unwrap()
            .unwrap();
        let resolved = database
            .batch_task_resolution_projection(&plan.id, &selectors)
            .unwrap();
        assert_eq!(refusal.task_id, "task_0000");
        assert_eq!(resolved.len(), 2);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(refusal, resolved)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=batch_authority tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
