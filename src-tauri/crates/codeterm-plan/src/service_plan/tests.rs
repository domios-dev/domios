use super::*;
use crate::plan::{AgentRef, MilestoneKind, OrchestratorRef, Plan, PlanStatus, TaskStatus};
use crate::service_plan::task_queries::TaskListFilter;

mod tag_retag;
mod task_queries;

fn store_with_task(kinds: &[&str]) -> (PlanStore, String, String) {
    let root = tempfile::tempdir().unwrap().keep();
    let store = PlanStore::new_for_test(&root);
    let mut p = Plan::new(
        "T".into(),
        root.display().to_string(),
        OrchestratorRef::from_tab("owner"),
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
                competition: None,
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
        OrchestratorRef::from_tab("owner"),
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
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();

    store
        .update_plan(&plan.id, Some("After"), None, None, 4)
        .unwrap();
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
            None,
            &[],
            None,
            None,
            6,
        )
        .unwrap();

    store.plans.write().clear();
    store.reload_from_disk(&plan.id).unwrap();
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.title, "After");
    assert_eq!(loaded.sections[0].title, "New Spec");
    assert_eq!(loaded.tasks[0].name, "New Task");
    assert_ne!(loaded.sections[0].handle.as_deref(), Some("old-spec"));
    assert_ne!(loaded.tasks[0].handle.as_deref(), Some("old-task"));
}

#[test]
fn plan_spec_markdown_assembles_only_sections() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Spec Plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
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
                competition: None,
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
        OrchestratorRef::from_tab("owner"),
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
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();

    let manifest = store.file_manifest(&plan.id).unwrap();
    assert_eq!(
        manifest.index_path,
        format!(".codeterm/plans/active/{}/INDEX.md", plan.id)
    );
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "section"
            && entry.title == "spec (Spec)"
            && entry.path == format!(".codeterm/plans/active/{}/sections/spec.md", plan.id)
    }));
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "task"
            && entry.title == "build (Build)"
            && entry.path == format!(".codeterm/plans/active/{}/tasks/build.md", plan.id)
    }));
}

#[test]
fn external_section_body_edit_reloads_into_full_plan() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "External edit".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let _section_id = store
        .add_section(&plan.id, "Spec", "before", None, &[], None, None, 2)
        .unwrap();
    let plan_dir = store_v2::plan_state_dir(&store.get(&plan.id).unwrap()).unwrap();
    let plan_path = store_v2::plan_yaml_path(&plan_dir);
    let mut disk: Plan =
        serde_yaml::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();
    disk.sections[0].body = "after direct edit".into();
    std::fs::write(&plan_path, serde_yaml::to_string(&disk).unwrap()).unwrap();
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
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn inline_body_plan_migrates_to_markdown_files_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let plans_dir = paths::plans_dir(&project).unwrap();
    let mut plan = Plan::new(
        "Legacy inline".into(),
        project.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.sections.push(crate::plan::Section {
        id: "sec_legacy".into(),
        title: "Spec".into(),
        handle: None,
        tags: vec![],
        body: "inline section".into(),
        subsections: vec![],
        canvas: Default::default(),
        attachments: vec![],
    });
    plan.tasks.push(crate::plan::Task {
        id: "task_legacy".into(),
        name: "Task".into(),
        kind: "implement".into(),
        model: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        external_refs: vec![],
        test_plan: None,
        qa_plan: None,
        correctness: None,
        status: TaskStatus::Pending,
        review_stage: Default::default(),
        qa_state: None,
        review_round: 0,
        review_oracle: None,
        review_evidence: vec![],
        review_override: None,
        review_audit: vec![],
        status_label: None,
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
        tags: vec![],
        section_ids: vec![],
        body: "inline task".into(),
        role_guidance: None,
        executor_role: None,
        competition: None,
        intent: None,
        execution_intent: Default::default(),
        parent_task_id: None,
        depends_on: vec![],
        assigned_tab_id: None,
        lane: Default::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: Default::default(),
        milestones: vec![],
        execution_attempts: vec![],
        interruption: Default::default(),
        pause_reason: None,
        personality: None,
        personality_provenance: None,
        attachments: vec![],
    });
    let json_path = paths::plan_json_path(&project, &plan.status, &plan.id).unwrap();
    std::fs::write(&json_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    store.scan_project(dir.path()).unwrap();

    let section_path = plans_dir.join(format!("active/{}/sections/spec.md", plan.id));
    let task_path = plans_dir.join(format!("active/{}/tasks/task.md", plan.id));
    assert!(std::fs::read_to_string(section_path)
        .unwrap()
        .ends_with("inline section\n"));
    assert!(std::fs::read_to_string(task_path)
        .unwrap()
        .ends_with("inline task\n"));
    let raw_json = std::fs::read_to_string(json_path).unwrap();
    assert!(!raw_json.contains("inline section"));
    assert!(!raw_json.contains("inline task"));
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.sections[0].body, "inline section");
    assert_eq!(loaded.tasks[0].body, "inline task");
}

#[test]
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn atomic_body_write_leaves_only_final_markdown_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Atomic".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
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

    let plan_dir = store_v2::plan_state_dir(&store.get(&plan.id).unwrap()).unwrap();
    let plan_path = store_v2::plan_yaml_path(&plan_dir);
    let disk: Plan = serde_yaml::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();
    assert_eq!(disk.sections[0].body, "replacement\n");
    assert!(!plan_dir.join("sections/spec.md.tmp").exists());
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
                competition: None,
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
                depends_on: vec![],
                competition: None,
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
                depends_on: vec![],
                competition: None,
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
                competition: None,
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
                competition: None,
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
    store.assign_task(&id, &tid, "tab-live", None, 20).unwrap();
    store
        .close_task(&id, &tid, Some("shipped"), AgentRef::from_tab("w"), 30)
        .unwrap();
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.status, TaskStatus::Done);
    assert!(t.assigned_tab_id.is_none());
    assert!(t.lane.tab_id.is_none());
    assert!(t.milestones.iter().any(|m| m.text == "shipped"));
    assert_eq!(p.updated_at, 30);
}

#[test]
fn close_task_closes_every_live_attempt_not_only_scalar_assignment() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "tab-live", None, 20).unwrap();
    store
        .with_plan(&id, 25, |plan| {
            let task = plan.tasks.iter_mut().find(|task| task.id == tid).unwrap();
            task.execution_attempts
                .push(crate::plan::TaskExecutionAttempt {
                    id: "attempt-vanished".into(),
                    executor_tab_id: "tab-vanished".into(),
                    assigned_at: Some(10),
                    acknowledged_at: Some(11),
                    ..Default::default()
                });
            Ok(())
        })
        .unwrap();

    store
        .close_task(&id, &tid, None, AgentRef::from_tab("w"), 30)
        .unwrap();

    let plan = store.get(&id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == tid).unwrap();
    assert!(task
        .execution_attempts
        .iter()
        .all(|attempt| attempt.closed_at == Some(30)));
}

#[test]
fn closed_task_assignment_is_not_indexed_or_bound() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "tab-live", None, 20).unwrap();
    store
        .close_task(&id, &tid, None, AgentRef::from_tab("w"), 30)
        .unwrap();

    let entry = store
        .list_index()
        .into_iter()
        .find(|entry| entry.plan_id == id)
        .unwrap();
    assert!(entry.assigned_tab_ids.is_empty());
    assert!(store.task_binding_for_tab("tab-live").is_none());
    assert!(store.task_bindings_for_tab("tab-live").is_empty());
}

#[test]
fn reconcile_assignment_cleanup_clears_lane_tab() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "tab-live", None, 20).unwrap();
    let drift = [crate::plan::reconcile::DriftItem {
        plan_id: id.clone(),
        task_id: tid.clone(),
        kind: crate::plan::reconcile::DriftKind::BoundToClosedTab,
        detail: "closed".into(),
        suggested_fix: "clear".into(),
        not_mine: false,
    }];

    store.apply_reconcile_fixes(&id, &drift, 30).unwrap();

    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert!(t.assigned_tab_id.is_none());
    assert!(t.lane.tab_id.is_none());
}

#[test]
fn tab_gone_cleanup_clears_lane_tab() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "tab-live", None, 20).unwrap();

    assert!(store.flag_tab_gone(&id, &tid, "tab-live", 30).unwrap());

    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert!(t.assigned_tab_id.is_none());
    assert!(t.lane.tab_id.is_none());
}

#[test]
fn lane_demotion_preserves_in_progress_status_and_marks_needs_reassign() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store.assign_task(&id, &tid, "tab-live", None, 20).unwrap();
    store
        .update_task(
            &id,
            &tid,
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
            21,
        )
        .unwrap();

    let fixes = store
        .apply_lane_demotions(
            &id,
            &[crate::plan::reconcile::LaneDemotion {
                task_id: tid.clone(),
                plan_id: id.clone(),
                reason: "tab missing after restart".into(),
            }],
            30,
        )
        .unwrap();

    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0].action, "cleared_assignment_set_needs_reassign");
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.status, TaskStatus::InProgress);
    assert_eq!(
        t.status_label.as_deref(),
        Some(crate::plan::reconcile::NEEDS_REASSIGN_LABEL)
    );
    assert!(t.assigned_tab_id.is_none());
    assert!(t.lane.tab_id.is_none());
    assert!(t
        .milestones
        .iter()
        .any(|milestone| milestone.text == "tab missing after restart"));
}

#[test]
fn close_task_unknown_task_errors() {
    let (store, id, _) = store_with_task(&["status_update"]);
    let err = store
        .close_task(&id, "nope", None, AgentRef::from_tab("w"), 30)
        .unwrap_err();
    assert!(matches!(err, PlanError::TaskNotFound));
}

#[test]
fn add_milestone_rejects_undeclared_kind() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    let err = store
        .add_milestone(&id, &tid, "deploy", "x", AgentRef::from_tab("w"), false, 1)
        .unwrap_err();
    assert!(matches!(err, PlanError::UndeclaredKind { .. }));
    store
        .add_milestone(&id, &tid, "deploy", "x", AgentRef::from_tab("w"), true, 2)
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
    store
        .add_retrospective(&id, "retro", AgentRef::from_tab("orch"), 40)
        .unwrap();
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
        .transfer(&id, OrchestratorRef::from_tab("new"), "handoff", 5)
        .unwrap();
    let p = store.get(&id).unwrap();
    assert_eq!(p.owner.tab_id.as_deref(), Some("new"));
    assert_eq!(p.owner_history.len(), 1);
    assert_eq!(p.owner_history[0].reason, "handoff");
}

#[test]
fn disown_then_claim_round_trip() {
    let (store, id, _) = store_with_task(&["status_update"]);
    store.disown(&id, 50).unwrap();
    assert!(store.get(&id).unwrap().owner.tab_id.is_none());
    store
        .claim(&id, OrchestratorRef::from_tab("claimer"), 51)
        .unwrap();
    let p = store.get(&id).unwrap();
    assert_eq!(p.owner.tab_id.as_deref(), Some("claimer"));
    assert_eq!(p.owner_history.len(), 2);
}

#[test]
fn owner_of_returns_current_owner() {
    let (store, id, _) = store_with_task(&["status_update"]);
    let owner = store.owner_of(&id).unwrap();
    assert_eq!(owner.tab_id.as_deref(), Some("owner"));
    assert!(store.owner_of("missing").is_none());
}

#[test]
fn set_layout_uses_the_real_id_the_frontend_sends() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store
        .set_layout(&id, &tid, Some(12.0), Some(34.0), 60)
        .unwrap();
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!((t.canvas.x, t.canvas.y), (Some(12.0), Some(34.0)));
}

#[test]
fn set_layout_accepts_prefixed_ids_from_cli_and_docs() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    store
        .set_layout(&id, &format!("task:{tid}"), Some(56.0), Some(78.0), 60)
        .unwrap();
    let p = store.get(&id).unwrap();
    let t = p.tasks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!((t.canvas.x, t.canvas.y), (Some(56.0), Some(78.0)));
}

#[test]
fn draft_task_creation_does_not_activate_execution() {
    use crate::plan::PlanMode;

    let root = tempfile::tempdir().unwrap().keep();
    let store = PlanStore::new_for_test(&root);
    let mut plan = crate::plan::Plan::new(
        "Approval".into(),
        root.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    let id = plan.id.clone();
    store.save(&mut plan, 1).unwrap();

    store
        .ensure_task_creation_allowed(&id, "implement", false)
        .unwrap();

    // Planning-kind tasks are first-class in planning mode: allowed without --force.
    store
        .ensure_task_creation_allowed(&id, "planning", false)
        .unwrap();
    assert!(!store.get(&id).unwrap().execution_tasks_unlocked());

    store.set_mode(&id, PlanMode::Approved, 2).unwrap();
    store
        .ensure_task_creation_allowed(&id, "implement", false)
        .unwrap();
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
            OrchestratorRef::from_tab("owner"),
            vec![],
            1,
        )
        .unwrap();
    let plan_b = store
        .create_plan(
            "B".into(),
            project_b.display().to_string(),
            OrchestratorRef::from_tab("owner"),
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
                competition: None,
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
                competition: None,
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
fn public_inbox_mutation_cannot_create_active_teamless_plan() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let first = store
        .ensure_inbox_plan(
            root.path().to_str().unwrap(),
            OrchestratorRef::from_tab("owner"),
            1,
        )
        .unwrap();
    let second = store
        .ensure_inbox_plan(
            root.path().to_str().unwrap(),
            OrchestratorRef::from_tab("other"),
            2,
        )
        .unwrap();

    assert_eq!(first.id, crate::plan::INBOX_PLAN_ID);
    assert_eq!(first.title, crate::plan::INBOX_PLAN_TITLE);
    assert_eq!(first.status, crate::plan::PlanStatus::Archived);
    assert_eq!(first.team_id, None);
    assert!(first.sections.is_empty());
    assert_eq!(second.created_at, first.created_at);
    assert!(root
        .path()
        .join(".codeterm/plans/archive/_inbox/plan.yaml")
        .exists());
}

#[test]
fn public_task_add_lands_in_archived_inbox_artifact() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id) = store
        .add_inbox_task(
            root.path().to_str().unwrap(),
            OrchestratorRef::from_tab("owner"),
            NewTask {
                name: "standalone".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();

    assert_eq!(plan_id, crate::plan::INBOX_PLAN_ID);
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.status, crate::plan::PlanStatus::Archived);
    assert_eq!(plan.team_id, None);
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].id, task_id);
    assert_eq!(plan.tasks[0].name, "standalone");
}
#[test]
fn index_projection_keeps_canonical_ownership_separate_from_central_storage() {
    let storage = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let database_path = storage.path().join("plans.db");
    let canonical_project_path = std::fs::canonicalize(project.path())
        .unwrap()
        .display()
        .to_string();
    let plan = Plan::new(
        "Central ownership projection".into(),
        canonical_project_path.clone(),
        OrchestratorRef::from_tab("owner-tab"),
    );
    let database = store_db::PlanDatabase::open(database_path.clone()).unwrap();
    let stored_path = database.persist_plan(&plan, None).unwrap();
    let entry = PlanStore::index_entry(&plan, stored_path);

    assert_eq!(entry.file_path, database_path);
    assert_eq!(entry.canonical_project_path, canonical_project_path);
    let projection = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        projection["canonical_project_path"],
        entry.canonical_project_path
    );
    assert!(!entry.file_path.starts_with(project.path()));
    assert!(!project.path().join(".codeterm").join("plans").exists());
}

#[test]
fn index_projection_derives_legacy_ownership_from_the_record_project_path() {
    let storage = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let database_path = storage.path().join("plans.db");
    let project_path = std::fs::canonicalize(project.path())
        .unwrap()
        .display()
        .to_string();
    let mut plan = Plan::new(
        "Legacy central ownership projection".into(),
        project_path.clone(),
        OrchestratorRef::from_tab("owner-tab"),
    );
    plan.canonical_project_path.clear();
    let database = store_db::PlanDatabase::open(database_path.clone()).unwrap();
    let stored_path = database.persist_plan(&plan, None).unwrap();
    let stored = database.load_plan(&plan.id).unwrap().unwrap();

    assert!(stored.canonical_project_path.is_empty());
    let entry = PlanStore::index_entry(&stored, stored_path);
    assert_eq!(entry.canonical_project_path, project_path);
    assert_eq!(entry.file_path, database_path);
    assert!(!entry.file_path.starts_with(project.path()));
    assert!(!project.path().join(".codeterm").join("plans").exists());
}
