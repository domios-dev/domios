use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, Task, TaskStatus};

fn plan_dir(store: &PlanStore, plan_id: &str) -> std::path::PathBuf {
    let plan = store.get(plan_id).unwrap();
    store_v2::plan_state_dir(&plan).unwrap()
}

fn reload_cleared(store: &PlanStore, plan_id: &str) -> Plan {
    store.plans.write().clear();
    store.reload_from_disk(plan_id).unwrap();
    store.get(plan_id).unwrap()
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
        files: Default::default(),
        id: "task_impl".into(),
        name: "Implement".into(),
        kind: "implement".into(),
        model: None,
        speed: None,
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
        key: None,
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
    store.save(&mut plan, 1).unwrap();
    let database = store.database_path_for_test();
    let json_path = paths::plan_json_path(&project, &plan.status, &plan.id).unwrap();
    let section_path = json_path.parent().unwrap().join("sections/spec.md");
    std::fs::create_dir_all(section_path.parent().unwrap()).unwrap();
    std::fs::write(&section_path, [0xff, 0xfe, b'#']).unwrap();

    drop(store);
    let store = PlanStore::open(database).unwrap();
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
    assert_eq!(std::fs::read(section_path).unwrap(), [0xff, 0xfe, b'#']);
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
    let root = crate::service_plan::paths::plan_state_dir_unchecked(
        crate::service_plan::paths::plan_sidecar_root(&plan),
        &plan.status,
        &plan.id,
    )
    .to_string_lossy()
    .replace(std::path::MAIN_SEPARATOR, "/");
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "stride_log" && entry.path == format!("{root}/tasks/build/strides.jsonl")
    }));
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "milestone"
            && entry.path == format!("{root}/milestones/build-{milestone_id}.md")
    }));
    assert!(manifest.entries.iter().any(|entry| {
        entry.kind == "element"
            && entry.path == format!("{root}/elements/decision-{decision_id}.md")
    }));
}
