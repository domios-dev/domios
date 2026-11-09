use super::*;
use crate::plan::{PlanEvidenceClass, ReviewCapacityClass};
use crate::service_plan::competition::TaskCompetitionUpdate;

#[test]
fn remaining_review_and_task_writes_are_bounded() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixture(root.path(), strides, attempts);
        plan.review_policy.credit = -5;
        plan.tasks[0].status = TaskStatus::InReview;
        plan.tasks[0].status_changed_at = 1;
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        store
            .update_task_competition(
                &plan.id,
                "task_0000",
                TaskCompetitionUpdate {
                    enabled: Some(true),
                    ..Default::default()
                },
                20,
            )
            .unwrap();
        let mut work = vec![take_work()];
        let credit = store
            .reconcile_review_credit(
                &plan.id,
                "observed backlog",
                AgentRef::from_tab("owner"),
                21,
            )
            .unwrap();
        assert_eq!(credit.reconciled_credit, -1);
        work.push(take_work());
        store
            .grant_review_capacity(
                &plan.id,
                "task_0000",
                ReviewCapacityClass::Review,
                AgentRef::from_tab("owner"),
                22,
            )
            .unwrap();
        work.push(take_work());
        store
            .grant_plan_evidence_reporter(
                &plan.id,
                "reviewer",
                "session",
                "nonce",
                PlanEvidenceClass::Review,
                AgentRef::from_tab("owner"),
                23,
            )
            .unwrap();
        work.push(take_work());
        assert!(store
            .escalate_stale_review(&plan.id, "task_0000", 5, AgentRef::from_tab("owner"), 24)
            .unwrap());
        work.push(take_work());
        assert!(!store
            .escalate_stale_review(&plan.id, "task_0000", 5, AgentRef::from_tab("owner"), 25)
            .unwrap());
        work.push(take_work());
        store
            .set_task_assignment("task_0002", Some("new-worker"), Some("worker"), 26)
            .unwrap();
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert!(receipt[0] <= 1 && receipt[2] <= 1 && receipt[3] <= 1 && receipt[4] <= 1);
            assert!(receipt[5] < 64 * 1024 && receipt[6] < 64 * 1024);
        }
        println!("fixture={SEED} strides={strides} attempts={attempts} remaining_work={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
    }
}

#[test]
fn review_provenance_and_its_stride_roll_back_together() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_grant_stride BEFORE INSERT ON plan_strides BEGIN SELECT RAISE(ABORT,'injected stride failure'); END;").unwrap();
    assert!(store
        .grant_review_capacity(
            &plan.id,
            "task_0000",
            ReviewCapacityClass::Review,
            AgentRef::from_tab("owner"),
            20
        )
        .is_err());
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
    assert!(stored.review_policy.capacity_grants.is_empty());
    assert!(stored.strides.is_empty());
}

#[test]
fn concurrent_review_provenance_grants_append_once() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|_| {
            let store = PlanStore::open(root.path().join("plans.db")).unwrap();
            let id = plan.id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .grant_review_capacity(
                        &id,
                        "task_0000",
                        ReviewCapacityClass::Review,
                        AgentRef::from_tab("owner"),
                        20,
                    )
                    .unwrap()
                    .created
            })
        })
        .collect();
    assert_eq!(
        writers
            .into_iter()
            .filter_map(|writer| writer.join().unwrap().then_some(()))
            .count(),
        1
    );
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    let stored = store.database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(stored.review_policy.capacity_grants.len(), 1);
    assert_eq!(stored.strides.len(), 1);
}
