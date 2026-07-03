use super::*;
use crate::plan::{AgentRef, CanvasElement, Goal, OrchestratorRef, Plan};

fn markdown_body(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text;
    };
    let Some(end) = rest.find("\n---") else {
        return text;
    };
    rest[end + "\n---".len()..].trim_start_matches('\n')
}

fn assert_markdown_identity(text: &str, plan_id: &str, kind: &str, id: &str) {
    assert!(text.contains(&format!("plan_id: {plan_id}")));
    assert!(text.contains(&format!("kind: {kind}")));
    assert!(text.contains(&format!("id: {id}")));
}

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
    let task_id = store
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

    let section_path = dir.path().join(format!(
        ".codeterm/plans/active/{}/sections/spec.md",
        plan.id
    ));
    let task_path = dir.path().join(format!(
        ".codeterm/plans/active/{}/tasks/implement.md",
        plan.id
    ));
    let json_path = dir
        .path()
        .join(format!(".codeterm/plans/active/{}/plan.json", plan.id));
    let raw_json = std::fs::read_to_string(&json_path).unwrap();

    assert_eq!(
        markdown_body(&std::fs::read_to_string(&section_path).unwrap()),
        "section\nbody"
    );
    let task_markdown = std::fs::read_to_string(&task_path).unwrap();
    assert_markdown_identity(&task_markdown, &plan.id, "task", &task_id);
    assert_eq!(markdown_body(&task_markdown), "task\nbody");
    assert!(!raw_json.contains("section\\nbody"));
    assert!(!raw_json.contains("task\\nbody"));
    let index_path = dir
        .path()
        .join(format!(".codeterm/plans/active/{}/INDEX.md", plan.id));
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
fn empty_markdown_sidecars_do_not_end_with_blank_lines() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Empty sidecars".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .set_goal(
            &plan.id,
            Goal {
                vector: "Keep empty sidecars tidy".into(),
                vision: String::new(),
                constraints: vec![],
                postulates: vec![],
            },
            2,
        )
        .unwrap();
    let section_id = store
        .add_section(&plan.id, "Empty section", "", None, &[], None, None, 3)
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Empty task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let loaded = store.get(&plan.id).unwrap();
    let paths = [
        crate::service::plan::body_files::goal_path(&loaded).unwrap(),
        crate::service::plan::body_files::section_path(&loaded, &section_id).unwrap(),
        crate::service::plan::body_files::task_path(&loaded, &task_id).unwrap(),
    ];

    for path in paths {
        let markdown = std::fs::read_to_string(path).unwrap();
        assert!(markdown.ends_with("---\n"));
        assert!(!markdown.ends_with("\n\n"));
    }
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

    let plan_dir = dir
        .path()
        .join(format!(".codeterm/plans/active/{}", plan.id));
    let milestone_path = plan_dir.join(format!("milestones/build-{}.md", milestone_id));
    let stride_log_path = plan_dir.join("tasks/build/strides.jsonl");
    let note_path = plan_dir.join(format!("elements/note-{}.md", note_id));
    let milestone_markdown = std::fs::read_to_string(&milestone_path).unwrap();
    assert_markdown_identity(&milestone_markdown, &plan.id, "milestone", &milestone_id);
    assert_eq!(markdown_body(&milestone_markdown), milestone_text);
    let strides = crate::service::plan::stride_log::read_milestones(&stride_log_path).unwrap();
    assert_eq!(strides.len(), 1);
    assert_eq!(strides[0].id, milestone_id);
    assert_eq!(strides[0].text, milestone_text);
    let note_markdown = std::fs::read_to_string(&note_path).unwrap();
    assert_markdown_identity(&note_markdown, &plan.id, "element", &note_id);
    assert_eq!(markdown_body(&note_markdown), note_text);
    let raw_json = std::fs::read_to_string(
        dir.path()
            .join(format!(".codeterm/plans/active/{}/plan.json", plan.id)),
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
        .join(format!(".codeterm/plans/active/{}/plan.json", plan.id));
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
        ".codeterm/plans/active/{}/tasks/build/strides.jsonl",
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
fn save_preserves_existing_sidecar_only_stride_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Stride merge save".into(),
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
    let first_id = store
        .add_milestone(
            &plan.id,
            &task_id,
            "status_update",
            "first in-memory stride",
            AgentRef::from_pane("worker"),
            true,
            3,
        )
        .unwrap();
    let mut sidecar_only = store.get(&plan.id).unwrap().tasks[0].milestones[0].clone();
    sidecar_only.id = "ms_sidecar_only".into();
    sidecar_only.text = "manual sidecar-only stride".into();
    sidecar_only.ts = 4;
    let stride_log_path = dir.path().join(format!(
        ".codeterm/plans/active/{}/tasks/build/strides.jsonl",
        plan.id
    ));
    crate::service::plan::stride_log::append_milestone(&stride_log_path, &sidecar_only).unwrap();

    let second_id = store
        .add_milestone(
            &plan.id,
            &task_id,
            "status_update",
            "second in-memory stride",
            AgentRef::from_pane("worker"),
            true,
            5,
        )
        .unwrap();

    let strides = crate::service::plan::stride_log::read_milestones(&stride_log_path).unwrap();
    assert_eq!(
        strides
            .iter()
            .map(|milestone| milestone.id.as_str())
            .collect::<Vec<_>>(),
        vec![first_id.as_str(), "ms_sidecar_only", second_id.as_str()]
    );
    assert!(strides
        .iter()
        .any(|milestone| milestone.text == "manual sidecar-only stride"));
    let raw_plan: Plan = serde_json::from_str(
        &std::fs::read_to_string(
            dir.path()
                .join(format!(".codeterm/plans/active/{}/plan.json", plan.id)),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(raw_plan.tasks[0].milestones.is_empty());
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
        role_guidance: None,
        executor_role: None,
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
        execution_attempts: vec![],
        park_reason: None,
    });
    plan.elements.push(CanvasElement {
        id: "el_discussion".into(),
        kind: "discussion".into(),
        canvas: Default::default(),
        content: serde_json::json!({ "body": "legacy discussion\n\nbody" }),
        links: vec![],
    });
    let json_path = paths::plan_json_path(&project, &plan.status, &plan.id).unwrap();
    std::fs::write(&json_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    store.scan_project(dir.path()).unwrap();

    let plan_dir = plans_dir.join("active").join(&plan.id);
    let milestone_markdown =
        std::fs::read_to_string(plan_dir.join("milestones/legacy-task-ms_legacy.md")).unwrap();
    assert_eq!(
        markdown_body(&milestone_markdown),
        "legacy milestone\n\nbody"
    );
    let discussion_markdown =
        std::fs::read_to_string(plan_dir.join("elements/discussion-el_discussion.md")).unwrap();
    assert_eq!(
        markdown_body(&discussion_markdown),
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
        role_guidance: None,
        executor_role: None,
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
        execution_attempts: vec![],
        park_reason: None,
    });
    let json_path = paths::plan_json_path(&project, &plan.status, &plan.id).unwrap();
    std::fs::write(&json_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    store.scan_project(dir.path()).unwrap();

    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.tasks[0].milestones.len(), 1);
    assert_eq!(loaded.tasks[0].milestones[0].text, "legacy short milestone");
    let stride_log_path =
        crate::service::plan::body_files::task_stride_log_path(&loaded, &loaded.tasks[0]).unwrap();
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
            && entry.path
                == format!(
                    ".codeterm/plans/active/{}/tasks/build/strides.jsonl",
                    plan.id
                )
    }));
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "milestone"
            && entry.path
                == format!(
                    ".codeterm/plans/active/{}/milestones/build-{}.md",
                    plan.id, milestone_id
                )
    }));
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "element"
            && entry.path
                == format!(
                    ".codeterm/plans/active/{}/elements/decision-{}.md",
                    plan.id, decision_id
                )
    }));
    let index = std::fs::read_to_string(
        dir.path()
            .join(format!(".codeterm/plans/active/{}/INDEX.md", plan.id)),
    )
    .unwrap();
    assert!(index.contains("## Milestones"));
    assert!(index.contains("## Stride Logs"));
    assert!(index.contains("tasks/build/strides.jsonl"));
    assert!(index.contains(&format!("milestones/build-{}.md", milestone_id)));
    assert!(index.contains("## Elements"));
    assert!(index.contains(&format!("elements/decision-{}.md", decision_id)));
}
