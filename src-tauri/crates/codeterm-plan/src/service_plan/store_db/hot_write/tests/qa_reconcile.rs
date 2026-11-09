use super::*;
use crate::plan::{QaState, ReviewStage};

#[test]
fn qa_and_reconcile_writes_stay_bounded_without_plan_loads() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixture(root.path(), strides, attempts);
        plan.tasks[0].status = TaskStatus::InReview;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].lane.tab_id = Some("worker".into());
        plan.tasks[2].assigned_tab_id = Some("gone".into());
        plan.tasks[2].lane.tab_id = Some("gone".into());
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let mut work = Vec::new();
        store
            .enqueue_qa(
                &plan.id,
                "task_0000",
                "queue",
                AgentRef::from_tab("owner"),
                30,
            )
            .unwrap();
        work.push(take_work());
        assert!(store
            .claim_next_qa(
                &plan.id,
                "claim",
                AgentRef::from_tab("owner"),
                Some("qa"),
                31
            )
            .unwrap()
            .is_some());
        work.push(take_work());
        store
            .pass_qa(
                &plan.id,
                "task_0000",
                "pass",
                AgentRef::from_tab("owner"),
                32,
            )
            .unwrap();
        work.push(take_work());
        assert!(store
            .flag_tab_gone(&plan.id, "task_0002", "gone", 33)
            .unwrap());
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert!(receipt[0] <= 2 && receipt[1] <= 2 && receipt[2] <= 1 && receipt[3] <= 1);
            assert_eq!(receipt[4], 0);
            assert!(receipt[5] < 64 * 1024 && receipt[6] < 64 * 1024);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} qa_reconcile_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        assert_eq!(
            store
                .database
                .read_task_core(&plan.id, "task_0000")
                .unwrap()
                .status,
            TaskStatus::Done
        );
        assert!(store
            .database
            .read_task_core(&plan.id, "task_0002")
            .unwrap()
            .assigned_tab_id
            .is_none());
    }
}

#[test]
fn task_batch_validation_failure_rolls_back_earlier_rows() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path(), 0, 0);
    let db = database(root.path(), &plan);
    let before = db.read_task_core(&plan.id, "task_0000").unwrap();
    let result: Result<(Vec<()>, CommittedRows), PlanError> = db.with_task_rows_batch(
        &plan.id,
        &["task_0000".into(), "task_0001".into()],
        TaskHistory::Core,
        30,
        |index, edit| {
            edit.task.speed = Some("staged".into());
            if index == 1 {
                return Err(PlanError::BadMode);
            }
            Ok(Some(()))
        },
    );
    assert!(matches!(result, Err(PlanError::BadMode)));
    assert_eq!(before, db.read_task_core(&plan.id, "task_0000").unwrap());
}

#[test]
fn concurrent_qa_claims_enforce_capacity_in_the_transaction() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path(), 0, 0);
    for task in &mut plan.tasks[..2] {
        task.status = TaskStatus::InReview;
        task.review_stage = ReviewStage::Qa;
        task.qa_state = Some(QaState::Queued);
    }
    drop(database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|_| {
            let db = PlanDatabase::open(root.path().join("plans.db")).unwrap();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.claim_next_qa_rows(&id, 1, 30, |edit| {
                    edit.task.qa_state = Some(QaState::Running);
                    Ok(())
                })
                .unwrap()
                .0
                .is_some()
            })
        })
        .collect();
    let accepted = writers
        .into_iter()
        .map(|writer| writer.join().unwrap())
        .filter(|accepted| *accepted)
        .count();
    assert_eq!(accepted, 1);
    let db = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    let plan = db.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(
        plan.tasks
            .iter()
            .filter(|task| task.qa_state == Some(QaState::Running))
            .count(),
        1
    );
}
