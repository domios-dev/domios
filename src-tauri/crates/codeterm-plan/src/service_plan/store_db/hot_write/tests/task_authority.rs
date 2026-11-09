use super::*;
use crate::plan::ExecutionIntent;
use crate::service_plan::task_assignment::AssignmentRuntimeBinding;

fn owned_plan(root: &std::path::Path, strides: usize, attempts: usize) -> Plan {
    let mut plan = fixture(root, strides, attempts);
    let task = &mut plan.tasks[0];
    task.status = TaskStatus::InProgress;
    task.assigned_tab_id = Some("worker".into());
    task.lane.tab_id = Some("worker".into());
    task.interruption.owner_epoch = 1;
    task.interruption.owner_tab_id = Some("worker".into());
    task.interruption.owner_session_id = Some("session".into());
    task.execution_attempts[0].interruption.owner_epoch = 1;
    plan
}

#[test]
fn interruption_recovery_and_assignment_have_bounded_work() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let plan = owned_plan(root.path(), strides, attempts);
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let mut work = Vec::new();
        store
            .record_attempt_checkout(
                &plan.id,
                "task_0000",
                "worker",
                "current",
                "checkout".into(),
                30,
            )
            .unwrap();
        work.push(take_work());
        store
            .record_dispatch_policy(
                &plan.id,
                "task_0000",
                "current",
                ExecutionIntent::Unspecified,
                None,
                31,
            )
            .unwrap();
        work.push(take_work());
        assert!(store
            .begin_provider_exhaustion_pause(
                &plan.id,
                "task_0000",
                "current",
                "worker",
                "foreign",
                1,
                32
            )
            .is_err());
        store
            .begin_provider_exhaustion_pause(
                &plan.id,
                "task_0000",
                "current",
                "worker",
                "session",
                1,
                32,
            )
            .unwrap();
        work.push(take_work());
        store
            .resume_from_provider_exhaustion(&plan.id, "task_0000", "worker", "session", 1, 33)
            .unwrap();
        work.push(take_work());
        store
            .begin_task_interruption(
                &plan.id,
                "task_0000",
                "current",
                "worker",
                "session",
                1,
                "interrupt",
                34,
            )
            .unwrap();
        work.push(take_work());
        store
            .mark_interruption_checkpoint_failed(&plan.id, "task_0000", "current", 1, 35)
            .unwrap();
        work.push(take_work());
        assert!(
            store
                .retire_stale_task_owner(
                    &plan.id,
                    "task_0000",
                    Some(TaskStatus::Pending),
                    |_| false,
                    36
                )
                .unwrap()
                .applied
        );
        work.push(take_work());
        store
            .assign_task_with_runtime_binding(
                &plan.id,
                "task_0000",
                "replacement",
                None,
                Some("worker"),
                Some(&AssignmentRuntimeBinding {
                    session_id: "new-session".into(),
                    session_binding_nonce: "new-nonce".into(),
                    provider: None,
                    model: None,
                    session_model: None,
                    reasoning_effort: None,
                }),
                None,
                37,
            )
            .unwrap();
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert_eq!(receipt[0], 1);
            assert_eq!(receipt[1], 1);
            assert!(receipt[2] <= 1);
            assert_eq!(&receipt[3..5], &[0, 0]);
            assert!(receipt[5] < 64 * 1024 && receipt[6] < 64 * 1024);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} authority_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        let task = store
            .database
            .read_task_core(&plan.id, "task_0000")
            .unwrap();
        assert_eq!(task.assigned_tab_id.as_deref(), Some("replacement"));
        assert_eq!(task.interruption.owner_epoch, 3);
    }
}

#[test]
fn concurrent_stale_owner_retirement_advances_epoch_once() {
    let root = tempfile::tempdir().unwrap();
    let plan = owned_plan(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|_| {
            let store = PlanStore::open(root.path().join("plans.db")).unwrap();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .retire_stale_task_owner(
                        &id,
                        "task_0000",
                        Some(TaskStatus::Pending),
                        |_| false,
                        30,
                    )
                    .unwrap()
                    .applied
            })
        })
        .collect();
    assert_eq!(
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .filter(|applied| *applied)
            .count(),
        1
    );
    let db = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    let plan = db.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(plan.tasks[0].interruption.owner_epoch, 2);
    assert!(plan.tasks[0].execution_attempts[0].retired_at.is_some());
}
