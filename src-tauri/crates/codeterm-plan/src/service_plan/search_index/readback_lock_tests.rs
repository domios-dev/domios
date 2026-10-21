use super::*;

#[test]
fn related_traversal_is_dangling_safe() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Related".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let section_id = store
        .add_section(&plan.id, "Section", "", None, &[], None, None, 2)
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Task".into(),
                kind: "fix".into(),
                section_ids: vec![section_id.clone(), "deleted_section".into()],
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .fill_task_lane_if_empty(&plan.id, &task_id, Some("tab-1"), None, None, 3)
        .unwrap();
    store
        .update_task(
            &plan.id,
            &task_id,
            None,
            None,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            4,
        )
        .unwrap();

    let groups = store.related_units(&task_id);
    let section_group = groups
        .iter()
        .find(|group| group.edge_type == "task_sections")
        .unwrap();
    assert!(section_group
        .units
        .iter()
        .any(|unit| unit.id == section_id && !unit.removed));
    assert!(section_group
        .units
        .iter()
        .any(|unit| unit.id == "deleted_section" && unit.removed));
    assert_eq!(
        store.get(&plan.id).unwrap().tasks[0].status,
        TaskStatus::InProgress
    );
}

#[test]
fn locked_plan_update_succeeds_after_verified_commit() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Plan update lock".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    while_plan_index_locked(root.path(), || {
        store.update_plan(&plan.id, Some("Updated under contention"), None, None, 2)
    })
    .unwrap();
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.title, "Updated under contention");
}

#[test]
fn locked_planner_waiver_succeeds_after_verified_readback() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Planner waiver lock".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let waiver = Some(PlannerRef::Waived {
        note: "solo test".into(),
    });

    while_plan_index_locked(root.path(), || {
        store.set_planner_ref(&plan.id, waiver.clone(), 2)
    })
    .unwrap();
    assert_eq!(store.get(&plan.id).unwrap().planner, waiver);
}

#[test]
fn parallel_task_updates_under_locked_index_do_not_false_fail() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    let (root, store) = store_and_root();
    let store = Arc::new(store);
    let plan = store
        .create_plan(
            "Parallel updates".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Parallel".into(),
                kind: "fix".into(),
                body: "before".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let locker = hold_plan_index_lock(root.path());
    let failures = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for idx in 0..8 {
        let store = Arc::clone(&store);
        let plan_id = plan.id.clone();
        let task_id = task_id.clone();
        let failures = Arc::clone(&failures);
        handles.push(thread::spawn(move || {
            let body = format!("after-{idx}");
            if store
                .update_task(
                    &plan_id,
                    &task_id,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(&body),
                    None,
                    None,
                    None,
                    None,
                    None,
                    &[],
                    None,
                    None,
                    3 + idx as u64,
                )
                .is_err()
            {
                failures.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(failures.load(Ordering::Relaxed), 0);
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert!(task.body.starts_with("after-"));
    drop(locker);
}
