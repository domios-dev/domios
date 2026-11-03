use super::*;
use crate::plan::{OrchestratorRef, Plan};

fn store_with_section() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Sections".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_sections".into();
    store.save(&mut plan, 1).unwrap();
    let parent = store
        .add_section(&plan.id, "Parent", "Parent body", None, &[], None, None, 2)
        .unwrap();
    let child = store
        .add_section(
            &plan.id,
            "Child",
            "Child body",
            Some(&parent),
            &[],
            None,
            None,
            3,
        )
        .unwrap();
    (dir, store, plan.id, child)
}

#[test]
fn show_section_finds_nested_section() {
    let (_dir, store, plan_id, child) = store_with_section();

    let details = store.show_section(&plan_id, &child).unwrap();

    assert_eq!(details.section.id, child);
    assert_eq!(details.title, "Child");
    assert!(details.markdown.contains("# Child"));
    assert!(details.markdown.contains("Child body"));
}

#[test]
fn show_section_finds_by_handle() {
    let (_dir, store, plan_id, child) = store_with_section();
    store
        .update_section(
            &plan_id,
            &child,
            None,
            None,
            &[],
            None,
            Some("child-handle"),
            4,
        )
        .unwrap();

    let details = store.show_section(&plan_id, "child-handle").unwrap();
    assert_eq!(details.section.id, child);
}

#[test]
fn show_section_missing_is_not_found() {
    let (_dir, store, plan_id, _child) = store_with_section();

    assert!(matches!(
        store.show_section(&plan_id, "sec_missing"),
        Err(PlanError::NotFound)
    ));
}
