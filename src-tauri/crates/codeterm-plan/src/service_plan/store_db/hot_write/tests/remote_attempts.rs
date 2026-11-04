use super::*;
use crate::plan::{RemoteDispatchState, TaskExecutionReportSnapshot};

#[test]
fn remote_dispatch_transactions_are_bounded_and_peer_failures_roll_back() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixture(root.path(), strides, attempts);
        plan.tasks[0].execution_attempts[0].closed_at = Some(2);
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let mut work = Vec::new();
        store
            .begin_remote_dispatch_attempt(
                &plan.id,
                "task_0000",
                "remote",
                "peer",
                "Peer",
                "provider",
                "base",
                "destination",
                30,
            )
            .unwrap();
        work.push(take_work());
        let before = store
            .database
            .read_task_core(&plan.id, "task_0000")
            .unwrap();
        assert!(store
            .set_remote_dispatch_state(
                &plan.id,
                "task_0000",
                "remote",
                "foreign",
                RemoteDispatchState::PeerOffline,
                31
            )
            .is_err());
        assert_eq!(
            before,
            store
                .database
                .read_task_core(&plan.id, "task_0000")
                .unwrap()
        );
        store
            .record_remote_dispatch_lane(
                &plan.id,
                "task_0000",
                "remote",
                "peer",
                "remote-worker",
                "lane",
                "remote/path",
                "base",
                "provider",
                "nonce",
                32,
            )
            .unwrap();
        work.push(take_work());
        assert!(store
            .acknowledge_remote_dispatch(
                &plan.id,
                "task_0000",
                "remote",
                "peer",
                "remote-worker",
                33
            )
            .unwrap());
        work.push(take_work());
        let report = TaskExecutionReportSnapshot {
            id: "report".into(),
            reporter_tab_id: Some("remote-worker".into()),
            status: Some("in_review".into()),
            ..Default::default()
        };
        assert!(store
            .ingest_remote_dispatch_report(
                &plan.id,
                "task_0000",
                "remote",
                "peer",
                report.clone(),
                34
            )
            .unwrap());
        work.push(take_work());
        assert!(!store
            .ingest_remote_dispatch_report(&plan.id, "task_0000", "remote", "peer", report, 35)
            .unwrap());
        assert_eq!(&take_work()[..5], &[0; 5]);
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert_eq!(&receipt[..5], &[1, 1, 1, 0, 0]);
            assert!(receipt[5] < 32 * 1024 && receipt[6] < 32 * 1024);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} remote_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        let persisted = store.database.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(persisted.tasks[0].status, TaskStatus::InReview);
        assert_eq!(persisted.tasks[0].execution_attempts.len(), 2);
        assert_eq!(
            persisted.tasks[0].execution_attempts[1].remote_dispatch_state,
            Some(RemoteDispatchState::Reported)
        );
    }
}

#[test]
fn concurrent_remote_ack_and_timeout_commit_one_terminal_outcome() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    let attempt = &mut plan.tasks[0].execution_attempts[0];
    attempt.mesh_peer = Some("peer".into());
    attempt.remote_dispatch_state = Some(RemoteDispatchState::AwaitingWorkerAck);
    attempt.acknowledged_at = None;
    drop(database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|index| {
            let store = PlanStore::open(root.path().join("plans.db")).unwrap();
            let barrier = barrier.clone();
            let plan_id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                if index == 0 {
                    store
                        .acknowledge_remote_dispatch(
                            &plan_id,
                            "task_0000",
                            "current",
                            "peer",
                            "worker",
                            30,
                        )
                        .unwrap_or(false)
                } else {
                    store
                        .mark_remote_ack_timeout(
                            &plan_id,
                            "task_0000",
                            "current",
                            "peer",
                            "worker",
                            30,
                        )
                        .unwrap()
                }
            })
        })
        .collect();
    let outcomes: Vec<_> = writers
        .into_iter()
        .map(|writer| writer.join().unwrap())
        .collect();
    assert_eq!(outcomes.iter().filter(|accepted| **accepted).count(), 1);
    let database = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    let task = &database.load_plan(&plan.id).unwrap().unwrap().tasks[0];
    let attempt = &task.execution_attempts[0];
    assert!(matches!(
        (attempt.acknowledged_at, attempt.closed_at),
        (Some(30), None) | (None, Some(30))
    ));
}
