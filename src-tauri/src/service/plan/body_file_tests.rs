use super::*;
use crate::plan::{AgentRef, CanvasElement, OrchestratorRef, Plan};

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

#[test]
fn milestone_and_note_bodies_roundtrip_through_markdown_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "External prose".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let milestone_text = "STATUS: done\n\n".to_string() + &"long milestone detail ".repeat(20);
    let milestone_id = store
        .add_milestone(
            &plan.id,
            &task_id,
            "status_update",
            &milestone_text,
            AgentRef::from_pane("worker"),
            true,
            3,
        )
        .unwrap();
    let note_text = "# Note\n\n".to_string() + &"long note detail ".repeat(22);
    let note_id = store
        .add_element(
            &plan.id,
            "note",
            serde_json::json!({ "text": note_text }),
            vec![],
            None,
            None,
            4,
        )
        .unwrap();

    let plan_dir = dir.path().join(format!(".codeterm/plans/{}", plan.id));
    let milestone_path = plan_dir.join(format!("milestones/build-{}.md", milestone_id));
    let stride_log_path = plan_dir.join("tasks/build/strides.jsonl");
    let note_path = plan_dir.join(format!("elements/note-{}.md", note_id));
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_text
    );
    let strides = crate::service::plan::stride_log::read_milestones(&stride_log_path).unwrap();
    assert_eq!(strides.len(), 1);
    assert_eq!(strides[0].id, milestone_id);
    assert_eq!(strides[0].text, milestone_text);
    assert_eq!(std::fs::read_to_string(&note_path).unwrap(), note_text);
    let raw_json = std::fs::read_to_string(
        dir.path()
            .join(".codeterm/plans")
            .join(format!("{}.json", plan.id)),
    )
    .unwrap();
    let raw_plan: Plan = serde_json::from_str(&raw_json).unwrap();
    assert!(raw_plan.tasks[0].milestones.is_empty());
    assert!(!raw_json.contains("long milestone detail"));
    assert!(!raw_json.contains("long note detail"));

    std::fs::write(&milestone_path, "edited milestone\n\nbody").unwrap();
    std::fs::write(&note_path, "edited note\n\nbody").unwrap();
    store.reload_from_disk(&plan.id).unwrap();
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(
        loaded.tasks[0].milestones[0].text,
        "edited milestone\n\nbody"
    );
    assert_eq!(loaded.elements[0].content["text"], "edited note\n\nbody");
}

#[test]
fn task_stride_log_hydrates_when_inline_milestones_are_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Stride hydrate".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let milestone_id = store
        .add_milestone(
            &plan.id,
            &task_id,
            "testing",
            "sidecar-only",
            AgentRef::from_pane("worker"),
            true,
            3,
        )
        .unwrap();
    let json_path = dir
        .path()
        .join(".codeterm/plans")
        .join(format!("{}.json", plan.id));
    let mut disk_plan: Plan =
        serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
    disk_plan.tasks[0].milestones.clear();
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&disk_plan).unwrap(),
    )
    .unwrap();

    store.reload_from_disk(&plan.id).unwrap();

    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.tasks[0].milestones.len(), 1);
    assert_eq!(loaded.tasks[0].milestones[0].id, milestone_id);
    assert_eq!(loaded.tasks[0].milestones[0].text, "sidecar-only");
}

#[test]
fn inline_and_stride_log_milestones_dedupe_deterministically() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Stride dedupe".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let milestone_id = store
        .add_milestone(
            &plan.id,
            &task_id,
            "status_update",
            "inline duplicate",
            AgentRef::from_pane("worker"),
            true,
            30,
        )
        .unwrap();
    let mut older = store.get(&plan.id).unwrap().tasks[0].milestones[0].clone();
    older.text = "older sidecar duplicate".into();
    older.ts = 10;
    let mut other = older.clone();
    other.id = "ms_other".into();
    other.text = "other sidecar stride".into();
    other.ts = 20;
    let stride_log_path = dir.path().join(format!(
        ".codeterm/plans/{}/tasks/build/strides.jsonl",
        plan.id
    ));
    crate::service::plan::stride_log::append_milestone(&stride_log_path, &older).unwrap();
    crate::service::plan::stride_log::append_milestone(&stride_log_path, &other).unwrap();

    store.reload_from_disk(&plan.id).unwrap();

    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.tasks[0].milestones.len(), 2);
    assert_eq!(loaded.tasks[0].milestones[0].id, milestone_id);
    assert_eq!(
        loaded.tasks[0].milestones[0].text,
        "older sidecar duplicate"
    );
    assert_eq!(loaded.tasks[0].milestones[1].id, "ms_other");
}

#[test]
fn inline_milestone_and_canvas_markdown_migrates_to_files_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let plans_dir = paths::plans_dir(&project).unwrap();
    let mut plan = Plan::new(
        "Legacy prose".into(),
        project.display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    plan.tasks.push(crate::plan::Task {
        id: "task_legacy".into(),
        name: "Task".into(),
        kind: "implement".into(),
        model: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        test_plan: None,
        correctness: None,
        status: crate::plan::TaskStatus::Pending,
        status_label: None,
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: Some("legacy-task".into()),
        tags: vec![],
        section_ids: vec![],
        body: String::new(),
        intent: None,
        parent_task_id: None,
        assigned_pane_id: None,
        lane: Default::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: Default::default(),
        milestones: vec![crate::plan::Milestone {
            id: "ms_legacy".into(),
            from: AgentRef::from_pane("worker"),
            kind: "decision".into(),
            text: "legacy milestone\n\nbody".into(),
            ts: 1,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
        }],
    });
    plan.elements.push(CanvasElement {
        id: "el_discussion".into(),
        kind: "discussion".into(),
        canvas: Default::default(),
        content: serde_json::json!({ "body": "legacy discussion\n\nbody" }),
        links: vec![],
    });
    let json_path = plans_dir.join(format!("{}.json", plan.id));
    std::fs::write(&json_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    store.scan_project(dir.path()).unwrap();

    let plan_dir = plans_dir.join(&plan.id);
    assert_eq!(
        std::fs::read_to_string(plan_dir.join("milestones/legacy-task-ms_legacy.md")).unwrap(),
        "legacy milestone\n\nbody"
    );
    assert_eq!(
        std::fs::read_to_string(plan_dir.join("elements/discussion-el_discussion.md")).unwrap(),
        "legacy discussion\n\nbody"
    );
    let raw_json = std::fs::read_to_string(json_path).unwrap();
    let raw_plan: Plan = serde_json::from_str(&raw_json).unwrap();
    assert!(raw_plan.tasks[0].milestones.is_empty());
    assert!(!raw_json.contains("legacy milestone"));
    assert!(!raw_json.contains("legacy discussion"));
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(
        loaded.tasks[0].milestones[0].text,
        "legacy milestone\n\nbody"
    );
    assert_eq!(
        loaded.elements[0].content["body"],
        "legacy discussion\n\nbody"
    );
}

#[test]
fn legacy_inline_only_milestones_load_then_save_to_stride_log_and_stripped_json() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let plans_dir = paths::plans_dir(&project).unwrap();
    let mut plan = Plan::new(
        "Legacy inline strides".into(),
        project.display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    plan.tasks.push(crate::plan::Task {
        id: "task_legacy".into(),
        name: "Task".into(),
        kind: "implement".into(),
        model: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        test_plan: None,
        correctness: None,
        status: crate::plan::TaskStatus::Pending,
        status_label: None,
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: Some("legacy-task".into()),
        tags: vec![],
        section_ids: vec![],
        body: String::new(),
        intent: None,
        parent_task_id: None,
        assigned_pane_id: None,
        lane: Default::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: Default::default(),
        milestones: vec![crate::plan::Milestone {
            id: "ms_legacy".into(),
            from: AgentRef::from_pane("worker"),
            kind: "status_update".into(),
            text: "legacy short milestone".into(),
            ts: 1,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
        }],
    });
    let json_path = plans_dir.join(format!("{}.json", plan.id));
    std::fs::write(&json_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    store.scan_project(dir.path()).unwrap();

    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.tasks[0].milestones.len(), 1);
    assert_eq!(loaded.tasks[0].milestones[0].text, "legacy short milestone");
    let stride_log_path = plans_dir
        .join(&plan.id)
        .join("tasks/legacy-task/strides.jsonl");
    assert!(!stride_log_path.exists());

    let mut loaded = loaded;
    store.save(&mut loaded, 2).unwrap();

    let strides = crate::service::plan::stride_log::read_milestones(&stride_log_path).unwrap();
    assert_eq!(strides.len(), 1);
    assert_eq!(strides[0].id, "ms_legacy");
    assert_eq!(strides[0].text, "legacy short milestone");
    let raw_plan: Plan =
        serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
    assert!(raw_plan.tasks[0].milestones.is_empty());
}

#[test]
fn plan_file_manifest_lists_milestone_and_canvas_body_paths() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Manifest prose".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let milestone_id = store
        .add_milestone(
            &plan.id,
            &task_id,
            "review",
            &"review notes ".repeat(30),
            AgentRef::from_pane("worker"),
            true,
            3,
        )
        .unwrap();
    let decision_id = store
        .add_element(
            &plan.id,
            "decision",
            serde_json::json!("decision notes ".repeat(30)),
            vec![],
            None,
            None,
            4,
        )
        .unwrap();

    let manifest = store.file_manifest(&plan.id).unwrap();
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "stride_log"
            && entry.path == format!(".codeterm/plans/{}/tasks/build/strides.jsonl", plan.id)
    }));
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "milestone"
            && entry.path
                == format!(
                    ".codeterm/plans/{}/milestones/build-{}.md",
                    plan.id, milestone_id
                )
    }));
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "element"
            && entry.path
                == format!(
                    ".codeterm/plans/{}/elements/decision-{}.md",
                    plan.id, decision_id
                )
    }));
    let index = std::fs::read_to_string(
        dir.path()
            .join(format!(".codeterm/plans/{}/INDEX.md", plan.id)),
    )
    .unwrap();
    assert!(index.contains("## Milestones"));
    assert!(index.contains("## Stride Logs"));
    assert!(index.contains("tasks/build/strides.jsonl"));
    assert!(index.contains(&format!("milestones/build-{}.md", milestone_id)));
    assert!(index.contains("## Elements"));
    assert!(index.contains(&format!("elements/decision-{}.md", decision_id)));
}
