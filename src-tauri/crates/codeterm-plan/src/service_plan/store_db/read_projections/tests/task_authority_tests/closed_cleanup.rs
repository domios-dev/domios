use super::*;

#[test]
fn closed_task_cleanup_retains_historical_executor_veto_without_attempt_models() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::service_plan::PlanStore::new_for_test(directory.path());
    let mut plan = authority_plan(directory.path());
    let task = &mut plan.tasks[0];
    task.status = TaskStatus::Done;
    task.assigned_tab_id = None;
    task.lane.tab_id = None;
    task.lane.branch = Some("opaque-branch".into());
    task.lane.workspace_id = Some("workspace".into());
    task.lane.workspace_name = Some("Workspace".into());
    task.lane.cleanup_status = Some("pending".into());
    let mut history = task.execution_attempts[0].clone();
    history.id = "history".into();
    history.executor_tab_id = "historical-worker".into();
    history.closed_at = Some(2);
    history.retired_at = Some(3);
    task.execution_attempts.push(history.clone());
    history.id = "duplicate-history".into();
    task.execution_attempts.push(history);
    store.save(&mut plan, 4).unwrap();
    let read = store
        .closed_task_lane_cleanup_projection(&plan.id, "task_exact")
        .unwrap()
        .unwrap();
    let task = read.task.as_ref().unwrap();
    assert_eq!(task.executor_tabs, ["worker", "historical-worker"]);
    assert_eq!(task.workspace_id.as_deref(), Some("workspace"));
    assert_eq!(task.workspace_name.as_deref(), Some("Workspace"));
    assert_eq!(task.cleanup_status.as_deref(), Some("pending"));
    assert_eq!(read.canonical_path(), plan.project_path);
    let before = store
        .task_lane_metadata_projection(&plan.id, "task_exact")
        .unwrap();
    let outcome = crate::service_plan::lane_cleanup::try_cleanup_task_lane(
        &store,
        &read,
        "task_exact",
        |tab| tab == "historical-worker",
        5,
    );
    assert_eq!(
        outcome,
        crate::service_plan::lane_cleanup::LaneCleanupOutcome::Refused(
            "lane Worker tab is still live; close it before cleanup".into()
        )
    );
    assert_eq!(
        store
            .task_lane_metadata_projection(&plan.id, "task_exact")
            .unwrap(),
        before
    );
    assert!(store
        .closed_task_lane_cleanup_projection("missing", "task_exact")
        .unwrap()
        .is_none());
    assert!(store
        .closed_task_lane_cleanup_projection(&plan.id, "missing")
        .unwrap()
        .unwrap()
        .task
        .is_none());
}

#[test]
fn task_cleanup_debt_matches_the_existing_plan_result_filtered_by_task_id() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::service_plan::PlanStore::new_for_test(directory.path());
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].status = TaskStatus::Done;
    plan.tasks[0].lane.cleanup_status = Some("pending".into());
    plan.tasks[0].lane.cleanup_hints = vec!["opaque residue".into()];
    let mut second = plan.tasks[0].clone();
    second.id = "second".into();
    second.handle = None;
    second.key = None;
    second.name = "second task".into();
    plan.tasks.push(second);
    store.save(&mut plan, 1).unwrap();
    for live in [false, true] {
        let all = store
            .cleanup_debt_projection(&plan.id, |_| live)
            .unwrap()
            .unwrap();
        assert_eq!(all.len(), 2);
        for task in ["task_exact", "second", "missing"] {
            let expected = all
                .iter()
                .filter(|debt| debt.task_id == task)
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(
                store
                    .task_cleanup_debt_projection(&plan.id, task, |_| live)
                    .unwrap()
                    .unwrap(),
                expected
            );
        }
    }
    assert!(store
        .task_cleanup_debt_projection("missing", "task_exact", |_| false)
        .unwrap()
        .is_none());
}

#[test]
fn selected_closed_cleanup_fixture_is_invariant_to_unrequested_rows_and_history() {
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
        plan.tasks[0].status = TaskStatus::Done;
        plan.tasks[0].lane.cleanup_status = Some("pending".into());
        plan.tasks[0].lane.cleanup_hints = vec!["selected hint".into()];
        plan.tasks[1].status = TaskStatus::Done;
        plan.tasks[1].lane.cleanup_hints = vec!["x".repeat(body)];
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
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
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = database
            .closed_task_lane_cleanup_projection(&plan.id, "task_0000")
            .unwrap()
            .unwrap();
        assert_eq!(read.task.as_ref().unwrap().executor_tabs.len(), 1);
        let debt = database
            .cleanup_debt_input(&plan.id, Some("task_0000"))
            .unwrap()
            .unwrap();
        assert_eq!(debt.tasks.len(), 1);
        assert_eq!(debt.tasks[0].id, "task_0000");
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len()
            + serde_json::to_vec(&debt.tasks[0].cleanup_hints)
                .unwrap()
                .len();
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=closed_task_cleanup tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_unrequested_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
