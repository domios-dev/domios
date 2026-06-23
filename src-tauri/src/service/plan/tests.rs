use super::*;
use crate::plan::{AgentRef, MilestoneKind, OrchestratorRef, Plan, PlanStatus, TaskStatus};
use crate::service::plan::task_queries::TaskListFilter;

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

#[test]
fn scan_project_loads_plan_written_directly_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let project = paths::project_root(dir.path());
    let plans_dir = paths::plans_dir(&project).unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Persisted".into(),
        project.display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    plan.created_at = 1000;
    plan.updated_at = 2000;
    let plan_id = plan.id.clone();
    let path = plans_dir.join(format!("{plan_id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    let loaded_project = store.scan_project(dir.path()).unwrap();
    let plans: Vec<_> = store
        .list_index()
        .into_iter()
        .filter_map(|e| store.get(&e.plan_id))
        .filter(|p| p.project_path == loaded_project.display().to_string())
        .collect();

    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].id, plan_id);
    assert_eq!(plans[0].title, "Persisted");
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

#[test]
fn list_tasks_is_strictly_project_scoped() {
    let root = tempfile::tempdir().unwrap();
    let project_a = root.path().join("a");
    let project_b = root.path().join("b");
    std::fs::create_dir_all(&project_a).unwrap();
    std::fs::create_dir_all(&project_b).unwrap();
    let store = PlanStore::new_for_test(root.path());
    let plan_a = store
        .create_plan(
            "A".into(),
            project_a.display().to_string(),
            OrchestratorRef::from_pane("owner"),
            vec![],
            1,
        )
        .unwrap();
    let plan_b = store
        .create_plan(
            "B".into(),
            project_b.display().to_string(),
            OrchestratorRef::from_pane("owner"),
            vec![],
            2,
        )
        .unwrap();
    store
        .add_task(
            &plan_a.id,
            NewTask {
                name: "only-a".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .add_task(
            &plan_b.id,
            NewTask {
                name: "only-b".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            4,
        )
        .unwrap();

    let tasks = store.list_tasks(TaskListFilter {
        project_path: Some(project_b.display().to_string()),
        active_only: true,
        ..Default::default()
    });

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].plan_id, plan_b.id);
    assert_eq!(tasks[0].task.name, "only-b");
}

#[test]
fn inbox_plan_is_lazy_and_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let first = store
        .ensure_inbox_plan(
            root.path().to_str().unwrap(),
            OrchestratorRef::from_pane("owner"),
            1,
        )
        .unwrap();
    let second = store
        .ensure_inbox_plan(
            root.path().to_str().unwrap(),
            OrchestratorRef::from_pane("other"),
            2,
        )
        .unwrap();

    assert_eq!(first.id, crate::plan::INBOX_PLAN_ID);
    assert_eq!(first.title, crate::plan::INBOX_PLAN_TITLE);
    assert!(first.sections.is_empty());
    assert_eq!(second.created_at, first.created_at);
    assert!(root.path().join(".codeterm/plans/_inbox.json").exists());
}

#[test]
fn add_inbox_task_lands_in_reserved_plan() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id) = store
        .add_inbox_task(
            root.path().to_str().unwrap(),
            OrchestratorRef::from_pane("owner"),
            NewTask {
                name: "standalone".into(),
                kind: "fix".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();

    assert_eq!(plan_id, crate::plan::INBOX_PLAN_ID);
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].id, task_id);
    assert_eq!(plan.tasks[0].name, "standalone");
}

#[test]
fn task_queries_exclude_archived_plans() {
    let (store, id, _) = store_with_task(&["status_update"]);
    store.archive(&id, 99).unwrap();

    let tasks = store.list_tasks(TaskListFilter {
        active_only: false,
        ..Default::default()
    });

    assert!(tasks.is_empty());
    assert!(store.coverage_report(&id).is_none());
}
