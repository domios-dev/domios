use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::{NewTask, PlanStore};

#[test]
fn apply_tags_replaces_same_facet_key() {
    let mut tags = vec!["area:nav".into(), "pan-zoom".into()];
    apply_tags(&mut tags, &["area:ui".into()], Some("area:ui"));
    assert_eq!(tags, vec!["area:ui", "pan-zoom"]);
}

#[test]
fn apply_tags_primary_only_swaps_facet_value() {
    let mut tags = vec!["area:nav".into(), "pan-zoom".into()];
    apply_tags(&mut tags, &[], Some("area:layout"));
    assert_eq!(tags, vec!["area:layout", "pan-zoom"]);
}

#[test]
fn inherit_parent_tags_keeps_parent_and_lets_child_replace_facets() {
    let parent: Vec<String> = vec!["work:backend".into(), "area:plan".into(), "topic".into()];
    assert_eq!(inherit_parent_tags(&parent, &[]), parent);
    assert_eq!(
        inherit_parent_tags(&parent, &["work:qa".into(), "extra".into()]),
        vec!["area:plan", "topic", "work:qa", "extra"]
    );
}

#[test]
fn preview_handle_matches_normalized_seed() {
    let tags = vec!["area:nav".into(), "pan-zoom".into()];
    assert_eq!(preview_handle(&tags, "Pan Zoom"), "nav-pan-zoom");
}

#[test]
fn slug_is_capped_so_filenames_stay_under_os_limit() {
    let long = "PERF backend profiling found plugin manager snapshot IPC takes seconds and freezes the whole UI during pan and zoom which is terrible".repeat(2);
    let s = slug(&long);
    assert!(s.len() <= 72, "slug must cap length, got {}", s.len());
    assert!(!s.ends_with('-'), "no trailing dash");
    assert_eq!(slug("Pan Zoom"), "pan-zoom");
}

#[test]
fn normalize_caps_legacy_stored_handles() {
    let mut plan = Plan::new("P".into(), "/repo".into(), OrchestratorRef::default());
    plan.tasks.push(crate::plan::Task {
        files: Default::default(),
        id: "task_1".into(),
        handle: Some("legacy-uncapped-handle-".repeat(10)),
        ..task_fixture()
    });
    normalize_plan(&mut plan);
    let handle = plan.tasks[0].handle.as_deref().unwrap();
    assert!(
        handle.len() <= 72,
        "stored handle must be re-capped, got {}",
        handle.len()
    );
}

#[test]
fn generated_handles_use_primary_facet_and_topic() {
    let mut plan = Plan::new("P".into(), "/repo".into(), OrchestratorRef::default());
    plan.tasks.push(task_fixture());
    normalize_plan(&mut plan);
    assert_eq!(plan.tasks[0].handle.as_deref(), Some("nav-pan-zoom"));
}

#[test]
fn renaming_task_keeps_existing_handle() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Handles".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "original title".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let before = store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task
        .handle
        .unwrap();

    store
        .update_task(
            &plan.id,
            &task_id,
            Some("renamed completely different title"),
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
            3,
        )
        .unwrap();

    let after = store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task
        .handle
        .unwrap();
    assert_eq!(after, before);
}

fn task_fixture() -> crate::plan::Task {
    crate::plan::Task {
        files: Default::default(),
        id: "task_1".into(),
        name: "Pan Zoom".into(),
        kind: "implement".into(),
        model: None,
        speed: None,
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
        key: None,
        review_override: None,
        review_audit: vec![],
        status_label: None,
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
        tags: vec!["area:nav".into(), "pan-zoom".into()],
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
        milestones: vec![],
        execution_attempts: vec![],
        interruption: Default::default(),
        pause_reason: None,
        personality: None,
        personality_provenance: None,
        attachments: vec![],
    }
}
