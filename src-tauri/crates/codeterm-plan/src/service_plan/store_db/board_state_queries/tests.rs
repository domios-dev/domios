use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;

#[test]
fn board_state_reads_exact_counts_and_event_windows_without_histories_or_writes() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 125, 20);
    plan.tasks[0].status = crate::plan::TaskStatus::Done;
    plan.tasks[1].status = crate::plan::TaskStatus::Blocked;
    let database = fixtures::database(root.path(), &plan);
    fixtures::audit(&database);
    let first = database
        .board_state_projection(&plan.id, 100, 0, true)
        .unwrap();
    assert!(!first.complete);
    let first_value = serde_json::to_value(&first.value).unwrap();
    assert_eq!(first_value["revision"], "1");
    assert_eq!(first_value["task_counts"]["total"], 1162);
    assert_eq!(first_value["task_counts"]["done"], 1);
    assert_eq!(first_value["task_counts"]["blocked"], 1);
    assert_eq!(
        first_value["mission_control"]["bounces"]["total_attempts"],
        21
    );
    assert_eq!(
        first_value["mission_control"]["bounces"]["first_try_done"],
        1
    );
    assert_eq!(
        first_value["mission_control"]["event_log"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        50
    );
    assert_eq!(first_value["next_event_cursor"], 50);
    let next = database
        .board_state_projection(&plan.id, 100, 50, true)
        .unwrap();
    let next_value = serde_json::to_value(&next.value).unwrap();
    assert_eq!(next_value["revision"], first_value["revision"]);
    assert_eq!(
        next_value["mission_control"]["event_log"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        50
    );
    assert_eq!(next_value["next_event_cursor"], 100);
    assert!(first_value["control_loop"].get("rounds").is_none());
    assert!(first_value["control_loop"].get("task_states").is_none());
    assert_eq!(first.mode, plan.mode);
    assert_eq!(first.status, plan.status);
    assert_eq!(first.team_id, plan.team_id);
    assert!(database
        .board_state_projection("missing", 100, 0, true)
        .is_err());
    let summary = database
        .board_state_projection(&plan.id, 100, 0, false)
        .unwrap();
    let summary_value = serde_json::to_value(&summary.value).unwrap();
    assert_eq!(
        summary_value["mission_control"]["event_log"]["recorded_events"],
        first_value["mission_control"]["event_log"]["recorded_events"]
    );
    assert!(summary_value["mission_control"]["event_log"]["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(summary_value["event_window_loaded"], false);
    let connection = database.connection.lock();
    let root: i64 = connection
        .query_row(
            "SELECT rootpage FROM sqlite_schema WHERE name='plan_tasks'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let body: i64 = connection
        .query_row(
            "SELECT cid FROM pragma_table_xinfo('plan_tasks') WHERE name='row_json'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for (surface, sql) in [("counts", COUNTS_SQL), ("bounces", BOUNCES_SQL)] {
        let program = connection
            .prepare(&format!("EXPLAIN {sql}"))
            .unwrap()
            .query_map([&plan.id], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let cursors: std::collections::HashSet<_> = program
            .iter()
            .filter(|row| row.0 == "OpenRead" && row.2 == root)
            .map(|row| row.1)
            .collect();
        assert!(
            program
                .iter()
                .all(|row| row.0 != "Column" || !cursors.contains(&row.1) || row.2 != body),
            "{surface} must not read task body JSON"
        );
        println!("board_surface={surface} task_body_column_reads=0");
    }
    drop(connection);
    assert!(fixtures::audit_counts(&database).is_empty());
}

#[test]
fn board_state_cache_reuses_unchanged_revision_and_invalidates_on_write_or_window_expiry() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 60, 10);
    let window_ms: u64 = 7 * 24 * 60 * 60 * 1000;
    let round_ts = 1_000_000u64;
    plan.strides.push(crate::plan::Milestone {
        id: "round-1".into(),
        kind: "curator_round".into(),
        text: serde_json::json!({"outcome": "acted", "learnings": 2}).to_string(),
        from: crate::plan::AgentRef::default(),
        ts: round_ts,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    });
    let database = fixtures::database(root.path(), &plan);
    crate::service_plan::store_db::read_projections::take_metrics();

    let first = database
        .board_state_projection(&plan.id, round_ts + 1_000, 0, true)
        .unwrap();
    let miss_metrics = crate::service_plan::store_db::read_projections::take_metrics();
    assert!(miss_metrics.queries > 1);
    assert_eq!(first.value.control_loop.counts_7d.rounds, 1);

    // Same revision within the 7-day window reuses the cache with a single PK lookup.
    let hit = database
        .board_state_projection(&plan.id, round_ts + 60_000, 0, true)
        .unwrap();
    let hit_metrics = crate::service_plan::store_db::read_projections::take_metrics();
    assert_eq!(hit_metrics.queries, 1);
    assert_eq!(
        serde_json::to_vec(&hit.value).unwrap(),
        serde_json::to_vec(&first.value).unwrap()
    );

    // Advancing past the oldest included curator_round's 7-day expiry recomputes counts_7d.
    let expired = database
        .board_state_projection(&plan.id, round_ts + window_ms + 1, 0, true)
        .unwrap();
    let expired_metrics = crate::service_plan::store_db::read_projections::take_metrics();
    assert_eq!(expired_metrics.queries, 2);
    assert_eq!(expired.value.control_loop.counts_7d.rounds, 0);

    // Persisting a revision change invalidates the cached board state.
    plan.tasks[0].status = crate::plan::TaskStatus::Done;
    plan.updated_at += 1;
    database.persist_plan(&plan, None).unwrap();
    crate::service_plan::store_db::read_projections::take_metrics();
    let updated = database
        .board_state_projection(&plan.id, round_ts + 60_000, 0, true)
        .unwrap();
    assert_ne!(updated.value.revision, first.value.revision);
    assert_eq!(
        updated.value.task_counts.done,
        first.value.task_counts.done + 1
    );
}
