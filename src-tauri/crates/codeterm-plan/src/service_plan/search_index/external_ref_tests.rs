use super::*;
use crate::plan::OrchestratorRef;
use crate::service_plan::{NewTask, PlanStore};

#[test]
fn task_unit_includes_external_refs() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan(
            "External reference index".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    store
        .add_task(
            &plan.id,
            NewTask {
                name: "Owning task".into(),
                acceptance_criteria: vec!["Acceptance marker".into()],
                body: "Body marker".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&plan.id).unwrap();
    plan.tasks[0].external_refs = vec![
        crate::service_plan::external_refs::canonicalize_external_ref(
            "rollacode/codeterm-releases#17",
        )
        .unwrap(),
    ];

    let unit = task_unit(&plan, &plan.tasks[0], "scope", "active");

    assert!(unit.body.contains("Acceptance marker"));
    assert!(unit
        .body
        .contains("https://github.com/rollacode/codeterm-releases/issues/17"));
}
