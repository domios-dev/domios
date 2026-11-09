use super::*;
use crate::service_plan::load_audit;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use std::sync::{Arc, Barrier};

#[test]
fn lane_writes_ignore_unrelated_history_and_bodies() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        plan.tasks[0].status = crate::plan::TaskStatus::Done;
        plan.tasks[0].lane.worktree_path = Some("selected-worktree".into());
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let output = store
            .edit_lane_turn_stats(&plan.id, "task_0000", "current", 20, |stats| {
                stats.push(LaneTurnChangeStats {
                    turn: 1,
                    captured_at: 20,
                    ..Default::default()
                });
                Ok(Some(1u32))
            })
            .unwrap();
        assert_eq!(output, Some(1));
        let attempt = super::super::take_work();
        let count = store
            .settle_lane_history(&plan.id, 21, |reads| {
                reads
                    .iter()
                    .map(|read| {
                        (
                            read.task_id.clone(),
                            TaskLaneUpdate {
                                cleanup_status: Some(Some("done".into())),
                                ..Default::default()
                            },
                        )
                    })
                    .collect()
            })
            .unwrap();
        assert_eq!(count, 1);
        let history = super::super::take_work();
        let writes = fixtures::audit_counts(&store.database);
        println!("fixture={} tasks={} base_content_bytes={} operation=lane_stats_history strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} writes={writes:?} attempt={attempt:?} history={history:?} response_bytes=1",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES);
        assert!(store.plan_loads_for_test().is_empty());
        assert_eq!((attempt[0], attempt[2]), (0, 1));
        assert_eq!((history[0], history[2]), (1, 0));
        assert!(attempt[5] < 8192 && attempt[6] < 8192);
        let measured = (attempt, history, writes);
        if let Some(prior) = prior {
            assert_eq!(prior, measured);
        }
        prior = Some(measured);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(
            &store.database,
        );
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(
            stored.tasks[0].execution_attempts[0].lane_turn_stats.len(),
            1
        );
        assert_eq!(stored.tasks[0].lane.cleanup_status.as_deref(), Some("done"));
        assert_eq!(stored.tasks[1], plan.tasks[1]);
    }
}

#[test]
fn lane_history_second_row_failure_is_atomic() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = crate::plan::Plan::new(
        "lanes".into(),
        root.path().display().to_string(),
        crate::plan::AgentRef::from_tab("owner"),
    );
    for id in ["first", "second"] {
        let mut task = load_audit::task(id, crate::plan::TaskStatus::Done, None);
        task.lane.branch = Some(id.into());
        plan.tasks.push(task);
    }
    let database = fixtures::database(root.path(), &plan);
    fixtures::audit(&database);
    database.connection.lock().execute_batch("CREATE TRIGGER fail_second BEFORE UPDATE ON plan_tasks WHEN NEW.task_id='second' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(database
        .settle_lane_history_rows(&plan.id, 20, |reads| reads
            .iter()
            .map(|read| (
                read.task_id.clone(),
                TaskLaneUpdate {
                    cleanup_status: Some(Some("done".into())),
                    ..Default::default()
                }
            ))
            .collect())
        .is_err());
    assert!(fixtures::audit_counts(&database).is_empty());
    assert_eq!(
        database.load_plan(&plan.id).unwrap().unwrap().tasks,
        plan.tasks
    );
}

#[test]
fn concurrent_transcript_reservations_do_not_duplicate_a_turn() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixtures::fixture(root.path(), 0, 0);
    let database = Arc::new(fixtures::database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let database = database.clone();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                database
                    .edit_lane_turn_rows(&id, "task_0000", "current", 20, |stats| {
                        if stats
                            .iter()
                            .any(|stats| stats.transcript_turn_key.as_deref() == Some("same"))
                        {
                            return Ok(None);
                        }
                        stats.push(LaneTurnChangeStats {
                            turn: 1,
                            captured_at: 20,
                            transcript_turn_key: Some("same".into()),
                            ..Default::default()
                        });
                        Ok(Some(()))
                    })
                    .unwrap()
                    .0
            })
        })
        .collect();
    let count = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap())
        .count();
    assert_eq!(count, 1);
    let stored = database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(
        stored.tasks[0].execution_attempts[0].lane_turn_stats.len(),
        1
    );
}
