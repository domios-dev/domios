use super::*;

fn bounded_read(consumer: &str, identity: bool, inventory: bool) {
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
        plan.tasks[0].lane.worktree_path = Some("/lane".into());
        plan.tasks[0].lane.branch = Some("lane/branch".into());
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
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = if identity {
            let read = database.plan_identity_projection(&plan.id).unwrap();
            assert_eq!(read.owner, plan.owner);
            assert_eq!(read.team_id, plan.team_id);
            serde_json::to_vec(&read).unwrap()
        } else {
            let rows = if inventory {
                database.recorded_task_lanes_projection().unwrap()
            } else {
                vec![database
                    .task_lane_reference_projection(&plan.id, &plan.tasks[0].id)
                    .unwrap()
                    .unwrap()]
            };
            assert_eq!(rows.len(), 1);
            let read = &rows[0];
            assert_eq!(read.plan_id, plan.id);
            assert_eq!(read.task_id, plan.tasks[0].id);
            assert_eq!(read.worktree_path, plan.tasks[0].lane.worktree_path);
            assert_eq!(read.branch, plan.tasks[0].lane.branch);
            assert_eq!(read.canonical_path, "/canonical");
            serde_json::to_vec(&rows).unwrap()
        };
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 512);
        assert!(decoded < 512);
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
fn clear_removed_task_lane_references_selected_bytes() {
    bounded_read("clear_removed_task_lane_references", false, true);
}

#[test]
fn project_for_recorded_lane_selected_bytes() {
    bounded_read("project_for_recorded_lane", false, true);
}

#[test]
fn clear_vanished_lane_on_cleanup_done_selected_bytes() {
    bounded_read("clear_vanished_lane_on_cleanup_done", false, false);
}

#[test]
fn restore_close_inventory_selected_bytes() {
    bounded_read("restore_close_inventory_identity", true, false);
    bounded_read("restore_close_inventory_lane", false, false);
}

#[test]
fn settle_close_inventory_selected_bytes() {
    bounded_read("settle_close_inventory", true, false);
}

#[test]
fn validate_restore_plans_selected_bytes() {
    bounded_read("validate_restore_plans", true, false);
}

#[test]
fn team_plan_owner_selected_bytes() {
    bounded_read("team_plan_owner", true, false);
}

#[test]
fn transfer_team_plan_core_selected_bytes() {
    bounded_read("transfer_team_plan_core", true, false);
}

#[test]
fn lane_references_keep_order_missing_and_sql_failures_loud() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.canonical_project_path = "  ".into();
    plan.tasks[0].lane.worktree_path = Some("/first".into());
    plan.tasks[1].lane.worktree_path = Some("/second".into());
    let database = fixtures::database(directory.path(), &plan);
    let rows = database.recorded_task_lanes_projection().unwrap();
    assert_eq!(rows[0].task_id, plan.tasks[0].id);
    assert_eq!(rows[1].task_id, plan.tasks[1].id);
    assert_eq!(rows[0].canonical_path, plan.project_path);
    assert!(database
        .task_lane_reference_projection("missing", "missing")
        .unwrap()
        .is_none());
    assert!(database
        .task_lane_reference_projection(&plan.id, "missing")
        .unwrap()
        .is_none());
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_set(row_json,'$.lane.branch',17) WHERE task_id=?1",
            [&plan.tasks[0].id],
        )
        .unwrap();
    assert!(database.recorded_task_lanes_projection().is_err());
    assert!(database
        .task_lane_reference_projection(&plan.id, &plan.tasks[0].id)
        .is_err());
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_tasks")
        .unwrap();
    assert!(database.recorded_task_lanes_projection().is_err());
    assert!(database
        .task_lane_reference_projection(&plan.id, &plan.tasks[0].id)
        .is_err());
}
