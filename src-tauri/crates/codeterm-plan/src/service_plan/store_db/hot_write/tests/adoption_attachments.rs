use super::*;
use crate::plan::InterruptionCheckpoint;
use crate::service_plan::interruption::checkpoint_digest;
use crate::service_plan::task_assignment::AssignmentRuntimeBinding;

fn adoptable(root: &std::path::Path, strides: usize, attempts: usize) -> Plan {
    let mut plan = fixture(root, strides, attempts);
    let task = &mut plan.tasks[0];
    task.milestones.push(milestone("attachment-milestone"));
    task.interruption.owner_epoch = 1;
    task.interruption.handoff_required = true;
    task.interruption.checkpoint = Some(InterruptionCheckpoint {
        task_id: task.id.clone(),
        attempt_id: "current".into(),
        owner_epoch: 1,
        diff_artifact: "diff".into(),
        diff_sha256: checkpoint_digest("diff"),
        base_head: "base".into(),
        ..Default::default()
    });
    task.execution_attempts[0].closed_at = Some(2);
    plan
}

fn binding() -> AssignmentRuntimeBinding {
    AssignmentRuntimeBinding {
        session_id: "session".into(),
        session_binding_nonce: "nonce".into(),
        provider: None,
        model: None,
        session_model: None,
        reasoning_effort: None,
    }
}

#[test]
fn task_attachment_and_checkpoint_adoption_are_bounded_and_undo_preserves_attachment() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let plan = adoptable(root.path(), strides, attempts);
        drop(database(root.path(), &plan));
        let source = root.path().join("attachment.txt");
        std::fs::write(&source, "attachment").unwrap();
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let attachment = store
            .attach_to_task(&plan.id, "task_0000", &source, 30)
            .unwrap();
        let mut work = vec![take_work()];
        store
            .attach_to_milestone(&plan.id, "task_0000", "attachment-milestone", &source, 30)
            .unwrap();
        work.push(take_work());
        let receipt = store
            .adopt_interrupted_task_with_undo(
                &plan.id,
                "task_0000",
                &checkpoint_digest("diff"),
                "base",
                "replacement",
                &binding(),
                31,
            )
            .unwrap();
        assert_eq!(receipt.outcome().0, 2);
        work.push(take_work());
        store.undo_checkpoint_adoption(receipt, 32).unwrap();
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert!(receipt[0] <= 2 && receipt[1] == 1);
            assert!(receipt[2] <= 1);
            assert!(receipt[3] <= 1 && receipt[4] == 0);
            assert!(receipt[5] < 64 * 1024 && receipt[6] < 64 * 1024);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} adoption_attachment_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        let task = &store.database.load_plan(&plan.id).unwrap().unwrap().tasks[0];
        assert_eq!(task.execution_attempts.len(), 1);
        assert_eq!(task.interruption.owner_epoch, 1);
        assert!(task.interruption.handoff_required);
        assert_eq!(task.attachments, vec![attachment]);
    }
}

#[test]
fn concurrent_checkpoint_adoptions_accept_one_owner() {
    let root = tempfile::tempdir().unwrap();
    let plan = adoptable(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|index| {
            let store = PlanStore::open(root.path().join("plans.db")).unwrap();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .adopt_interrupted_task_with_undo(
                        &id,
                        "task_0000",
                        &checkpoint_digest("diff"),
                        "base",
                        &format!("worker-{index}"),
                        &binding(),
                        30,
                    )
                    .is_ok()
            })
        })
        .collect();
    assert_eq!(
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .filter(|accepted| *accepted)
            .count(),
        1
    );
}
