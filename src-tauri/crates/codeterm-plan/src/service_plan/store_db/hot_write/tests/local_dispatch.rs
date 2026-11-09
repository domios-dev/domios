use super::*;
use crate::plan::{PlanMode, TaskLaneMetadata};
use crate::service_plan::DispatchRollback;

#[test]
fn local_dispatch_ack_and_targeted_rollback_stay_bounded() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixture(root.path(), strides, attempts);
        plan.tasks[0].status = TaskStatus::Blocked;
        plan.tasks[0].status_label = Some("blocked reason".into());
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let mut work = Vec::new();
        let started = store
            .begin_task_dispatch_with_undo(
                &plan.id,
                "task_0000",
                "new-worker",
                "Worker",
                TaskLaneMetadata::default(),
                30,
            )
            .unwrap();
        work.push(take_work());
        store
            .finish_task_dispatch(
                &plan.id,
                "task_0000",
                "new-worker",
                &started.attempt_id,
                "session",
                "nonce",
                TaskLaneMetadata::default(),
                31,
            )
            .unwrap();
        work.push(take_work());
        store
            .acknowledge_task_attempt(&plan.id, "task_0000", "new-worker", &started.attempt_id, 32)
            .unwrap();
        work.push(take_work());
        assert_eq!(
            store
                .roll_back_task_dispatch_attempt(
                    &started.undo,
                    "new-worker",
                    &started.attempt_id,
                    33
                )
                .unwrap(),
            DispatchRollback::Restored
        );
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert!(
                receipt[0] == 1
                    && receipt[1] <= 3
                    && receipt[2] <= 2
                    && receipt[3] == 0
                    && receipt[4] <= 1
            );
            assert!(receipt[5] < 64 * 1024 && receipt[6] < 64 * 1024);
            assert_eq!(receipt[8], 0);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} local_dispatch_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        let persisted = store.database.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(persisted.mode, PlanMode::Planning);
        assert_eq!(persisted.tasks[0].status, TaskStatus::Blocked);
        assert_eq!(
            persisted.tasks[0].status_label.as_deref(),
            Some("blocked reason")
        );
        assert_eq!(persisted.strides.len(), strides + 1);
        assert!(persisted
            .strides
            .last()
            .unwrap()
            .text
            .starts_with("startup rolled back; task remains blocked: "));
        assert!(persisted.tasks[0].execution_attempts[0].closed_at.is_none());
        assert!(persisted.tasks[0].execution_attempts[1].closed_at.is_some());
        let rebuilt = store.database.persist_plan_rows(&persisted, None).unwrap();
        assert_eq!(rebuilt.strides, strides + 1);
        assert_eq!(
            store.database.load_plan(&plan.id).unwrap().unwrap(),
            persisted
        );
        assert_eq!(
            store
                .database
                .persist_plan_rows(&persisted, None)
                .unwrap()
                .strides,
            0
        );
    }
}

#[test]
fn concurrent_local_dispatch_observations_accept_one_owner() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    plan.tasks[0].execution_attempts.clear();
    drop(database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|index| {
            let store = PlanStore::open(root.path().join("plans.db")).unwrap();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.begin_task_dispatch_with_observation(
                    &id,
                    "task_0000",
                    &format!("worker-{index}"),
                    "Worker",
                    TaskLaneMetadata::default(),
                    None,
                    false,
                    30,
                )
            })
        })
        .collect();
    let results: Vec<_> = writers
        .into_iter()
        .map(|writer| writer.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(results
        .iter()
        .any(|result| matches!(result, Err(PlanError::DispatchInFlight { .. }))));
    let db = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    let plan = db.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(
        plan.tasks[0]
            .execution_attempts
            .iter()
            .filter(|attempt| attempt.closed_at.is_none())
            .count(),
        1
    );
}
