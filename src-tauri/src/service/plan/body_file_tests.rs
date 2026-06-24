use super::*;
use crate::plan::{OrchestratorRef, Plan};

#[test]
fn section_and_task_bodies_roundtrip_through_markdown_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "External".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .add_section(&plan.id, "Spec", "section\nbody", None, &[], None, None, 2)
        .unwrap();
    store
        .add_task(
            &plan.id,
            NewTask {
                name: "Implement".into(),
                kind: "implement".into(),
                body: "task\nbody".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();

    let section_path = dir
        .path()
        .join(format!(".codeterm/plans/{}/sections/spec.md", plan.id));
    let task_path = dir
        .path()
        .join(format!(".codeterm/plans/{}/tasks/implement.md", plan.id));
    let json_path = dir
        .path()
        .join(".codeterm/plans")
        .join(format!("{}.json", plan.id));
    let raw_json = std::fs::read_to_string(&json_path).unwrap();

    assert_eq!(
        std::fs::read_to_string(&section_path).unwrap(),
        "section\nbody"
    );
    assert_eq!(std::fs::read_to_string(&task_path).unwrap(), "task\nbody");
    assert!(!raw_json.contains("section\\nbody"));
    assert!(!raw_json.contains("task\\nbody"));
    let index_path = dir
        .path()
        .join(format!(".codeterm/plans/{}/INDEX.md", plan.id));
    let index = std::fs::read_to_string(index_path).unwrap();
    assert!(index.contains("# External"));
    assert!(index.contains("- [spec (Spec)](sections/spec.md)"));
    assert!(index.contains("- [implement (Implement)](tasks/implement.md)"));

    store.reload_from_disk(&plan.id).unwrap();
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.sections[0].body, "section\nbody");
    assert_eq!(loaded.tasks[0].body, "task\nbody");
}
