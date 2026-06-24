use super::*;
use crate::plan::{OrchestratorRef, Plan};
use std::path::PathBuf;

fn store_with_plan() -> (PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::mem::forget(dir);
    let store = PlanStore::new_for_test(&root);
    let mut plan = Plan::new(
        "Batch".into(),
        root.display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_batch".into();
    store.save(&mut plan, 1).unwrap();
    (store, plan.id)
}

fn store_with_plan_root() -> (PathBuf, PlanStore, String) {
    let root = tempfile::tempdir().unwrap().keep();
    let store = PlanStore::new_for_test(&root);
    let mut plan = Plan::new(
        "Batch".into(),
        root.display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_batch".into();
    store.save(&mut plan, 1).unwrap();
    (root, store, plan.id)
}

#[test]
fn batch_resolves_refs_and_writes_in_one_plan_mutation() {
    let (store, plan_id) = store_with_plan();
    let result = store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: Some("k1".into()),
                ops: vec![
                    BatchOp::UpsertTask {
                        client_id: Some("t".into()),
                        handle: "handoff-cli".into(),
                        name: Some("Handoff CLI".into()),
                        kind: None,
                        sections: vec![],
                        status: Some("in_progress".into()),
                        body: None,
                        model: None,
                        why: None,
                        priority: None,
                        group: None,
                    },
                    BatchOp::AddMilestone {
                        client_id: Some("m".into()),
                        task: "$t".into(),
                        kind: "decision".into(),
                        text: "ship batch first".into(),
                        from: None,
                        key: None,
                        links: vec![],
                    },
                    BatchOp::AddElement {
                        client_id: Some("e".into()),
                        kind: "note".into(),
                        content: serde_json::json!({"text":"x"}),
                        link: Some("$t".into()),
                        links: vec![],
                        x: None,
                        y: None,
                    },
                ],
            },
            2,
        )
        .unwrap();
    assert!(result.ids.contains_key("t"));
    assert!(result.ids.contains_key("m"));
    assert!(result.ids.contains_key("e"));
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].milestones.len(), 1);
    assert_eq!(plan.elements[0].links, vec![plan.tasks[0].id.clone()]);
}

#[test]
fn batch_is_atomic_when_later_op_fails() {
    let (store, plan_id) = store_with_plan();
    let err = store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![
                    BatchOp::UpsertTask {
                        client_id: Some("t".into()),
                        handle: "created".into(),
                        name: None,
                        kind: None,
                        sections: vec![],
                        status: None,
                        body: None,
                        model: None,
                        why: None,
                        priority: None,
                        group: None,
                    },
                    BatchOp::SetTaskBody {
                        task: "missing".into(),
                        body: "nope".into(),
                    },
                ],
            },
            2,
        )
        .unwrap_err();
    assert!(matches!(err, PlanError::TaskNotFound));
    assert!(store.get(&plan_id).unwrap().tasks.is_empty());
}

#[test]
fn batch_handoff_sidecar_is_not_written_when_later_op_fails() {
    let (root, store, plan_id) = store_with_plan_root();
    let err = store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![
                    BatchOp::AddHandoff {
                        client_id: Some("h".into()),
                        mode: None,
                        content: serde_json::json!({"goal":"rollback"}),
                        body: "must not remain".into(),
                        task: None,
                        from: None,
                        to: None,
                    },
                    BatchOp::SetTaskBody {
                        task: "missing".into(),
                        body: "fail".into(),
                    },
                ],
            },
            2,
        )
        .unwrap_err();
    assert!(matches!(err, PlanError::TaskNotFound));
    assert!(store.get(&plan_id).unwrap().elements.is_empty());
    let handoffs_dir = root
        .join(".codeterm")
        .join("plans")
        .join(&plan_id)
        .join("handoffs");
    let leftover = handoffs_dir
        .read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    assert!(!leftover);
}

#[test]
fn batch_spec_side_effects_are_not_written_when_later_op_fails() {
    let (root, store, plan_id) = store_with_plan_root();
    let err = store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![
                    BatchOp::UpsertSpec {
                        client_id: Some("sp".into()),
                        spec_id: None,
                        title: "Spec".into(),
                    },
                    BatchOp::UpsertSpecSection {
                        client_id: Some("ss".into()),
                        spec: "$sp".into(),
                        section_id: None,
                        title: "Req".into(),
                        body: Some("must not remain".into()),
                    },
                    BatchOp::SetTaskBody {
                        task: "missing".into(),
                        body: "fail".into(),
                    },
                ],
            },
            2,
        )
        .unwrap_err();
    assert!(matches!(err, PlanError::TaskNotFound));
    assert!(store.list_spec_index().is_empty());
    let specs_dir = root.join(".codeterm").join("specs");
    let leftover = specs_dir
        .read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    assert!(!leftover);
}

#[test]
fn batch_create_plan_from_spec_is_not_written_when_later_op_fails() {
    let (root, store, plan_id) = store_with_plan_root();
    let before = root
        .join(".codeterm")
        .join("plans")
        .read_dir()
        .unwrap()
        .count();
    let err = store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![
                    BatchOp::UpsertSpec {
                        client_id: Some("sp".into()),
                        spec_id: None,
                        title: "Spec".into(),
                    },
                    BatchOp::CreatePlanFromSpec {
                        client_id: Some("derived".into()),
                        spec: "$sp".into(),
                        title: "Derived".into(),
                    },
                    BatchOp::SetTaskBody {
                        task: "missing".into(),
                        body: "fail".into(),
                    },
                ],
            },
            2,
        )
        .unwrap_err();
    assert!(matches!(err, PlanError::TaskNotFound));
    assert_eq!(
        root.join(".codeterm")
            .join("plans")
            .read_dir()
            .unwrap()
            .count(),
        before
    );
    assert_eq!(store.list_index().len(), 1);
}

#[test]
fn batch_ingests_markdown_sections_and_persists_body() {
    let (store, plan_id) = store_with_plan();
    let result = store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![BatchOp::IngestSections {
                    body: "## Backend\nPersist section names and bodies.\n\n## UI\nRender them."
                        .into(),
                }],
            },
            2,
        )
        .unwrap();

    let backend_id = result.ids.get("backend").unwrap();
    let plan = store.get(&plan_id).unwrap();
    let backend = plan.sections.iter().find(|s| &s.id == backend_id).unwrap();
    assert_eq!(backend.title, "Backend");
    assert_eq!(backend.body, "Persist section names and bodies.");
    assert!(plan.sections.iter().any(|s| s.title == "UI"));
}

#[test]
fn batch_upsert_and_milestone_key_are_idempotent() {
    let (store, plan_id) = store_with_plan();
    let request = BatchRequest {
        key: Some("stable".into()),
        ops: vec![
            BatchOp::UpsertTask {
                client_id: Some("t".into()),
                handle: "same".into(),
                name: Some("Same".into()),
                kind: None,
                sections: vec![],
                status: None,
                body: None,
                model: None,
                why: None,
                priority: None,
                group: None,
            },
            BatchOp::AddMilestone {
                client_id: Some("m".into()),
                task: "$t".into(),
                kind: "testing".into(),
                text: "ran".into(),
                from: None,
                key: None,
                links: vec![],
            },
        ],
    };
    let first = store.apply_batch(&plan_id, request.clone(), 2).unwrap();
    let second = store.apply_batch(&plan_id, request, 3).unwrap();
    assert_eq!(first.task_ids, second.task_ids);
    assert_eq!(first.milestone_ids, second.milestone_ids);
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].milestones.len(), 1);
}

#[test]
fn batch_set_task_sections_links_existing_task() {
    let (store, plan_id) = store_with_plan();
    store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![BatchOp::UpsertTask {
                    client_id: None,
                    handle: "existing".into(),
                    name: None,
                    kind: None,
                    sections: vec![],
                    status: None,
                    body: None,
                    model: None,
                    why: None,
                    priority: None,
                    group: None,
                }],
            },
            2,
        )
        .unwrap();
    store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![BatchOp::SetTaskSections {
                    client_id: None,
                    task: "existing".into(),
                    sections: vec!["sec_a".into()],
                }],
            },
            3,
        )
        .unwrap();
    assert_eq!(store.get(&plan_id).unwrap().tasks[0].section_ids, ["sec_a"]);
}

#[test]
fn batch_set_task_sections_is_atomic_when_later_op_fails() {
    let (store, plan_id) = store_with_plan();
    store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![BatchOp::UpsertTask {
                    client_id: None,
                    handle: "existing".into(),
                    name: None,
                    kind: None,
                    sections: vec![],
                    status: None,
                    body: None,
                    model: None,
                    why: None,
                    priority: None,
                    group: None,
                }],
            },
            2,
        )
        .unwrap();
    let err = store
        .apply_batch(
            &plan_id,
            BatchRequest {
                key: None,
                ops: vec![
                    BatchOp::SetTaskSections {
                        client_id: None,
                        task: "existing".into(),
                        sections: vec!["sec_a".into()],
                    },
                    BatchOp::SetTaskBody {
                        task: "missing".into(),
                        body: "fail".into(),
                    },
                ],
            },
            3,
        )
        .unwrap_err();
    assert!(matches!(err, PlanError::TaskNotFound));
    assert!(store.get(&plan_id).unwrap().tasks[0].section_ids.is_empty());
}
