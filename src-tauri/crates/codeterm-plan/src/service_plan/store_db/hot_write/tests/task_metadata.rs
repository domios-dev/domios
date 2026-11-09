use super::*;
use crate::plan::TaskFiles;
use crate::service_plan::mutations::task_ops::AcceptanceCriteriaUpdate;
use crate::service_plan::TaskLaneUpdate;

#[test]
fn task_metadata_families_have_bounded_work_without_plan_loads() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let plan = fixture(root.path(), strides, attempts);
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let mut work = Vec::new();
        store
            .update_task_speed(&plan.id, "task_0000", Some("fast"), 30)
            .unwrap();
        work.push(take_work());
        store
            .update_task_quality(
                &plan.id,
                "task_0000",
                Some(AcceptanceCriteriaUpdate::Append(vec!["criterion".into()])),
                Some(Some("test")),
                None,
                None,
                31,
            )
            .unwrap();
        work.push(take_work());
        store
            .add_task_external_refs(
                &plan.id,
                "task_0000",
                &["https://github.com/example/project/issues/42".into()],
                32,
            )
            .unwrap();
        work.push(take_work());
        store
            .record_report_lane_truth(&plan.id, "task_0000", Some("branch"), None, 33)
            .unwrap();
        work.push(take_work());
        store
            .fill_task_lane_if_empty(&plan.id, "task_0000", Some("worker"), None, None, 34)
            .unwrap();
        work.push(take_work());
        store
            .update_task_lane(
                &plan.id,
                "task_0000",
                TaskLaneUpdate {
                    branch: Some(Some("replacement".into())),
                    ..Default::default()
                },
                35,
            )
            .unwrap();
        work.push(take_work());
        store
            .update_task_files(
                &plan.id,
                "task_0000",
                TaskFiles {
                    edit: vec!["owned.rs".into()],
                    ..Default::default()
                },
                36,
            )
            .unwrap();
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert_eq!(&receipt[..5], &[1, 1, 0, 0, 0]);
            assert!(receipt[5] < 32 * 1024 && receipt[6] < 32 * 1024);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} metadata_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        let task = store
            .database
            .read_task_core(&plan.id, "task_0000")
            .unwrap();
        assert_eq!(task.speed.as_deref(), Some("fast"));
        assert_eq!(task.acceptance_criteria, vec!["criterion"]);
        assert_eq!(task.external_refs.len(), 1);
        assert_eq!(task.lane.branch.as_deref(), Some("replacement"));
        assert_eq!(task.files.edit, vec!["owned.rs"]);
    }
}

#[test]
fn task_metadata_failure_rolls_back_and_concurrent_fields_survive() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    let before = store
        .database
        .read_task_core(&plan.id, "task_0000")
        .unwrap();
    let rejected: Result<(), PlanError> =
        store.with_task_rows(&plan.id, "task_0000", TaskHistory::Core, 30, |edit| {
            edit.task.speed = Some("rejected".into());
            Err(PlanError::BadMode)
        });
    assert!(matches!(rejected, Err(PlanError::BadMode)));
    assert_eq!(
        before,
        store
            .database
            .read_task_core(&plan.id, "task_0000")
            .unwrap()
    );
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
                        .update_task_speed(&plan_id, "task_0000", Some("accepted"), 31)
                        .unwrap();
                } else {
                    store
                        .update_task_quality(
                            &plan_id,
                            "task_0000",
                            None,
                            Some(Some("accepted")),
                            None,
                            None,
                            32,
                        )
                        .unwrap();
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    let task = store
        .database
        .read_task_core(&plan.id, "task_0000")
        .unwrap();
    assert_eq!(task.speed.as_deref(), Some("accepted"));
    assert_eq!(task.test_plan.as_deref(), Some("accepted"));
}
