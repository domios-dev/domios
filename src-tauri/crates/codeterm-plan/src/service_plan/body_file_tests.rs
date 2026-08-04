use super::*;
use crate::plan::{AgentRef, CanvasElement, Goal, OrchestratorRef, Plan, Task, TaskStatus};

fn plan_dir(store: &PlanStore, plan_id: &str) -> std::path::PathBuf {
    let plan = store.get(plan_id).unwrap();
    store_v2::plan_state_dir(&plan).unwrap()
}

fn reload_cleared(store: &PlanStore, plan_id: &str) -> Plan {
    store.plans.write().clear();
    store.reload_from_disk(plan_id).unwrap();
    store.get(plan_id).unwrap()
}

fn markdown_body(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text;
    };
    let Some(end) = rest.find("\n---") else {
        return text;
    };
    rest[end + "\n---".len()..].trim_start_matches('\n')
}

#[test]
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn section_and_task_bodies_roundtrip_through_markdown_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "External".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .add_section(&plan.id, "Spec", "section\nbody", None, &[], None, None, 2)
        .unwrap();
    let _task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Implement".into(),
                kind: "implement".into(),
                body: "task\nbody".into(),
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();

    let plan_state = plan_dir(&store, &plan.id);
    let task_path = store_v2::task_yaml_path(&plan_state, &store.get(&plan.id).unwrap().tasks[0]);
    let plan_path = store_v2::plan_yaml_path(&plan_state);
    let task_doc: Task =
        serde_yaml::from_str(&std::fs::read_to_string(&task_path).unwrap()).unwrap();
    let plan_doc: Plan =
        serde_yaml::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();

    assert_eq!(plan_doc.sections[0].body, "section\nbody\n");
    assert_eq!(task_doc.body, "task\nbody\n");
    assert!(plan_doc.tasks.is_empty(), "tasks live in task yaml files");
    assert!(!std::fs::read_to_string(&plan_path)
        .unwrap()
        .contains("task\\nbody"));

    let loaded = reload_cleared(&store, &plan.id);
    assert_eq!(loaded.sections[0].body, "section\nbody\n");
    assert_eq!(loaded.tasks[0].body, "task\nbody\n");
}

// Second-regression lock: executor_role must survive across plan.json, task
// sidecar frontmatter, and reload in one test (prior fixes tested layers alone).
#[test]
fn executor_role_persists_across_json_frontmatter_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Role".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Implement".into(),
                kind: "implement".into(),
                executor_role: Some("qa".into()),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let plan_state = plan_dir(&store, &plan.id);
    let task_path = store_v2::task_yaml_path(&plan_state, &store.get(&plan.id).unwrap().tasks[0]);
    let task_doc: Task =
        serde_yaml::from_str(&std::fs::read_to_string(&task_path).unwrap()).unwrap();
    assert_eq!(task_doc.executor_role.as_deref(), Some("qa"));

    let loaded = reload_cleared(&store, &plan.id);
    let task = loaded.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.executor_role.as_deref(), Some("qa"));
}

#[test]
fn personality_persists_across_yaml_sidecar_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Persona".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Implement".into(),
                kind: "implement".into(),
                personality: Some("Hopper".into()),
                personality_provenance: None,
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let plan_state = plan_dir(&store, &plan.id);
    let task_path = store_v2::task_yaml_path(&plan_state, &store.get(&plan.id).unwrap().tasks[0]);
    let task_doc: Task =
        serde_yaml::from_str(&std::fs::read_to_string(&task_path).unwrap()).unwrap();
    assert_eq!(task_doc.personality.as_deref(), Some("Hopper"));

    let loaded = reload_cleared(&store, &plan.id);
    let task = loaded.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.personality.as_deref(), Some("Hopper"));
}

#[test]
fn corrupt_markdown_sidecar_keeps_plan_indexed_with_json_body() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = Plan::new(
        "Corrupt sidecar".into(),
        project.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.sections.push(crate::plan::Section {
        id: "sec_spec".into(),
        title: "Spec".into(),
        handle: Some("spec".into()),
        tags: vec![],
        body: "json section body".into(),
        subsections: vec![],
        canvas: Default::default(),
        attachments: vec![],
    });
    plan.tasks.push(crate::plan::Task {
        id: "task_impl".into(),
        name: "Implement".into(),
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
        handle: Some("implement".into()),
        tags: vec![],
        section_ids: vec![],
        body: "json task body".into(),
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
    std::fs::create_dir_all(json_path.parent().unwrap()).unwrap();
    std::fs::write(&json_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    let section_path = json_path.parent().unwrap().join("sections/spec.md");
    std::fs::create_dir_all(section_path.parent().unwrap()).unwrap();
    std::fs::write(&section_path, [0xff, 0xfe, b'#']).unwrap();

    store.scan_project(dir.path()).unwrap();

    let loaded = store
        .get(&plan.id)
        .expect("corrupt sidecar must not skip plan");
    assert_eq!(loaded.sections[0].body, "json section body");
    assert_eq!(loaded.tasks[0].body, "json task body");
    assert!(store
        .list_index()
        .iter()
        .any(|entry| entry.plan_id == plan.id));

    store.reload_from_disk(&plan.id).unwrap();
    let reloaded = store.get(&plan.id).unwrap();
    assert_eq!(reloaded.sections[0].body.trim_end(), "json section body");
}

#[test]
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn empty_markdown_sidecars_do_not_end_with_blank_lines() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Empty sidecars".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
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
                competition: None,
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let loaded = store.get(&plan.id).unwrap();
    let paths = [
        crate::service_plan::body_files::goal_path(&loaded).unwrap(),
        crate::service_plan::body_files::section_path(&loaded, &section_id).unwrap(),
        crate::service_plan::body_files::task_path(&loaded, &task_id).unwrap(),
    ];

    for path in paths {
        let markdown = std::fs::read_to_string(path).unwrap();
        assert!(markdown.ends_with("---\n"));
        assert!(!markdown.ends_with("\n\n"));
    }
}

#[test]
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn milestone_and_note_bodies_roundtrip_through_markdown_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "External prose".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                competition: None,
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
            AgentRef::from_tab("worker"),
            true,
            3,
        )
        .unwrap();
    let note_text = "# Note\n\n".to_string() + &"long note detail ".repeat(22);
    let _note_id = store
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

    let plan_state = plan_dir(&store, &plan.id);
    let task_path = store_v2::task_yaml_path(&plan_state, &store.get(&plan.id).unwrap().tasks[0]);
    let plan_path = store_v2::plan_yaml_path(&plan_state);
    let task_doc: Task =
        serde_yaml::from_str(&std::fs::read_to_string(&task_path).unwrap()).unwrap();
    assert_eq!(task_doc.milestones.len(), 1);
    assert_eq!(task_doc.milestones[0].id, milestone_id);
    assert_eq!(task_doc.milestones[0].text, milestone_text);
    let plan_doc: Plan =
        serde_yaml::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();
    assert_eq!(plan_doc.elements[0].content["text"], note_text.trim_end());
    let raw_plan_yaml = std::fs::read_to_string(&plan_path).unwrap();
    assert!(!raw_plan_yaml.contains("long milestone detail"));
    assert!(!raw_plan_yaml.contains("long note detail"));

    let mut edited_task = task_doc;
    edited_task.milestones[0].text = "edited milestone\n\nbody".into();
    std::fs::write(&task_path, serde_yaml::to_string(&edited_task).unwrap()).unwrap();
    let mut edited_plan = plan_doc;
    edited_plan.elements[0].content = serde_json::json!({ "text": "edited note\n\nbody" });
    std::fs::write(&plan_path, serde_yaml::to_string(&edited_plan).unwrap()).unwrap();
    store.reload_from_disk(&plan.id).unwrap();
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(
        loaded.tasks[0].milestones[0].text,
        "edited milestone\n\nbody"
    );
    assert_eq!(loaded.elements[0].content["text"], "edited note\n\nbody");
}

#[test]
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn task_stride_log_hydrates_when_inline_milestones_are_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Stride hydrate".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                competition: None,
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
            AgentRef::from_tab("worker"),
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
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn inline_and_stride_log_milestones_dedupe_deterministically() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Stride dedupe".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                competition: None,
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
            AgentRef::from_tab("worker"),
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
    crate::service_plan::stride_log::append_milestone(&stride_log_path, &older).unwrap();
    crate::service_plan::stride_log::append_milestone(&stride_log_path, &other).unwrap();

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
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn save_preserves_existing_sidecar_only_stride_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Stride merge save".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                competition: None,
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
            AgentRef::from_tab("worker"),
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
    crate::service_plan::stride_log::append_milestone(&stride_log_path, &sidecar_only).unwrap();

    let second_id = store
        .add_milestone(
            &plan.id,
            &task_id,
            "status_update",
            "second in-memory stride",
            AgentRef::from_tab("worker"),
            true,
            5,
        )
        .unwrap();

    let strides = crate::service_plan::stride_log::read_milestones(&stride_log_path).unwrap();
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
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn ordinary_save_preserves_external_stride_append_after_stale_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Stride race save".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                competition: None,
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
            AgentRef::from_tab("worker"),
            true,
            3,
        )
        .unwrap();
    let mut stale_plan = store.get(&plan.id).unwrap();
    let mut external = stale_plan.tasks[0].milestones[0].clone();
    external.id = "ms_external_append".into();
    external.text = "external append during save".into();
    external.ts = 4;
    let stride_log_path = dir.path().join(format!(
        ".codeterm/plans/active/{}/tasks/build/strides.jsonl",
        plan.id
    ));
    crate::service_plan::stride_log::append_milestone(&stride_log_path, &external).unwrap();

    stale_plan.title = "Ordinary stale save".into();
    store.save(&mut stale_plan, 5).unwrap();

    let strides = crate::service_plan::stride_log::read_milestones(&stride_log_path).unwrap();
    let ids = strides
        .iter()
        .map(|milestone| milestone.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![first_id.as_str(), "ms_external_append"]);
    assert!(strides
        .iter()
        .any(|milestone| milestone.text == "external append during save"));
}

#[test]
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn inline_milestone_and_canvas_markdown_migrates_to_files_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let plans_dir = paths::plans_dir(&project).unwrap();
    let mut plan = Plan::new(
        "Legacy prose".into(),
        project.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
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
        status: crate::plan::TaskStatus::Pending,
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
        handle: Some("legacy-task".into()),
        tags: vec![],
        section_ids: vec![],
        body: String::new(),
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
        attachments: vec![],
        milestones: vec![crate::plan::Milestone {
            id: "ms_legacy".into(),
            from: AgentRef::from_tab("worker"),
            kind: "decision".into(),
            text: "legacy milestone\n\nbody".into(),
            ts: 1,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        }],
        execution_attempts: vec![],
        interruption: Default::default(),
        pause_reason: None,
        personality: None,
        personality_provenance: None,
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
        "legacy milestone\n\nbody\n"
    );
    let discussion_markdown =
        std::fs::read_to_string(plan_dir.join("elements/discussion-el_discussion.md")).unwrap();
    assert_eq!(
        markdown_body(&discussion_markdown),
        "legacy discussion\n\nbody\n"
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
#[ignore = "legacy body-file format, superseded by store_v2 yaml"]
fn legacy_inline_only_milestones_load_then_save_to_stride_log_and_stripped_json() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = Plan::new(
        "Legacy inline strides".into(),
        project.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
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
        status: crate::plan::TaskStatus::Pending,
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
        handle: Some("legacy-task".into()),
        tags: vec![],
        section_ids: vec![],
        body: String::new(),
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
        attachments: vec![],
        milestones: vec![crate::plan::Milestone {
            id: "ms_legacy".into(),
            from: AgentRef::from_tab("worker"),
            kind: "status_update".into(),
            text: "legacy short milestone".into(),
            ts: 1,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        }],
        execution_attempts: vec![],
        interruption: Default::default(),
        pause_reason: None,
        personality: None,
        personality_provenance: None,
    });
    let json_path = paths::plan_json_path(&project, &plan.status, &plan.id).unwrap();
    std::fs::write(&json_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    store.scan_project(dir.path()).unwrap();

    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.tasks[0].milestones.len(), 1);
    assert_eq!(loaded.tasks[0].milestones[0].text, "legacy short milestone");
    let stride_log_path =
        crate::service_plan::body_files::task_stride_log_path(&loaded, &loaded.tasks[0]).unwrap();
    assert!(!stride_log_path.exists());

    let mut loaded = loaded;
    store.save(&mut loaded, 2).unwrap();

    let strides = crate::service_plan::stride_log::read_milestones(&stride_log_path).unwrap();
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
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Build".into(),
                kind: "implement".into(),
                competition: None,
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
            AgentRef::from_tab("worker"),
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
}
