use super::*;

#[test]
fn launch_and_seed_reads_preserve_selected_fields_membership_and_predecessor_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].acceptance_criteria = vec!["criterion".into()];
    let template = plan.tasks[0].execution_attempts[0].clone();
    let mut attempts = Vec::new();
    for (index, (tab, closed, retired, group, remote)) in [
        ("older", true, false, false, false),
        ("retired", false, true, false, false),
        ("live", false, false, false, false),
        ("competitor", true, false, true, false),
        ("remote", true, false, false, true),
        ("\u{2003}older\u{2003}", true, false, false, false),
        ("new-tab", true, false, false, false),
        ("\u{2003}", true, false, false, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut attempt = template.clone();
        attempt.id = format!("attempt-{index}");
        attempt.executor_tab_id = tab.into();
        attempt.closed_at = closed.then_some(2);
        attempt.retired_at = retired.then_some(3);
        attempt.competition_group_id = group.then(|| "group".into());
        attempt.mesh_peer = remote.then(|| "peer".into());
        attempts.push(attempt);
    }
    plan.tasks[0].execution_attempts = attempts;
    let database = fixtures::database(directory.path(), &plan);
    let seed = database
        .task_dispatch_seed_projection(&plan.id, "NAMED", "new-tab")
        .unwrap();
    assert_eq!(seed.acceptance_criteria, ["criterion"]);
    assert_eq!(seed.predecessor_tabs, ["older", "retired"]);
    for source in [None, Some("attempt-0"), Some("missing")] {
        let launch = database
            .task_launch_projection(&plan.id, "NAMED", source)
            .unwrap();
        assert_eq!(launch.task_id, plan.tasks[0].id);
        assert_eq!(launch.name, plan.tasks[0].name);
        assert_eq!(launch.status, plan.tasks[0].status);
        assert_eq!(launch.lane, plan.tasks[0].lane);
        assert_eq!(launch.execution_intent, plan.tasks[0].execution_intent);
        assert_eq!(
            launch.dispatch_intent,
            plan.tasks[0].interruption.execution_intent
        );
        assert_eq!(launch.source_attempt_exists, source == Some("attempt-0"));
    }
    assert!(matches!(
        database.task_launch_projection("missing", "NAMED", None),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.task_dispatch_seed_projection(&plan.id, "missing", "new-tab"),
        Err(PlanError::TaskNotFound)
    ));
}

#[test]
fn launch_and_seed_defaults_and_selected_corruption_remain_fallible() {
    let directory = tempfile::tempdir().unwrap();
    let plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_remove(row_json,'$.lane','$.execution_intent','$.interruption','$.acceptance_criteria')",[]).unwrap();
    let launch = database
        .task_launch_projection(&plan.id, "task_exact", None)
        .unwrap();
    assert_eq!(launch.lane, crate::plan::TaskLaneMetadata::default());
    assert_eq!(
        launch.execution_intent,
        crate::plan::TaskExecutionIntent::default()
    );
    assert_eq!(
        launch.dispatch_intent,
        crate::plan::ExecutionIntent::default()
    );
    assert!(database
        .task_dispatch_seed_projection(&plan.id, "task_exact", "new-tab")
        .unwrap()
        .acceptance_criteria
        .is_empty());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.acceptance_criteria',42,'$.name',42)",[]).unwrap();
    assert!(database
        .task_launch_projection(&plan.id, "task_exact", None)
        .is_err());
    assert!(database
        .task_dispatch_seed_projection(&plan.id, "task_exact", "new-tab")
        .is_err());
}

#[test]
fn launch_and_seed_fixture_ignore_unrequested_body_and_duplicate_predecessor_history() {
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
        let closed = (0..=history)
            .map(|index| {
                let mut attempt = original.clone();
                attempt.id = format!("closed-{index}");
                attempt.executor_tab_id = "prior-worker".into();
                attempt.closed_at = Some(2);
                attempt.competition_group_id = None;
                attempt.mesh_peer = None;
                attempt
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, closed);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let launch = database
            .task_launch_projection(&plan.id, "task_0000", Some(&original.id))
            .unwrap();
        assert!(launch.source_attempt_exists);
        let seed = database
            .task_dispatch_seed_projection(&plan.id, "task_0000", "new-tab")
            .unwrap();
        assert_eq!(seed.predecessor_tabs, ["prior-worker"]);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(launch, seed)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=launch_seed tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
