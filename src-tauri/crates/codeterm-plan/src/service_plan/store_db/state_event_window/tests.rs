use super::*;
use crate::service_plan::store_db::{hot_write::tests::fixtures, read_projections};

#[test]
fn indexed_board_events_match_legacy_with_long_runs_and_renamed_authors() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 400, 20);
    let mut renamed = plan.tasks[0].execution_attempts[0].clone();
    renamed.executor_title = Some("Renamed worker".into());
    plan.tasks
        .last_mut()
        .unwrap()
        .execution_attempts
        .push(renamed.clone());
    renamed.id = "alias-attempt".into();
    renamed.executor_tab_id = "alias".into();
    plan.tasks
        .last_mut()
        .unwrap()
        .execution_attempts
        .push(renamed);
    for (i, stride) in plan.strides.iter_mut().enumerate() {
        stride.ts = 1000 + i as u64;
        stride.kind = if i >= 270 || i % 3 != 0 {
            "worker_report"
        } else {
            "decision"
        }
        .into();
        stride.text = format!("\u{a0}report {i}\nsecond line\u{a0}");
        if i % 2 == 0 {
            stride.from = crate::plan::AgentRef::from_tab("alias");
        }
    }
    for i in 0..12 {
        let mut milestone = plan.strides[0].clone();
        milestone.id = format!("linked-{i}");
        milestone.kind = "tab_todo_linked".into();
        milestone.text = "same link\nignored".into();
        milestone.ts = 1100;
        plan.tasks[i % 3].milestones.push(milestone);
    }
    let mut planning = plan.strides[0].clone();
    planning.id = "multiline-planning".into();
    planning.kind = "planning".into();
    planning.text = "keep the entire\nplanning payload".into();
    planning.ts = 1105;
    plan.tasks[0].milestones.push(planning);
    let database = fixtures::database(root.path(), &plan);
    let connection = database.connection.lock();
    connection.execute("UPDATE plans SET header_json=json_set(header_json,
        '$.planning_log',json('[{\"at\":1050,\"by\":{\"kind\":\"user\"},\"from\":\"draft\",\"to\":\"ready\"}]'),
        '$.gates',json('[{\"asked_at\":1100,\"asked_by_tab_id\":\"worker\",\"question\":\" \\nA question\\nmore\"}]')) WHERE id=?1",[&plan.id]).unwrap();
    for (limit, offset) in [(50, 0), (50, 50), (7, 1), (50, 10000), (0, 0)] {
        let old = super::super::state_event_queries::legacy_event_window(
            &connection,
            &plan.id,
            limit,
            offset,
        )
        .unwrap();
        let new = event_window(&connection, &plan.id, limit, offset).unwrap();
        assert_eq!(
            serde_json::to_vec(&new).unwrap(),
            serde_json::to_vec(&old).unwrap(),
            "limit={limit} offset={offset}"
        );
    }
    let value = event_window(&connection, &plan.id, 50, 0).unwrap();
    assert_eq!(value.entries.last().unwrap().count, 132);
    assert_eq!(value.entries.last().unwrap().author, "Renamed worker");
}

#[test]
fn sql_hot_board_window_visits_only_the_tail_as_history_grows() {
    for history in [200, 20000] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), history, 100);
        for (i, stride) in plan.strides.iter_mut().enumerate() {
            stride.ts = 1000 + i as u64;
        }
        let database = fixtures::database(root.path(), &plan);
        fixtures::audit(&database);
        VISITED.with(|visited| visited.set(0));
        read_projections::take_metrics();
        let board = database
            .board_state_projection(&plan.id, 100, 0, true)
            .unwrap();
        let visited = VISITED.with(|visited| visited.get());
        let metrics = read_projections::take_metrics();
        assert_eq!(visited, 51);
        assert!(metrics.queries <= 16, "queries={}", metrics.queries);
        assert_eq!(board.value.mission_control.event_log.entries.len(), 50);
        assert!(fixtures::audit_counts(&database).is_empty());
        let connection = database.connection.lock();
        for (sql, index) in [
            (TASKS, "plan_tasks_event_order"),
            (STRIDES, "plan_strides_event_order"),
            (MILESTONES, "plan_milestones_event_order"),
        ] {
            let plan_rows = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([&plan.id], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(
                plan_rows
                    .iter()
                    .any(|row| row.contains("SEARCH") && row.contains(index)),
                "{plan_rows:?}"
            );
            assert!(
                !plan_rows.iter().any(|row| row.contains("MATERIALIZE")),
                "{plan_rows:?}"
            );
            if sql != MILESTONES {
                assert!(
                    !plan_rows.iter().any(|row| row.contains("TEMP B-TREE")),
                    "{plan_rows:?}"
                );
            }
        }
        let mut statement = connection.prepare(STRIDES).unwrap();
        {
            let mut rows = statement.query([&plan.id]).unwrap();
            for _ in 0..51 {
                assert!(rows.next().unwrap().is_some());
            }
        }
        let steps = statement.get_status(rusqlite::StatementStatus::VmStep);
        assert!(steps < 3000, "history={history} vm_steps={steps}");
        println!("fixture={} strides={history} visited_events={visited} queries={} stride_vm_steps={steps}",fixtures::SEED,metrics.queries);
    }
}

#[test]
fn collapsed_milestones_materialize_only_first_lines_and_distinct_task_names() {
    let mut previous = None;
    for extra in [0, 32768] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = crate::plan::Plan::new(
            "Event window".into(),
            root.path().display().to_string(),
            Default::default(),
        );
        let mut task = crate::service_plan::load_audit::task(
            "one",
            crate::plan::TaskStatus::InProgress,
            Some("worker"),
        );
        task.milestones = (0..1000)
            .map(|number| crate::plan::Milestone {
                id: format!("report-{number}"),
                kind: "worker_report".into(),
                text: format!("fixed first line\n{}", "x".repeat(extra)),
                from: crate::plan::AgentRef::from_tab("worker"),
                ts: 100 + number,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            })
            .collect();
        plan.tasks.push(task);
        let database = fixtures::database(root.path(), &plan);
        fixtures::audit(&database);
        read_projections::take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let connection = database.connection.lock();
        let result = event_window(&connection, &plan.id, 50, 0).unwrap();
        drop(connection);
        assert_eq!(result.entries.len(), 2);
        let report = result
            .entries
            .iter()
            .find(|entry| entry.kind == "worker_report")
            .unwrap();
        assert_eq!(report.count, 1000);
        assert_eq!(report.text, "fixed first line");
        assert_eq!(
            report.task_name.as_deref(),
            Some(plan.tasks[0].name.as_str())
        );
        let metrics = read_projections::take_metrics();
        let response = serde_json::to_vec(&result).unwrap().len();
        assert_eq!(crate::service_plan::store_sync::take_decoded_row_bytes(), 0);
        assert_eq!(
            crate::service_plan::store_rows::take_serialized_row_bytes(),
            0
        );
        assert!(fixtures::audit_counts(&database).is_empty());
        let measured = (metrics, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}
