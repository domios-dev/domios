use super::*;
use crate::plan::{AgentRef, MilestoneKind, OrchestratorRef, Plan, PlanStatus, TaskStatus};

#[test]
fn store_saves_loads_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "T".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();
    let path = dir
        .path()
        .join(".codeterm/plans")
        .join(format!("{}.json", p.id));
    assert!(path.exists());
    let loaded = store.get(&p.id).unwrap();
    assert_eq!(loaded.title, "T");
    assert!(store.list_index().iter().any(|e| e.plan_id == p.id));
    assert_eq!(loaded.updated_at, 1000);
}

fn store_with_task(kinds: &[&str]) -> (PlanStore, String, String) {
    let root = tempfile::tempdir().unwrap().keep();
    let store = PlanStore::new_for_test(&root);
    let mut p = Plan::new(
        "T".into(),
        root.display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    for k in kinds {
        p.milestone_kinds.push(MilestoneKind {
            key: k.to_string(),
            label: k.to_string(),
            color: None,
        });
    }
    store.save(&mut p, 1).unwrap();
    let tid = store
        .add_task(
            &p.id,
            NewTask {
                name: "n".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (store, p.id, tid)
}

#[test]
fn add_section_appends_and_bumps_updated_at() {
    let (store, id, _) = store_with_task(&["status_update"]);
    let sid = store.add_section(&id, "Spec", "body", None, 10).unwrap();
    let p = store.get(&id).unwrap();
    assert!(p.sections.iter().any(|s| s.id == sid && s.title == "Spec"));
    assert_eq!(p.updated_at, 10);
    let sub = store.add_section(&id, "Child", "", Some(&sid), 11).unwrap();
    let p = store.get(&id).unwrap();
    let parent = p.sections.iter().find(|s| s.id == sid).unwrap();
    assert!(parent.subsections.iter().any(|s| s.id == sub));
}

#[test]
fn add_task_supports_many_to_many_sections() {
    let (store, id, _) = store_with_task(&["status_update"]);
    let tid = store
        .add_task(
            &id,
            NewTask {
                name: "Dante".into(),
                kind: "implement".into(),
                section_ids: vec!["s1".into(), "s2".into()],
                ..Default::default()
            },
            20,
        )
        .unwrap();
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.section_ids, vec!["s1".to_string(), "s2".to_string()]);
    assert_eq!(p.updated_at, 20);
}

#[test]
fn close_task_sets_done_and_optional_milestone() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store
        .close_task(&id, &tid, Some("shipped"), AgentRef::from_pane("w"), 30)
        .unwrap();
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.status, TaskStatus::Done);
    assert!(t.milestones.iter().any(|m| m.text == "shipped"));
    assert_eq!(p.updated_at, 30);
}

#[test]
fn close_task_unknown_task_errors() {
    let (store, id, _) = store_with_task(&["status_update"]);
    let err = store
        .close_task(&id, "nope", None, AgentRef::from_pane("w"), 30)
        .unwrap_err();
    assert!(matches!(err, PlanError::TaskNotFound));
}

#[test]
fn add_milestone_rejects_undeclared_kind() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    let err = store
        .add_milestone(&id, &tid, "deploy", "x", AgentRef::from_pane("w"), false, 1)
        .unwrap_err();
    assert!(matches!(err, PlanError::UndeclaredKind { .. }));
    store
        .add_milestone(&id, &tid, "deploy", "x", AgentRef::from_pane("w"), true, 2)
        .unwrap();
    assert!(store
        .get(&id)
        .unwrap()
        .milestone_kinds
        .iter()
        .any(|k| k.key == "deploy"));
}

#[test]
fn archive_and_unarchive_toggle_status() {
    let (store, id, _) = store_with_task(&["status_update"]);
    store.archive(&id, 40).unwrap();
    assert_eq!(store.get(&id).unwrap().status, PlanStatus::Archived);
    store.unarchive(&id, 41).unwrap();
    let p = store.get(&id).unwrap();
    assert_eq!(p.status, PlanStatus::Active);
    assert_eq!(p.updated_at, 41);
}

#[test]
fn transfer_updates_owner_and_history() {
    let (store, id, _) = store_with_task(&["status_update"]);
    store
        .transfer(&id, OrchestratorRef::from_pane("new"), "handoff", 5)
        .unwrap();
    let p = store.get(&id).unwrap();
    assert_eq!(p.owner.pane_id.as_deref(), Some("new"));
    assert_eq!(p.owner_history.len(), 1);
    assert_eq!(p.owner_history[0].reason, "handoff");
}

#[test]
fn disown_then_claim_round_trip() {
    let (store, id, _) = store_with_task(&["status_update"]);
    store.disown(&id, 50).unwrap();
    assert!(store.get(&id).unwrap().owner.pane_id.is_none());
    store
        .claim(&id, OrchestratorRef::from_pane("claimer"), 51)
        .unwrap();
    let p = store.get(&id).unwrap();
    assert_eq!(p.owner.pane_id.as_deref(), Some("claimer"));
    assert_eq!(p.owner_history.len(), 2);
}

#[test]
fn owner_of_returns_current_owner() {
    let (store, id, _) = store_with_task(&["status_update"]);
    let owner = store.owner_of(&id).unwrap();
    assert_eq!(owner.pane_id.as_deref(), Some("owner"));
    assert!(store.owner_of("missing").is_none());
}

#[test]
fn set_layout_routes_by_namespaced_id() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store
        .set_layout(&id, &format!("task:{tid}"), Some(12.0), Some(34.0), 60)
        .unwrap();
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.canvas.x, Some(12.0));
    assert_eq!(t.canvas.y, Some(34.0));
}

#[test]
fn unknown_plan_errors_not_found() {
    let (store, _, _) = store_with_task(&["status_update"]);
    let err = store.archive("missing", 1).unwrap_err();
    assert!(matches!(err, PlanError::NotFound));
}
