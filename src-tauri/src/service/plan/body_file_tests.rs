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
    let note_path = plan_dir.join(format!("elements/note-{}.md", note_id));
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_text
    );
    assert_eq!(std::fs::read_to_string(&note_path).unwrap(), note_text);
    let raw_json = std::fs::read_to_string(
        dir.path()
            .join(".codeterm/plans")
            .join(format!("{}.json", plan.id)),
    )
    .unwrap();
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
    assert!(index.contains(&format!("milestones/build-{}.md", milestone_id)));
    assert!(index.contains("## Elements"));
    assert!(index.contains(&format!("elements/decision-{}.md", decision_id)));
}
