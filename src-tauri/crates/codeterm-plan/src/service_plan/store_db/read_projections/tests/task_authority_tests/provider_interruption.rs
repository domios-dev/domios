use super::*;
use crate::plan::{ExecutionIntent, Task, TaskExecutionAttempt};
use crate::service_plan::{ProviderRecoveryRead, TaskInterruptionRead};

fn active(task: &Task, attempt: &TaskExecutionAttempt) -> bool {
    attempt.reported_at.is_none()
        || (task.status == TaskStatus::InProgress
            && attempt.continuation.epoch > 0
            && attempt.continuation.state.as_deref() == Some("continued")
            && attempt.continuation.delivered_at.is_some())
}

fn interruption_reference(plan: &Plan, tab: &str) -> Option<TaskInterruptionRead> {
    plan.tasks.iter().find_map(|task| {
        let owner = &task.interruption;
        if owner.execution_intent != ExecutionIntent::Mutating
            || owner.owner_tab_id.as_deref() != Some(tab)
            || owner.handoff_required
        {
            return None;
        }
        let attempt = task.execution_attempts.iter().find(|attempt| {
            attempt.executor_tab_id == tab
                && attempt.closed_at.is_none()
                && active(task, attempt)
                && attempt.interruption.owner_epoch == owner.owner_epoch
        })?;
        Some(TaskInterruptionRead {
            plan_id: plan.id.clone(),
            task_id: task.id.clone(),
            attempt_id: attempt.id.clone(),
            owner_session_id: owner.owner_session_id.clone()?,
            owner_epoch: owner.owner_epoch,
            repo_path: if plan.canonical_project_path.trim().is_empty() {
                plan.project_path.clone()
            } else {
                plan.canonical_project_path.clone()
            },
            worktree_path: task.lane.worktree_path.clone()?,
            base_head: task.lane.base.clone(),
            test_plan: task.test_plan.clone(),
        })
    })
}

fn recovery_reference(plan: &Plan) -> Vec<ProviderRecoveryRead> {
    plan.tasks
        .iter()
        .filter_map(|task| {
            if task.status_label.as_deref()
                != Some(crate::service_plan::interruption::PROVIDER_EXHAUSTED_LABEL)
            {
                return None;
            }
            let owner = &task.interruption;
            let first = task.execution_attempts.iter().find(|attempt| {
                attempt.closed_at.is_none()
                    && Some(attempt.executor_tab_id.as_str()) == owner.owner_tab_id.as_deref()
            })?;
            if first.provider.as_deref() != Some("provider") {
                return None;
            }
            Some(ProviderRecoveryRead {
                plan_id: plan.id.clone(),
                task_id: task.id.clone(),
                owner_tab_id: owner.owner_tab_id.clone()?,
                owner_session_id: owner.owner_session_id.clone()?,
                owner_epoch: owner.owner_epoch,
            })
        })
        .collect()
}

fn configure(task: &mut Task) {
    task.status = TaskStatus::InProgress;
    task.status_label = Some(crate::service_plan::interruption::PROVIDER_EXHAUSTED_LABEL.into());
    task.interruption.execution_intent = ExecutionIntent::Mutating;
    task.interruption.owner_tab_id = Some("worker".into());
    task.interruption.owner_session_id = Some("session".into());
    task.interruption.owner_epoch = 7;
    task.lane.worktree_path = Some("lane".into());
    let attempt = &mut task.execution_attempts[0];
    attempt.provider = Some("provider".into());
    attempt.interruption.owner_epoch = 7;
}

#[test]
fn provider_interruption_matches_owner_continuation_and_first_attempt_rules() {
    for variant in 0..27 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        configure(&mut plan.tasks[0]);
        let task = &mut plan.tasks[0];
        match variant {
            1 => task.execution_attempts[0].retired_at = Some(1),
            2 => task.execution_attempts[0].closed_at = Some(1),
            3 => task.execution_attempts[0].reported_at = Some(1),
            4..=8 => {
                let attempt = &mut task.execution_attempts[0];
                attempt.reported_at = Some(1);
                attempt.continuation.epoch = 2;
                attempt.continuation.state = Some("continued".into());
                attempt.continuation.delivered_at = Some(3);
                match variant {
                    5 => attempt.continuation.epoch = 0,
                    6 => attempt.continuation.state = Some("pending".into()),
                    7 => attempt.continuation.delivered_at = None,
                    8 => task.status = TaskStatus::Paused,
                    _ => {}
                }
            }
            9 => task.interruption.handoff_required = true,
            10 => task.interruption.execution_intent = ExecutionIntent::ReadOnly,
            11 => task.interruption.owner_session_id = None,
            12 => task.interruption.owner_tab_id = Some("other".into()),
            13 => task.execution_attempts[0].interruption.owner_epoch += 1,
            14 => task.lane.worktree_path = None,
            15 => task.lane.worktree_path = Some(String::new()),
            16 => {
                task.test_plan = Some("gate".into());
                task.lane.base = Some("base".into());
            }
            17 => task.status_label = None,
            18 => task.execution_attempts[0].provider = Some("other".into()),
            19..=21 => {
                let mut first = task.execution_attempts[0].clone();
                first.id = "first".into();
                first.provider = Some("other".into());
                if variant == 20 {
                    first.closed_at = Some(1);
                }
                if variant == 21 {
                    first.executor_tab_id = "other".into();
                }
                task.execution_attempts.insert(0, first);
            }
            22 => plan.status = PlanStatus::Archived,
            23 => plan.canonical_project_path = "\u{2003}\t".into(),
            24 => plan.canonical_project_path = " canonical ".into(),
            25 => {
                task.interruption.owner_epoch = u64::MAX;
                task.execution_attempts[0].interruption.owner_epoch = u64::MAX;
            }
            26 => {
                task.interruption.owner_epoch = u64::MAX;
                task.execution_attempts[0].interruption.owner_epoch = u64::MAX - 1;
            }
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        assert_eq!(
            database.task_interruption_projection("worker").unwrap(),
            interruption_reference(&plan, "worker"),
            "variant {variant}"
        );
        assert_eq!(
            database.provider_recovery_projection("provider").unwrap(),
            recovery_reference(&plan),
            "variant {variant}"
        );
        let mut expected = plan
            .tasks
            .iter()
            .flat_map(|task| {
                task.execution_attempts.iter().filter_map(move |attempt| {
                    (attempt.closed_at.is_none()
                        && attempt.provider.as_deref() == Some("provider")
                        && active(task, attempt))
                    .then_some(attempt.executor_tab_id.clone())
                })
            })
            .collect::<Vec<_>>();
        expected.sort();
        expected.dedup();
        assert_eq!(
            database
                .provider_exhaustion_tabs_projection("provider")
                .unwrap(),
            expected,
            "variant {variant}"
        );
    }
}

#[test]
fn provider_interruption_order_and_committed_epoch_changes_are_observed() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = authority_plan(directory.path());
    first.id = "a".into();
    first.updated_at = 10;
    configure(&mut first.tasks[0]);
    let mut second = first.clone();
    second.id = "b".into();
    let database = fixtures::database(directory.path(), &first);
    database.persist_plan_rows(&second, None).unwrap();
    assert_eq!(
        database
            .task_interruption_projection("worker")
            .unwrap()
            .unwrap()
            .plan_id,
        "a"
    );
    second.updated_at = 11;
    database.persist_plan_rows(&second, None).unwrap();
    assert_eq!(
        database
            .task_interruption_projection("worker")
            .unwrap()
            .unwrap()
            .plan_id,
        "b"
    );
    let targets = database.provider_recovery_projection("provider").unwrap();
    assert_eq!(
        targets
            .iter()
            .map(|row| row.plan_id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a"]
    );
    let writer = rusqlite::Connection::open(directory.path().join("plans.db")).unwrap();
    writer.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.owner_epoch',8) WHERE plan_id='b'",[]).unwrap();
    assert_eq!(
        database
            .task_interruption_projection("worker")
            .unwrap()
            .unwrap()
            .plan_id,
        "a"
    );
    assert!(database
        .task_interruption_projection("missing")
        .unwrap()
        .is_none());
    assert!(database
        .provider_recovery_projection("missing")
        .unwrap()
        .is_empty());
    assert!(database
        .provider_exhaustion_tabs_projection("missing")
        .unwrap()
        .is_empty());
}

#[test]
fn provider_interruption_growth_returns_only_selected_facts_without_writes() {
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
        configure(&mut plan.tasks[0]);
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
        let tabs = database
            .provider_exhaustion_tabs_projection("provider")
            .unwrap();
        let recovery = database.provider_recovery_projection("provider").unwrap();
        let interruption = database.task_interruption_projection("worker").unwrap();
        assert_eq!(tabs, vec!["worker"]);
        assert_eq!(recovery.len(), 1);
        assert_eq!(interruption.as_ref().unwrap().attempt_id, "current");
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(tabs, recovery, interruption))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=provider_interruption tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.owner_session_id',42) WHERE task_id='task_0000'",[]).unwrap();
        assert!(database.task_interruption_projection("worker").is_err());
        assert!(database.provider_recovery_projection("provider").is_err());
        database
            .connection
            .lock()
            .execute("DROP TABLE plan_task_attempts", [])
            .unwrap();
        assert!(database
            .provider_exhaustion_tabs_projection("provider")
            .is_err());
    }
}
