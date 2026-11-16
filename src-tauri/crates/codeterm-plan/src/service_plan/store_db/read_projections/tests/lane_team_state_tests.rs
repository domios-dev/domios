use super::*;

fn bounded_state(consumer: &str) {
    let mut previous = None;
    for (strides, attempts, body, rounds) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.project_path = "/project".into();
        plan.canonical_project_path = "/canonical".into();
        plan.team_id = Some("team".into());
        plan.tasks[0].status = crate::plan::TaskStatus::InProgress;
        plan.tasks[0].lane.worktree_path = Some("/lane".into());
        plan.tasks[0].execution_attempts[0].worktree_path = Some("/lane".into());
        plan.tasks[0].execution_attempts[0].lane_turn_stats = vec![
            crate::plan::LaneTurnChangeStats {
                turn: 7,
                ..Default::default()
            },
            crate::plan::LaneTurnChangeStats {
                turn: 3,
                ..Default::default()
            },
        ];
        plan.tasks[1].body.push_str(&"x".repeat(body));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| crate::plan::CuratorRound {
                    number,
                    summary: "unrelated history".repeat(32),
                    ..Default::default()
                })
                .collect(),
        ));
        if consumer == "lifecycle_plan" {
            plan.mode = crate::plan::PlanMode::Paused;
            plan.paused_from_mode = Some(crate::plan::PlanMode::Execution);
            let operation: crate::plan::PlanPauseOperation = serde_json::from_value(json!({
                "id":"current-pause","paused_at":1,"paused_from_mode":"execution",
                "reason":"pause","stage":"complete"
            }))
            .unwrap();
            plan.pause_operations = (0..rounds)
                .map(|_| {
                    let mut old = operation.clone();
                    old.reason = "history".repeat(32);
                    old
                })
                .chain([operation.clone()])
                .collect();
        }
        if matches!(
            consumer,
            "settle_lane"
                | "run_settlement"
                | "settle_lanes_held_by_closed_tab"
                | "settle_finished_lane_history"
        ) {
            plan.tasks[0].status = crate::plan::TaskStatus::Done;
        }
        if consumer == "find_lane_attempt" {
            let attempt = &mut plan.tasks[0].execution_attempts[0];
            attempt.branch = Some("feature".into());
            attempt.lane_ownership = Some(crate::plan::LaneOwnership::ForeignCheckout);
        }
        if consumer == "run_lane_watchdog" {
            plan.tasks[0].assigned_tab_id = Some("worker".into());
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = match consumer {
            "restore_lane_watches" => {
                let read = database.lane_watch_projection().unwrap();
                assert_eq!(read.len(), 1);
                assert_eq!(read[0].worktree_path, "/lane");
                assert_eq!(read[0].canonical_path, "/canonical");
                serde_json::to_vec(&read).unwrap()
            }
            "find_lane_attempt" => {
                let read = database.lane_attempt_projection("worker").unwrap().unwrap();
                assert_eq!(read.plan_id, plan.id);
                assert_eq!(read.task_id, plan.tasks[0].id);
                assert_eq!(read.attempt_id, "current");
                assert_eq!(read.worktree_path, "/lane");
                assert_eq!(read.canonical_path, "/canonical");
                assert_eq!(read.turn, 7);
                assert_eq!(read.branch.as_deref(), Some("feature"));
                assert_eq!(
                    read.ownership,
                    Some(crate::plan::LaneOwnership::ForeignCheckout)
                );
                serde_json::to_vec(&read).unwrap()
            }
            "active_team_plans" => {
                let read = database
                    .team_execution_projection(&["team".into()])
                    .unwrap();
                assert_eq!(read.len(), 1);
                assert_eq!(read[0].id, plan.id);
                assert_eq!(read[0].team_id, "team");
                assert_eq!(read[0].mode, plan.mode);
                assert!(read[0].has_running_tasks);
                serde_json::to_vec(&read).unwrap()
            }
            "settle_lane"
            | "run_settlement"
            | "settle_lanes_held_by_closed_tab"
            | "settle_finished_lane_history" => {
                let read = if consumer == "settle_lanes_held_by_closed_tab" {
                    database
                        .lane_settlement_projection(None, None, Some("worker"))
                        .unwrap()
                } else {
                    database
                        .lane_settlement_projection(Some(&plan.id), Some(&plan.tasks[0].id), None)
                        .unwrap()
                };
                assert_eq!(read.len(), 1);
                assert_eq!(read[0].task_id, plan.tasks[0].id);
                assert_eq!(read[0].bound_tabs, vec!["worker"]);
                assert_eq!(read[0].worktree.as_deref(), Some("/lane"));
                assert_eq!(read[0].canonical_path(), "/canonical");
                serde_json::to_vec(&read).unwrap()
            }
            "clean_plan_lanes_with_progress" => {
                let read = database.lane_cleanup_projection(&plan.id).unwrap();
                assert_eq!(
                    serde_json::to_value(&read).unwrap(),
                    serde_json::to_value(crate::service_plan::LaneCleanupPlanRead::from(&plan))
                        .unwrap()
                );
                assert_eq!(read.tasks.len(), fixtures::TASKS);
                serde_json::to_vec(&read).unwrap()
            }
            "run_lane_watchdog" => {
                let read = database.lane_watchdog_projection().unwrap();
                assert_eq!(read.len(), 1);
                assert_eq!(
                    serde_json::to_value(&read[0]).unwrap(),
                    serde_json::to_value(crate::service_plan::LaneWatchdogRead::from(&plan))
                        .unwrap()
                );
                assert_eq!(
                    read[0].tasks[0].expected_session.as_deref(),
                    Some("session")
                );
                serde_json::to_vec(&read).unwrap()
            }
            "lifecycle_plan" => {
                let read = database.plan_lifecycle_projection(&plan.id).unwrap();
                let expected = crate::service_plan::PlanLifecycleRead::from(&plan);
                assert_eq!(
                    serde_json::to_value(&read).unwrap(),
                    serde_json::to_value(expected).unwrap()
                );
                serde_json::to_vec(&read).unwrap()
            }
            "take_over_team_plan" => {
                let read = database.team_owner_projection("team").unwrap().unwrap();
                assert_eq!(read.0, plan.id);
                assert_eq!(read.1, plan.owner.tab_id);
                serde_json::to_vec(&read).unwrap()
            }
            _ => unreachable!(),
        };
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 1);
        let limit = if consumer == "clean_plan_lanes_with_progress" {
            1024 * 1024
        } else {
            512
        };
        assert!(metrics.materialized_bytes < limit);
        assert!(decoded < limit);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer={consumer} tasks={} base_bytes={} strides={strides} unrelated_attempts={attempts} extra_body={body} rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={} changed_rows=0", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, metrics.queries, metrics.materialized_bytes, response.len());
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn restore_lane_watches_selected_bytes() {
    bounded_state("restore_lane_watches");
}
#[test]
fn find_lane_attempt_selected_bytes() {
    bounded_state("find_lane_attempt");
}
#[test]
fn active_team_plans_selected_bytes() {
    bounded_state("active_team_plans");
}
#[test]
fn take_over_team_plan_selected_bytes() {
    bounded_state("take_over_team_plan");
}

#[test]
fn lane_and_team_state_preserve_selection_and_surface_selected_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.team_id = Some("team".into());
    plan.tasks[0].lane.worktree_path = Some("/lane".into());
    let older = &mut plan.tasks[0].execution_attempts[0];
    older.worktree_path = Some("/old".into());
    older.retired_at = Some(2);
    let mut newer = older.clone();
    newer.id = "newer".into();
    newer.worktree_path = Some("/newer".into());
    newer.placement = crate::plan::WorkerPlacement::Shared;
    plan.tasks[0].execution_attempts.push(newer);
    plan.tasks[1].status = crate::plan::TaskStatus::Done;
    plan.tasks[1].lane.worktree_path = Some("/finished".into());
    plan.tasks[2].lane.worktree_path = Some("/cleaned".into());
    plan.tasks[2].lane.cleanup_status = Some("done".into());
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(database.lane_watch_projection().unwrap().len(), 1);
    assert_eq!(
        database
            .lane_attempt_projection("worker")
            .unwrap()
            .unwrap()
            .worktree_path,
        "/old"
    );
    assert!(database
        .lane_attempt_projection("missing")
        .unwrap()
        .is_none());
    assert!(database.team_owner_projection("missing").unwrap().is_none());
    assert!(database.team_execution_projection(&[]).unwrap().is_empty());
    assert!(
        !database
            .team_execution_projection(&["team".into()])
            .unwrap()[0]
            .has_running_tasks
    );
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.mode','invalid') WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.team_execution_projection(&["team".into()]),
        Err(PlanError::Quarantined { .. })
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_task_attempts")
        .unwrap();
    assert!(database.lane_attempt_projection("worker").is_err());
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_tasks")
        .unwrap();
    assert!(database.lane_watch_projection().is_err());
    assert!(database
        .team_execution_projection(&["team".into()])
        .is_err());
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE specs; DROP TABLE plans")
        .unwrap();
    assert!(database.team_owner_projection("team").is_err());
}

#[test]
fn lifecycle_plan_selected_bytes() {
    bounded_state("lifecycle_plan");
}

#[test]
fn settle_lane_selected_bytes() {
    bounded_state("settle_lane");
}
#[test]
fn run_settlement_selected_bytes() {
    bounded_state("run_settlement");
}
#[test]
fn settle_lanes_held_by_closed_tab_selected_bytes() {
    bounded_state("settle_lanes_held_by_closed_tab");
}
#[test]
fn settle_finished_lane_history_selected_bytes() {
    bounded_state("settle_finished_lane_history");
}

#[test]
fn clean_plan_lanes_with_progress_selected_bytes() {
    bounded_state("clean_plan_lanes_with_progress");
}
#[test]
fn run_lane_watchdog_selected_bytes() {
    bounded_state("run_lane_watchdog");
}

#[test]
fn cleanup_watchdog_and_lifecycle_read_errors_are_loud() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.tasks[0].status = crate::plan::TaskStatus::InProgress;
    let database = fixtures::database(directory.path(), &plan);
    assert!(matches!(
        database.lane_cleanup_projection("missing"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.plan_lifecycle_projection("missing"),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',17) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(database.plan_lifecycle_projection(&plan.id).is_err());
    assert!(database.lane_watchdog_projection().is_err());
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_task_attempts")
        .unwrap();
    assert!(database.lane_cleanup_projection(&plan.id).is_err());
    assert!(database.lane_watchdog_projection().is_err());
}
