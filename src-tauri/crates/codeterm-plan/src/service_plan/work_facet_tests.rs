use super::batch::{BatchOp, BatchRequest};
use super::{NewTask, PlanError, PlanStore};
use crate::plan::{OrchestratorRef, Plan, TagFacet};
use codeterm_types::plan::work_facet::{WorkFacetViolation, WORK_VALUES};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn facet(key: &str) -> TagFacet {
    TagFacet {
        key: key.into(),
        label: None,
        color_lane: false,
        values: vec![],
    }
}

fn new_task(tags: &[&str]) -> NewTask {
    NewTask {
        name: "Drafted task".into(),
        kind: "planning".into(),
        tags: tags.iter().map(|tag| (*tag).into()).collect(),
        ..Default::default()
    }
}

fn setup(facets: Vec<TagFacet>) -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Work facets".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tag_facets = facets;
    store.save(&mut plan, 1).unwrap();
    (dir, store, plan.id)
}

#[test]
fn work_facet_add_persists_each_allowed_value_and_unrelated_tags() {
    let (_dir, store, id) = setup(vec![facet("area"), facet("work")]);
    for value in WORK_VALUES {
        let tag = format!("work:{value}");
        let tags = ["area:plan", "topic", "work", tag.as_str(), "workflow:review"];
        let task_id = store
            .add_task_with_external_refs(&id, new_task(&tags), &[], 2)
            .unwrap();
        store.reload_from_disk(&id).unwrap();
        let plan = store.get(&id).unwrap();
        let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
        assert_eq!(task.tags, tags);
        assert_eq!(plan.tag_facets.len(), 2);
    }
    assert_eq!(store.get(&id).unwrap().tasks.len(), WORK_VALUES.len());
}

#[test]
fn work_facet_add_rejects_invalid_drafts_before_persistence() {
    let (_dir, store, id) = setup(vec![facet("work")]);
    store.add_task(&id, new_task(&["work:docs"]), 2).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let observed = writes.clone();
    store.set_persistence_hook_for_test(Some(Arc::new(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
    })));
    let cases = [
        vec![],
        vec!["area:frontend"],
        vec!["work"],
        vec!["work:frontend", "work:frontend"],
        vec!["work:frontend", "work:backend"],
        vec!["work:frontend", "work:"],
        vec!["work:other"],
        vec!["work:Frontend"],
        vec!["work:"],
        vec!["work:frontend:backend"],
        vec!["work: frontend"],
        vec!["work:frontend "],
        vec![" work:frontend"],
        vec!["work :frontend"],
    ];
    for tags in cases {
        let before = serde_json::to_value(store.get(&id).unwrap()).unwrap();
        let error = store.add_task(&id, new_task(&tags), 3).unwrap_err();
        assert!(
            matches!(error, PlanError::WorkFacetViolation(_)),
            "{tags:?}: {error}"
        );
        assert_eq!(error.to_json()["type"], "work_facet_violation");
        assert_eq!(writes.load(Ordering::SeqCst), 0, "{tags:?}");
        assert_eq!(serde_json::to_value(store.get(&id).unwrap()).unwrap(), before);
        store.reload_from_disk(&id).unwrap();
        assert_eq!(serde_json::to_value(store.get(&id).unwrap()).unwrap(), before);
    }
}

#[test]
fn work_facet_undeclared_plans_keep_existing_tag_behavior() {
    let (_dir, store, id) = setup(vec![facet("area")]);
    for tags in [
        vec![],
        vec!["work:", "work:custom", "work:custom", "area:ui", "topic"],
    ] {
        let task_id = store.add_task(&id, new_task(&tags), 2).unwrap();
        store.reload_from_disk(&id).unwrap();
        let plan = store.get(&id).unwrap();
        let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
        assert_eq!(task.tags, tags);
    }
}

#[test]
fn work_facet_declaration_does_not_backfill_or_block_legacy_tasks() {
    let (_dir, store, id) = setup(vec![]);
    let task_id = store.add_task(&id, new_task(&["topic"]), 2).unwrap();
    store.set_tag_facets(&id, vec![facet("work")], 3).unwrap();
    store.add_task(&id, new_task(&["work:qa"]), 4).unwrap();
    store.reload_from_disk(&id).unwrap();
    let plan = store.get(&id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.tags, ["topic"]);
}

#[test]
fn work_facet_batch_cannot_bypass_task_add_contract() {
    let (_dir, store, id) = setup(vec![facet("work")]);
    let before = serde_json::to_value(store.get(&id).unwrap()).unwrap();
    let error = store
        .apply_batch(
            &id,
            BatchRequest {
                key: None,
                ops: vec![BatchOp::from(new_task(&[]))],
            },
            2,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        PlanError::WorkFacetViolation(WorkFacetViolation::Missing)
    ));
    store.reload_from_disk(&id).unwrap();
    assert_eq!(serde_json::to_value(store.get(&id).unwrap()).unwrap(), before);
}
