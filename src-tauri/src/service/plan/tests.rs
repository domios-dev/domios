use super::*;
use crate::plan::{AgentRef, MilestoneKind, OrchestratorRef, Plan, PlanStatus, TaskStatus};
use crate::service::plan::task_queries::TaskListFilter;

mod tag_retag;

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
fn in_review_status_persists_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Review".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "worker lane".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .update_task(
            &p.id,
            &task_id,
            None,
            None,
            None,
            None,
            Some("in_review"),
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
            3,
        )
        .unwrap();

    store.plans.write().clear();
    store.reload_from_disk(&p.id).unwrap();

    let loaded = store.get(&p.id).unwrap();
    let task = loaded.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::InReview);
}

#[test]
fn task_quality_fields_persist_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Quality".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "criteria lane".into(),
                kind: "implement".into(),
                acceptance_criteria: vec![
                    "Task JSON contains structured acceptance criteria".into(),
                    "CLI shows each criterion".into(),
                ],
                test_plan: Some(
                    "cargo test service::plan::tests::task_quality_fields_persist_and_reload"
                        .into(),
                ),
                correctness: Some("quality fields round-trip unchanged".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store.plans.write().clear();
    store.reload_from_disk(&p.id).unwrap();

    let loaded = store.get(&p.id).unwrap();
    let task = loaded.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(
        task.acceptance_criteria,
        vec![
            "Task JSON contains structured acceptance criteria",
            "CLI shows each criterion"
        ]
    );
    assert_eq!(
        task.test_plan.as_deref(),
        Some("cargo test service::plan::tests::task_quality_fields_persist_and_reload")
    );
    assert_eq!(
        task.correctness.as_deref(),
        Some("quality fields round-trip unchanged")
    );
}

#[test]
fn reload_from_disk_preserves_plan_arc_for_concurrent_holders() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Original".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();
    let held = store.plans.read().get(&p.id).cloned().unwrap();

    let path = dir
        .path()
        .join(".codeterm/plans")
        .join(format!("{}.json", p.id));
    let mut edited: Plan = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edited.title = "Edited".into();
    edited.updated_at = 2000;
    std::fs::write(&path, serde_json::to_string_pretty(&edited).unwrap()).unwrap();

    store.reload_from_disk(&p.id).unwrap();

    let current = store.plans.read().get(&p.id).cloned().unwrap();
    assert!(std::sync::Arc::ptr_eq(&held, &current));
    assert_eq!(held.lock().title, "Edited");
}

#[test]
fn reload_from_disk_does_not_overwrite_newer_in_flight_plan_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(PlanStore::new_for_test(dir.path()));
    let mut p = Plan::new(
        "Original".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();
    let held = store.plans.read().get(&p.id).cloned().unwrap();

    let path = dir
        .path()
        .join(".codeterm/plans")
        .join(format!("{}.json", p.id));
    let mut stale: Plan = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    stale.title = "Stale disk snapshot".into();
    stale.updated_at = 3000;
    std::fs::write(&path, serde_json::to_string_pretty(&stale).unwrap()).unwrap();

    let mut guard = held.lock();
    let reload_store = store.clone();
    let plan_id = p.id.clone();
    let reload = std::thread::spawn(move || reload_store.reload_from_disk(&plan_id));
    std::thread::sleep(std::time::Duration::from_millis(25));

    guard.title = "Newer in-flight mutation".into();
    guard.updated_at = 3000;
    let written_path = PlanStore::write_atomic(&guard).unwrap();
    store.note_plan_write(&guard, &written_path);
    drop(guard);

    reload.join().unwrap().unwrap();

    let current = store.get(&p.id).unwrap();
    assert_eq!(current.title, "Newer in-flight mutation");
    assert_eq!(current.updated_at, 3000);
    let entry = store
        .list_index()
        .into_iter()
        .find(|entry| entry.plan_id == p.id)
        .unwrap();
    assert_eq!(entry.title, "Newer in-flight mutation");
    assert_eq!(entry.updated_at, 3000);
}

#[test]
fn scan_project_loads_plan_written_directly_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let plans_dir = paths::plans_dir(&project).unwrap();
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
    let sid = store
        .add_section(&id, "Spec", "body", None, &[], None, None, 10)
        .unwrap();
    let p = store.get(&id).unwrap();
    assert!(p.sections.iter().any(|s| s.id == sid && s.title == "Spec"));
    assert_eq!(p.updated_at, 10);
    let sub = store
        .add_section(&id, "Child", "", Some(&sid), &[], None, None, 11)
        .unwrap();
    let p = store.get(&id).unwrap();
    let parent = p.sections.iter().find(|s| s.id == sid).unwrap();
    assert!(parent.subsections.iter().any(|s| s.id == sub));
}

#[test]
fn generated_index_regenerates_with_current_titles() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Before".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let section_id = store
        .add_section(&plan.id, "Old Spec", "", None, &[], None, None, 2)
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Old Task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();

    store.update_plan(&plan.id, Some("After"), None, 4).unwrap();
    store
        .update_section(
            &plan.id,
            &section_id,
            Some("New Spec"),
            None,
            &[],
            None,
            None,
            5,
        )
        .unwrap();
    store
        .update_task(
            &plan.id,
            &task_id,
            Some("New Task"),
            None,
            None,
            None,
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
            6,
        )
        .unwrap();

    let index_path = dir
        .path()
        .join(format!(".codeterm/plans/{}/INDEX.md", plan.id));
    let index = std::fs::read_to_string(index_path).unwrap();
    assert!(index.contains("# After"));
    assert!(index.contains("- [new-spec (New Spec)](sections/new-spec.md)"));
    assert!(index.contains("- [new-task (New Task)](tasks/new-task.md)"));
    assert!(!index.contains("Old Spec"));
    assert!(!index.contains("Old Task"));
}

#[test]
fn plan_spec_markdown_assembles_only_sections() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Spec Plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let section_id = store
        .add_section(
            &plan.id,
            "Overview",
            "overview body",
            None,
            &[],
            None,
            None,
            2,
        )
        .unwrap();
    store
        .add_section(
            &plan.id,
            "Detail",
            "detail body",
            Some(&section_id),
            &[],
            None,
            None,
            3,
        )
        .unwrap();
    store
        .add_task(
            &plan.id,
            NewTask {
                name: "Implement".into(),
                kind: "implement".into(),
                body: "task body".into(),
                ..Default::default()
            },
            4,
        )
        .unwrap();

    let spec = store.spec_markdown(&plan.id).unwrap();
    assert!(spec.contains("# Overview\n\noverview body"));
    assert!(spec.contains("## Detail\n\ndetail body"));
    assert!(!spec.contains("Implement"));
    assert!(!spec.contains("task body"));
}

#[test]
fn plan_file_manifest_lists_index_and_body_paths() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Files".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .add_section(&plan.id, "Spec", "", None, &[], None, None, 2)
        .unwrap();
    store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();

    let manifest = store.file_manifest(&plan.id).unwrap();
    assert_eq!(
        manifest.index_path,
        format!(".codeterm/plans/{}/INDEX.md", plan.id)
    );
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "section"
            && entry.title == "spec (Spec)"
            && entry.path == format!(".codeterm/plans/{}/sections/spec.md", plan.id)
    }));
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "task"
            && entry.title == "build (Build)"
            && entry.path == format!(".codeterm/plans/{}/tasks/build.md", plan.id)
    }));
}

#[test]
fn external_section_body_edit_reloads_into_full_plan() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "External edit".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let _section_id = store
        .add_section(&plan.id, "Spec", "before", None, &[], None, None, 2)
        .unwrap();
    let section_path = dir
        .path()
        .join(format!(".codeterm/plans/{}/sections/spec.md", plan.id));

    std::fs::write(&section_path, "after direct edit").unwrap();
    store.reload_from_disk(&plan.id).unwrap();

    let list_plan = store
        .list_full()
        .into_iter()
        .find(|p| p.id == plan.id)
        .unwrap();
    assert_eq!(list_plan.sections[0].body, "after direct edit");
    assert_eq!(
        store.get(&plan.id).unwrap().sections[0].body,
        "after direct edit"
    );
}

#[test]
fn inline_body_plan_migrates_to_markdown_files_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let plans_dir = paths::plans_dir(&project).unwrap();
    let mut plan = Plan::new(
        "Legacy inline".into(),
        project.display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    plan.sections.push(crate::plan::Section {
        id: "sec_legacy".into(),
        title: "Spec".into(),
        handle: None,
        tags: vec![],
        body: "inline section".into(),
        subsections: vec![],
        canvas: Default::default(),
    });
    plan.tasks.push(crate::plan::Task {
        id: "task_legacy".into(),
        name: "Task".into(),
        kind: "implement".into(),
        model: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        test_plan: None,
        correctness: None,
        status: TaskStatus::Pending,
        status_label: None,
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
        tags: vec![],
        section_ids: vec![],
        body: "inline task".into(),
        intent: None,
        parent_task_id: None,
        assigned_pane_id: None,
        lane: Default::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: Default::default(),
        milestones: vec![],
    });
    let json_path = plans_dir.join(format!("{}.json", plan.id));
    std::fs::write(&json_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    store.scan_project(dir.path()).unwrap();

    let section_path = plans_dir.join(format!("{}/sections/spec.md", plan.id));
    let task_path = plans_dir.join(format!("{}/tasks/task.md", plan.id));
    assert_eq!(
        std::fs::read_to_string(section_path).unwrap(),
        "inline section"
    );
    assert_eq!(std::fs::read_to_string(task_path).unwrap(), "inline task");
    let raw_json = std::fs::read_to_string(json_path).unwrap();
    assert!(!raw_json.contains("inline section"));
    assert!(!raw_json.contains("inline task"));
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.sections[0].body, "inline section");
    assert_eq!(loaded.tasks[0].body, "inline task");
}

#[test]
fn atomic_body_write_leaves_only_final_markdown_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Atomic".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let section_id = store
        .add_section(&plan.id, "Spec", "body", None, &[], None, None, 2)
        .unwrap();
    store
        .update_section(
            &plan.id,
            &section_id,
            None,
            Some("replacement"),
            &[],
            None,
            None,
            3,
        )
        .unwrap();

    let section_dir = dir
        .path()
        .join(format!(".codeterm/plans/{}/sections", plan.id));
    assert_eq!(
        std::fs::read_to_string(section_dir.join("spec.md")).unwrap(),
        "replacement"
    );
    assert!(!section_dir.join("spec.md.tmp").exists());
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
fn show_task_includes_parent_label_for_subtasks() {
    let (store, id, parent_id) = store_with_task(&["status_update"]);
    let child_id = store
        .add_task(
            &id,
            NewTask {
                name: "Child".into(),
                kind: "implement".into(),
                parent_task_id: Some(parent_id.clone()),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    let detail = store.show_task(&child_id).expect("child task");
    let parent = detail.parent.expect("parent label");
    assert_eq!(parent.id, parent_id);
    assert_eq!(parent.name, "n");
}

#[test]
fn add_task_persists_parent_task_id() {
    let (store, id, parent_id) = store_with_task(&["status_update"]);
    let child_id = store
        .add_task(
            &id,
            NewTask {
                name: "Child".into(),
                kind: "implement".into(),
                parent_task_id: Some(parent_id.clone()),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    let p = store.get(&id).unwrap();
    let child = p.tasks.iter().find(|t| t.id == child_id).unwrap();
    assert_eq!(child.parent_task_id.as_deref(), Some(parent_id.as_str()));
}

#[test]
fn update_task_sets_and_clears_parent_task_id() {
    let (store, id, parent_id) = store_with_task(&["status_update"]);
    let child_id = store
        .add_task(
            &id,
            NewTask {
                name: "Child".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store
        .update_task(
            &id,
            &child_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some(parent_id.as_str())),
            None,
            &[],
            None,
            None,
            21,
        )
        .unwrap();
    assert_eq!(
        store
            .get(&id)
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.id == child_id)
            .unwrap()
            .parent_task_id
            .as_deref(),
        Some(parent_id.as_str())
    );

    store
        .update_task(
            &id,
            &child_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(None),
            None,
            &[],
            None,
            None,
            22,
        )
        .unwrap();
    assert_eq!(
        store
            .get(&id)
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.id == child_id)
            .unwrap()
            .parent_task_id,
        None
    );
}

#[test]
fn update_task_preserves_model_when_patching_parent() {
    let (store, id, parent_id) = store_with_task(&["status_update"]);
    let child_id = store
        .add_task(
            &id,
            NewTask {
                name: "Child".into(),
                kind: "implement".into(),
                model: Some("opus".into()),
                model_rationale: Some("deep reasoning".into()),
                ..Default::default()
            },
            20,
        )
        .unwrap();

    store
        .update_task(
            &id,
            &child_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some(parent_id.as_str())),
            None,
            &[],
            None,
            None,
            21,
        )
        .unwrap();

    let plan = store.get(&id).unwrap();
    let child = plan.tasks.iter().find(|t| t.id == child_id).unwrap();
    assert_eq!(child.parent_task_id.as_deref(), Some(parent_id.as_str()));
    assert_eq!(child.model.as_deref(), Some("opus"));
    assert_eq!(child.model_rationale.as_deref(), Some("deep reasoning"));
}

#[test]
fn close_task_sets_done_and_optional_milestone() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "pane-live", None, 20).unwrap();
    store
        .close_task(&id, &tid, Some("shipped"), AgentRef::from_pane("w"), 30)
        .unwrap();
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.status, TaskStatus::Done);
    assert!(t.assigned_pane_id.is_none());
    assert!(t.lane.pane_id.is_none());
    assert!(t.milestones.iter().any(|m| m.text == "shipped"));
    assert_eq!(p.updated_at, 30);
}

#[test]
fn closed_task_assignment_is_not_indexed_or_bound() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "pane-live", None, 20).unwrap();
    store
        .close_task(&id, &tid, None, AgentRef::from_pane("w"), 30)
        .unwrap();

    let entry = store
        .list_index()
        .into_iter()
        .find(|entry| entry.plan_id == id)
        .unwrap();
    assert!(entry.assigned_pane_ids.is_empty());
    assert!(store.task_binding_for_pane("pane-live").is_none());
    assert!(store.task_bindings_for_pane("pane-live").is_empty());
}

#[test]
fn reconcile_assignment_cleanup_clears_lane_pane() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "pane-live", None, 20).unwrap();
    let drift = [crate::plan::reconcile::DriftItem {
        plan_id: id.clone(),
        task_id: tid.clone(),
        kind: crate::plan::reconcile::DriftKind::BoundToClosedPane,
        detail: "closed".into(),
        suggested_fix: "clear".into(),
        not_mine: false,
    }];

    store.apply_reconcile_fixes(&id, &drift, 30).unwrap();

    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert!(t.assigned_pane_id.is_none());
    assert!(t.lane.pane_id.is_none());
}

#[test]
fn pane_gone_cleanup_clears_lane_pane() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "pane-live", None, 20).unwrap();

    assert!(store.flag_pane_gone(&id, &tid, 30).unwrap());

    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert!(t.assigned_pane_id.is_none());
    assert!(t.lane.pane_id.is_none());
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
fn set_tag_facets_persists_and_reloads() {
    use crate::plan::{TagFacet, TagFacetValue};
    let (store, id, _) = store_with_task(&["status_update"]);
    store
        .set_tag_facets(
            &id,
            vec![TagFacet {
                key: "area".into(),
                label: Some("Area".into()),
                color_lane: true,
                values: vec![TagFacetValue {
                    v: "api".into(),
                    label: Some("API".into()),
                    color: Some("#38bdf8".into()),
                }],
            }],
            70,
        )
        .unwrap();
    store.reload_from_disk(&id).unwrap();
    let p = store.get(&id).unwrap();
    assert_eq!(p.tag_facets.len(), 1);
    assert_eq!(p.tag_facets[0].key, "area");
    assert!(p.tag_facets[0].color_lane);
    assert_eq!(p.tag_facets[0].values[0].color.as_deref(), Some("#38bdf8"));
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
    let sid = store
        .add_section(&id, "Backend", "", None, &[], None, None, 80)
        .unwrap();
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

#[test]
fn task_queries_filter_by_status() {
    let (store, id, blocked_id) = store_with_task(&["status_update"]);
    store
        .add_task(
            &id,
            NewTask {
                name: "still-pending".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .update_task(
            &id,
            &blocked_id,
            None,
            None,
            None,
            None,
            Some("blocked"),
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
            3,
        )
        .unwrap();

    let tasks = store.list_tasks(TaskListFilter {
        active_only: false,
        status: Some(TaskStatus::Blocked),
        ..Default::default()
    });

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task.id, blocked_id);
}
