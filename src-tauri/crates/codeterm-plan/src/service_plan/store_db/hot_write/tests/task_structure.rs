use super::*;
use crate::service_plan::NewTask;

#[test]
fn task_structure_writes_are_bounded_without_whole_plan_loads() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let plan = fixture(root.path(), strides, attempts);
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        load_audit::start(&store);
        let mut work = Vec::new();
        let id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "new task".into(),
                    ..Default::default()
                },
                30,
            )
            .unwrap();
        work.push(take_work());
        store
            .update_task_dependencies(&plan.id, &id, vec!["task_0000".into()], 31)
            .unwrap();
        work.push(take_work());
        let children = store
            .add_child_tasks_atomic(
                &plan.id,
                &id,
                vec![NewTask {
                    name: "child".into(),
                    ..Default::default()
                }],
                32,
            )
            .unwrap();
        work.push(take_work());
        store.rm_task(&plan.id, &children[0], 33).unwrap();
        work.push(take_work());
        store.rm_task(&plan.id, &id, 34).unwrap();
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert!(receipt[0] <= 2 && receipt[1] == 1);
            assert_eq!(&receipt[2..5], &[0, 0, 0]);
            assert!(receipt[5] < 64 * 1024 && receipt[6] < 64 * 1024);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} structure_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        assert!(store.database.read_task_core(&plan.id, &id).is_err());
        assert_eq!(
            store
                .database
                .load_plan(&plan.id)
                .unwrap()
                .unwrap()
                .tasks
                .len(),
            TASKS
        );
    }
}

#[test]
fn concurrent_dependency_edges_cannot_commit_a_cycle() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|index| {
            let store = PlanStore::open(root.path().join("plans.db")).unwrap();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.update_task_dependencies(
                    &id,
                    &format!("task_{index:04}"),
                    vec![format!("task_{:04}", 1 - index)],
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
        .any(|result| matches!(result, Err(PlanError::DependencyCycle))));
}

#[test]
fn split_failure_rolls_back_all_children_and_removal_rechecks_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path(), 0, 0);
    drop(database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    assert!(store
        .add_child_tasks_atomic(
            &plan.id,
            "task_0000",
            vec![
                NewTask {
                    name: "first".into(),
                    ..Default::default()
                },
                NewTask {
                    name: "second".into(),
                    depends_on: vec!["missing".into()],
                    ..Default::default()
                },
            ],
            30
        )
        .is_err());
    assert_eq!(
        store
            .database
            .load_plan(&plan.id)
            .unwrap()
            .unwrap()
            .tasks
            .len(),
        TASKS
    );
    store
        .update_task_dependencies(&plan.id, "task_0001", vec!["task_0000".into()], 31)
        .unwrap();
    assert!(matches!(
        store.rm_task(&plan.id, "task_0000", 32),
        Err(PlanError::DependencyTaskReferenced { .. })
    ));
    assert!(store.database.read_task_core(&plan.id, "task_0000").is_ok());
}
