use super::*;
use std::collections::HashMap;

#[test]
fn dispatch_preflight_selects_status_hold_and_placement_facts_without_history() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let task = &mut plan.tasks[0];
    task.kind = "unplanned".into();
    task.tags = vec!["unplanned".into(), "build-breaking".into()];
    task.model = Some("opaque-model".into());
    task.pause_reason = Some("gate: opaque".into());
    task.status_label = Some("provider_exhausted".into());
    task.files.edit = vec!["owned.rs".into()];
    task.lane.destination = Some("integration".into());
    let database = fixtures::database(directory.path(), &plan);
    for reference in ["task_exact", "NAMED", "cod-1"] {
        let read = database
            .task_dispatch_preflight_projection(&plan.id, reference)
            .unwrap();
        let task = &plan.tasks[0];
        assert_eq!(read.plan_id, plan.id);
        assert_eq!(read.id, task.id);
        assert_eq!(read.name, task.name);
        assert_eq!(read.status, task.status);
        assert_eq!(read.pause_reason, task.pause_reason);
        assert_eq!(read.status_label, task.status_label);
        assert_eq!(read.model, task.model);
        assert_eq!(read.files, task.files);
        assert_eq!(read.lane, task.lane);
        assert!(read.is_unplanned_work && read.build_breaking);
    }
    assert!(matches!(
        database.task_dispatch_preflight_projection("missing", "task_exact"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.task_dispatch_preflight_projection(&plan.id, "missing"),
        Err(PlanError::TaskNotFound)
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_set(row_json,'$.model',42) WHERE plan_id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(database
        .task_dispatch_preflight_projection(&plan.id, "task_exact")
        .is_err());
}

#[test]
fn dispatch_occupancy_matches_held_attempt_order_reports_and_path_rules() {
    for (closed, retired, reported, competitive, live, path) in [
        (false, false, false, false, false, None),
        (false, true, false, false, false, None),
        (true, false, false, false, false, None),
        (false, false, true, false, false, None),
        (false, false, true, false, true, None),
        (false, false, true, true, false, None),
        (false, false, false, false, false, Some(" /other ")),
        (false, false, false, false, false, Some("\u{2003}")),
        (
            false,
            false,
            false,
            false,
            false,
            Some("C:/repo/./checkout"),
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        plan.canonical_project_path = "C:/repo/checkout".into();
        let attempt = &mut plan.tasks[0].execution_attempts[0];
        attempt.closed_at = closed.then_some(1);
        attempt.retired_at = retired.then_some(1);
        attempt.reported_at = reported.then_some(1);
        attempt.competition_group_id = competitive.then(|| "competition".into());
        attempt.worktree_path = path.map(str::to_string);
        let mut second = plan.tasks[0].clone();
        second.id = "second".into();
        second.handle = None;
        second.key = None;
        second.execution_attempts[0].id = "second-attempt".into();
        second.execution_attempts[0].closed_at = None;
        second.execution_attempts[0].worktree_path = None;
        plan.tasks.push(second);
        let database = fixtures::database(directory.path(), &plan);
        let reported: HashMap<_, _> = plan.tasks[0]
            .execution_attempts
            .iter()
            .filter(|attempt| {
                attempt.closed_at.is_none()
                    && attempt.competition_group_id.is_none()
                    && attempt.reported_at.is_some()
            })
            .map(|attempt| {
                (
                    ("task_exact".to_string(), attempt.id.clone()),
                    live.then(|| attempt.executor_tab_id.clone()),
                )
            })
            .collect();
        let live_tabs = if live {
            vec!["worker".to_string()]
        } else {
            vec![]
        };
        let expected = plan.tasks.iter().find_map(|task| {
            task.execution_attempts.iter().find_map(|attempt| {
                if attempt.closed_at.is_some() {
                    return None;
                }
                let tab = reported.get(&(task.id.clone(), attempt.id.clone()));
                if matches!(tab, Some(None)) {
                    return None;
                }
                let path = attempt
                    .worktree_path
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())
                    .unwrap_or(&plan.canonical_project_path);
                (codeterm_paths::normalize_cwd(path)
                    == codeterm_paths::normalize_cwd(&plan.canonical_project_path))
                .then(|| (task.id.clone(), attempt.id.clone(), tab.cloned().flatten()))
            })
        });
        let actual = database
            .dispatch_checkout_occupant_projection(
                &plan.id,
                "task_exact",
                &plan.canonical_project_path,
                &live_tabs,
            )
            .unwrap();
        assert_eq!(
            actual.map(|row| (row.task_id, row.attempt_id, row.executor_tab_id)),
            expected
        );
    }
}

#[test]
fn remote_lane_integration_selects_the_first_exact_attempt_and_reviewed_sha() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let task = &mut plan.tasks[0];
    task.lane.destination = Some("integration".into());
    let first = &mut task.execution_attempts[0];
    first.judge_verdict = Some("accepted".into());
    first.report = Some(crate::plan::TaskExecutionReportSnapshot {
        id: "reviewed-report".into(),
        sha: Some("reviewed-sha".into()),
        ..Default::default()
    });
    first.destination = task.lane.destination.clone();
    first.remote_dispatch_state = Some(crate::plan::RemoteDispatchState::Integrated);
    first.remote_lane_tip = Some("reviewed-tip".into());
    first.remote_merge_commit = Some("merge".into());
    let mut duplicate = first.clone();
    duplicate.judge_verdict = Some("rejected".into());
    task.execution_attempts.push(duplicate);
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .remote_lane_integration_projection(&plan.id, "task_exact", "current")
        .unwrap();
    assert_eq!(read.judge_verdict.as_deref(), Some("accepted"));
    assert_eq!(read.report_sha.as_deref(), Some("reviewed-sha"));
    assert_eq!(read.task_destination, read.destination);
    assert_eq!(read.remote_lane_tip.as_deref(), Some("reviewed-tip"));
    assert_eq!(
        read.remote_dispatch_state,
        Some(crate::plan::RemoteDispatchState::Integrated)
    );
    assert!(matches!(
        database.remote_lane_integration_projection(&plan.id, "task_exact", "missing"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.remote_dispatch_state',42) WHERE plan_id=?1 AND position=0",[&plan.id]).unwrap();
    assert!(database
        .remote_lane_integration_projection(&plan.id, "task_exact", "current")
        .is_err());
}

#[test]
fn dispatch_preflight_and_remote_integration_fixture_ignore_unrequested_growth() {
    let mut baseline = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let current = &mut plan.tasks[0].execution_attempts[0];
        current.id = "selected".into();
        current.reported_at = Some(1);
        current.closed_at = None;
        current.competition_group_id = None;
        let executor_tab_id = current.executor_tab_id.clone();
        let old = (0..history)
            .map(|index| {
                let mut old = current.clone();
                old.id = format!("old-{index}");
                old.closed_at = Some(2);
                old
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.extend(old);
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let task = database
            .task_dispatch_preflight_projection(&plan.id, "task_0000")
            .unwrap();
        let live_tabs = vec![executor_tab_id];
        let checkout = if task.canonical_project_path.trim().is_empty() {
            &task.project_path
        } else {
            &task.canonical_project_path
        };
        let occupant = database
            .dispatch_checkout_occupant_projection(&plan.id, "task_0000", checkout, &live_tabs)
            .unwrap();
        let integration = database
            .remote_lane_integration_projection(&plan.id, "task_0000", "selected")
            .unwrap();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(task, occupant, integration))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 3);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=dispatch_preflight tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_unrequested_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(baseline) = &baseline {
            assert_eq!(&measured, baseline);
        }
        baseline = Some(measured);
    }
}
