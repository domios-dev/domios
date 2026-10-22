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
