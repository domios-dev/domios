use super::*;

#[test]
fn qa_waves_group_compatible_tasks_by_shared_group() {
    let (_dir, store, plan_id) = setup();
    store
        .with_plan(&plan_id, 1, |p| {
            p.task_groups = vec![crate::plan::TaskGroup {
                key: "phase-1".into(),
                label: "Phase 1".into(),
                color: None,
            }];
            Ok(())
        })
        .unwrap();
    let a = store
        .add_task(
            &plan_id,
            NewTask {
                name: "a".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("Oracle: x".into()),
                group: Some("phase-1".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let b = store
        .add_task(
            &plan_id,
            NewTask {
                name: "b".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("Oracle: y".into()),
                group: Some("phase-1".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    for task_id in [&a, &b] {
        dispatch(&store, &plan_id, task_id, &format!("tab_{task_id}"), 3).unwrap();
        set_status(&store, &plan_id, task_id, "in_review", 4).unwrap();
        store
            .enqueue_qa(&plan_id, task_id, "ready", AgentRef::from_tab("orch"), 5)
            .unwrap();
    }
    let waves = store.qa_waves(&plan_id).unwrap();
    assert_eq!(waves.len(), 1);
    assert_eq!(waves[0].group_key, "phase-1");
    assert_eq!(waves[0].task_ids.len(), 2);
    assert!(waves[0].task_ids.contains(&a));
    assert!(waves[0].task_ids.contains(&b));
}
