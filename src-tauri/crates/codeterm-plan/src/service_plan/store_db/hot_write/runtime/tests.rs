use super::super::tests::fixtures::{audit, audit_counts, database, fixture};
use super::*;
use crate::plan::{HeartbeatLearning, OrchestratorRef, Plan};
use std::sync::{Arc, Barrier};

#[test]
fn runtime_header_and_heartbeat_work_is_independent_of_unrelated_history() {
    let mut previous = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let dir = tempfile::tempdir().unwrap();
        let plan = fixture(dir.path(), strides, attempts);
        let db = database(dir.path(), &plan);
        audit(&db);
        let (_, header) = db
            .edit_runtime_rows::<PlannerSessionRows, _>(&plan.id, 20, |rows| {
                rows.planner = Some(PlannerRef::Tab {
                    tab_id: "planner".into(),
                    session_id: Some("session".into()),
                    title: None,
                });
                Ok(())
            })
            .unwrap();
        let (_, heartbeat) = db
            .edit_runtime_rows::<HeartbeatRows, _>(&plan.id, 21, |rows| {
                rows.heartbeat.learnings.push(HeartbeatLearning {
                    id: "learn".into(),
                    text: "bounded".into(),
                    ts: 21,
                    tag: None,
                });
                Ok(())
            })
            .unwrap();
        assert_eq!(audit_counts(&db), vec![("plans".into(), 3)]);
        let measured = [header, heartbeat].map(|r| {
            (
                r.task_rows,
                r.attempts,
                r.strides,
                r.header_rows,
                r.decoded_bytes,
                r.serialized_bytes,
            )
        });
        assert!(measured.iter().all(|r| r.0 == 0
            && r.1 == 0
            && r.2 == 0
            && r.4 < 4096
            && r.5 < 4096));
        if let Some(previous) = previous {
            assert_eq!(previous, measured);
        }
        previous = Some(measured);
    }
}

#[test]
fn runtime_stride_and_header_roll_back_together_on_write_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "pause".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pause".into();
    let db = database(dir.path(), &plan);
    let before = db.load_plan(&plan.id).unwrap().unwrap();
    db.connection.lock().execute_batch("CREATE TRIGGER refuse_header BEFORE UPDATE ON plans BEGIN SELECT RAISE(ABORT, 'injected header failure'); END;").unwrap();
    assert!(db
        .edit_runtime_rows::<PauseRows, _>(&plan.id, 20, |rows| {
            rows.mode = PlanMode::Paused;
            rows.strides.push(crate::plan::Milestone {
                id: "pause-stride".into(),
                from: Default::default(),
                kind: "decision".into(),
                text: "pause".into(),
                ts: 20,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(())
        })
        .is_err());
    assert_eq!(db.load_plan(&plan.id).unwrap().unwrap(), before);
}

#[test]
fn concurrent_pause_claims_and_planner_binding_never_load_whole_plans() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let mut plan = Plan::new(
        "pause".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    store.reset_plan_loads_for_test();
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .begin_plan_pause_operation(
                        &id,
                        "pause",
                        Default::default(),
                        vec![],
                        vec![],
                        20,
                    )
                    .unwrap()
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                crate::service_plan::pause_operation::PlanPauseBegin::Applied { .. }
            ))
            .count(),
        1
    );
    assert!(store.plan_loads_for_test().is_empty());
    store
        .with_runtime_rows::<PlannerSessionRows, _>(&plan.id, 21, |rows| {
            rows.planner = Some(PlannerRef::Tab {
                tab_id: "planner".into(),
                session_id: None,
                title: None,
            });
            Ok(())
        })
        .unwrap();
    store
        .complete_planner_report_session(&plan.id, "planner", "session", 22)
        .unwrap();
    assert!(store
        .complete_planner_report_session(&plan.id, "planner", "foreign", 23)
        .is_err());
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn curator_round_rows_stay_bounded_and_commit_state_and_stride_atomically() {
    use crate::plan::CuratorRound;
    let mut previous = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let dir = tempfile::tempdir().unwrap();
        let plan = fixture(dir.path(), strides, attempts);
        let db = database(dir.path(), &plan);
        let (_, receipt) = db
            .edit_runtime_rows::<CuratorRoundRows, _>(&plan.id, 20, |rows| {
                rows.append_round(CuratorRound {
                    number: 1,
                    ended_at: 20,
                    ..Default::default()
                });
                Ok(())
            })
            .unwrap();
        let measured = (
            receipt.task_rows,
            receipt.attempts,
            receipt.strides,
            receipt.header_rows,
            receipt.decoded_bytes,
            receipt.serialized_bytes,
            receipt.projected_text_bytes,
        );
        assert_eq!(
            (receipt.task_rows, receipt.attempts, receipt.strides),
            (0, 0, 1)
        );
        if let Some(previous) = previous {
            assert_eq!(previous, measured);
        }
        previous = Some(measured);
        let stored = db.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.control_loop.last_round.as_ref().unwrap().number, 1);
        assert_eq!(stored.control_loop.task_states.len(), plan.tasks.len());
    }
}

#[test]
fn curator_round_header_write_stays_bounded_as_round_history_grows() {
    use crate::plan::{CuratorCounts, CuratorOutcome, CuratorRound};
    let dir = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "rounds".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let db = database(dir.path(), &plan);
    let now = 30 * 24 * 60 * 60 * 1000_u64;
    let mut writes = Vec::new();
    for number in 100..164_u64 {
        let ended_at = now - 1_000_000 + number;
        let (_, receipt) = db
            .edit_runtime_rows::<CuratorRoundRows, _>(&plan.id, ended_at, |rows| {
                rows.append_round(CuratorRound {
                    number,
                    ended_at,
                    outcome: if number % 2 == 0 {
                        CuratorOutcome::Quiet
                    } else {
                        CuratorOutcome::Nudged
                    },
                    summary: "constant".into(),
                    learnings: 1,
                    ..Default::default()
                });
                Ok(())
            })
            .unwrap();
        let header: i64 = db
            .connection
            .lock()
            .query_row(
                "SELECT length(header_json) FROM plans WHERE id=?1",
                [&plan.id],
                |row| row.get(0),
            )
            .unwrap();
        writes.push((receipt.strides, receipt.serialized_bytes, header));
    }
    // Only the alternating outcome name and `_revision` digits vary; header history
    // would add a whole serialized round per write.
    let spread = |value: fn(&(usize, usize, i64)) -> i64| {
        let values = writes.iter().map(value);
        values.clone().max().unwrap() - values.min().unwrap()
    };
    assert!(writes.iter().all(|write| write.0 == 1), "{writes:?}");
    assert!(spread(|write| write.1 as i64) <= 4, "{writes:?}");
    assert!(spread(|write| write.2) <= 4, "{writes:?}");
    let counts =
        super::super::super::status_queries::control_counts(&db.connection.lock(), &plan.id, now)
            .unwrap();
    assert_eq!(
        counts,
        CuratorCounts {
            quiet: 32,
            nudged: 32,
            learnings: 64,
            ..Default::default()
        }
    );
}

#[test]
fn curator_state_failure_rolls_back_and_concurrent_updates_conserve_both_writes() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let mut plan = Plan::new(
        "curator".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    store.reset_plan_loads_for_test();
    assert!(store
        .edit_curator_state(&plan.id, 2, |rows| {
            rows.control_loop.consecutive_failures = 99;
            Err::<(), _>(PlanError::Validation("injected refusal".into()))
        })
        .is_err());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let id = plan.id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .edit_curator_state(&id, 3, |rows| {
                        rows.control_loop.consecutive_failures += 1;
                        Ok(())
                    })
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    assert!(store.plan_loads_for_test().is_empty());
    assert_eq!(
        store
            .get(&plan.id)
            .unwrap()
            .control_loop
            .consecutive_failures,
        2
    );
}

#[test]
fn unchanged_curator_round_performs_zero_database_writes() {
    let dir = tempfile::tempdir().unwrap();
    let plan = Plan::new(
        "unchanged curator".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let db = database(dir.path(), &plan);
    let before = db.load_plan(&plan.id).unwrap().unwrap();
    let changes = db.connection.lock().total_changes();
    let operations = db.operation_count_for_test(Some(&plan.id));
    for now in [20, 1_020, 86_400_020] {
        let (applied, receipt) = db
            .edit_runtime_rows::<CuratorRoundRows, _>(&plan.id, now, |_| Ok(false))
            .unwrap();
        assert!(!applied);
        assert_eq!(receipt.header_rows, 0);
        assert_eq!(receipt.heartbeat_rows, 0);
        assert_eq!(receipt.strides, 0);
        assert_eq!(db.connection.lock().total_changes(), changes);
        assert_eq!(db.operation_count_for_test(Some(&plan.id)), operations);
    }
    assert_eq!(db.load_plan(&plan.id).unwrap().unwrap(), before);
}
