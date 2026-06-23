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

#[test]
fn reload_from_disk_replaces_memory_and_index() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Original".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();

    let path = dir
        .path()
        .join(".codeterm/plans")
        .join(format!("{}.json", p.id));
    let mut edited: Plan = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edited.title = "Edited".into();
    edited.updated_at = 2000;
    std::fs::write(&path, serde_json::to_string_pretty(&edited).unwrap()).unwrap();

    store.reload_from_disk(&p.id).unwrap();

    assert_eq!(store.get(&p.id).unwrap().title, "Edited");
    let entry = store
        .list_index()
        .into_iter()
        .find(|e| e.plan_id == p.id)
        .unwrap();
    assert_eq!(entry.title, "Edited");
    assert_eq!(entry.updated_at, 2000);
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
fn set_layout_uses_the_real_id_the_frontend_sends() {
    // The frontend (useCanvasNodeDrag) sends node.id — the bare 'task_<uuid>' id,
    // NOT a 'task:<id>' namespace. Regression for the drag-never-persists 404:
    // set_layout used split_once(':') which rejected every underscore id.
    let (store, id, tid) = store_with_task(&["status_update"]);
    store
        .set_layout(&id, &tid, Some(12.0), Some(34.0), 60)
        .unwrap();
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!((t.canvas.x, t.canvas.y), (Some(12.0), Some(34.0)));
}

#[test]
fn unknown_plan_errors_not_found() {
    let (store, _, _) = store_with_task(&["status_update"]);
    let err = store.archive("missing", 1).unwrap_err();
    assert!(matches!(err, PlanError::NotFound));
}

#[test]
fn set_task_groups_persists_and_reloads() {
    use crate::plan::TaskGroup;
    let (store, id, _) = store_with_task(&["status_update"]);
    store
        .set_task_groups(
            &id,
            vec![TaskGroup {
                key: "phase-1".into(),
                label: "Phase 1".into(),
                color: None,
            }],
            70,
        )
        .unwrap();
    store.reload_from_disk(&id).unwrap();
    let p = store.get(&id).unwrap();
    assert_eq!(p.task_groups.len(), 1);
    assert_eq!(p.task_groups[0].key, "phase-1");
}

#[test]
fn link_and_unlink_plan_dedup_and_remove() {
    let (store, id, _) = store_with_task(&["status_update"]);
    store.link_plan(&id, "pl_other", 71).unwrap();
    store.link_plan(&id, "pl_other", 72).unwrap();
    assert_eq!(store.get(&id).unwrap().links, vec!["pl_other".to_string()]);
    store.unlink_plan(&id, "pl_other", 73).unwrap();
    assert!(store.get(&id).unwrap().links.is_empty());
}

#[test]
fn coverage_report_reports_gaps() {
    let (store, id, _) = store_with_task(&["status_update"]);
    let sid = store.add_section(&id, "Backend", "", None, 80).unwrap();
    let report = store.coverage_report(&id).unwrap();
    assert!(report.gaps.contains(&"Backend".to_string()));
    assert!(report.sections.iter().any(|s| s.section_id == sid));
}
