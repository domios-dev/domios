use super::*;
use crate::plan::review::{ReviewVerdict, DEFAULT_REVIEW_WIP_LIMIT};

#[test]
fn retired_task_status_is_refused() {
    let error = serde_json::from_str::<TaskStatus>("\"parked\"").unwrap_err();
    assert!(error.to_string().contains("unknown variant"));
    let current: TaskStatus = serde_json::from_str("\"paused\"").unwrap();
    assert_eq!(current, TaskStatus::Paused);
}

#[test]
fn plan_roundtrips_and_defaults() {
    let p = Plan::new(
        "Voice UX".into(),
        "/repo".into(),
        OrchestratorRef::from_tab("abc123"),
    );
    assert_eq!(p.status, PlanStatus::Active);
    assert!(p.sections.is_empty() && p.tasks.is_empty() && p.loops.is_empty());
    let json = serde_json::to_string(&p).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, p.id);
    assert_eq!(back.title, "Voice UX");
    assert_eq!(back.canonical_project_path, "/repo");
}

#[test]
fn task_groups_and_links_roundtrip() {
    let mut p = Plan::new("T".into(), "/repo".into(), Default::default());
    p.task_groups = vec![TaskGroup {
        key: "phase-1".into(),
        label: "Phase 1".into(),
        color: None,
    }];
    p.links = vec!["pl_other".into()];
    let json = serde_json::to_string(&p).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task_groups.len(), 1);
    assert_eq!(back.task_groups[0].key, "phase-1");
    assert_eq!(back.links, vec!["pl_other".to_string()]);
}

#[test]
fn old_json_without_new_fields_still_loads() {
    // A plan file written before task_groups / links / Task.group existed.
    let raw = r#"{
        "id": "pl_old",
        "title": "Legacy",
        "status": "active",
        "owner": {},
        "project_path": "/repo",
        "created_at": 1,
        "updated_at": 2,
        "tasks": [
            {"id":"task_1","name":"n","type":"implement","model":null,
             "model_rationale":null,"status":"pending","status_label":null,
             "section_ids":[],"body":"","intent":null,"assigned_tab_id":null,
             "peer":null}
        ]
    }"#;
    let p: Plan = serde_json::from_str(raw).unwrap();
    assert_eq!(p.id, "pl_old");
    assert!(p.canonical_project_path.is_empty());
    assert!(p.task_groups.is_empty());
    assert!(p.tag_facets.is_empty());
    assert!(p.links.is_empty());
    assert_eq!(p.tasks.len(), 1);
    assert_eq!(p.tasks[0].group, None);
    assert_eq!(p.tasks[0].priority, TaskPriority::P2);
    assert_eq!(p.tasks[0].created_at, 0);
    assert_eq!(p.tasks[0].updated_at, 0);
    assert_eq!(p.tasks[0].status_changed_at, 0);
    assert_eq!(p.tasks[0].handle, None);
    assert!(p.tasks[0].tags.is_empty());
    assert_eq!(p.tasks[0].taken_by_role, None);
    assert_eq!(p.tasks[0].executor_role, None);
    assert_eq!(p.tasks[0].parent_task_id, None);
    assert!(p.tasks[0].depends_on.is_empty());
}

#[test]
fn executor_role_roundtrips() {
    let raw = r#"{
        "id": "pl_exec",
        "title": "Exec",
        "status": "active",
        "owner": {},
        "project_path": "/repo",
        "created_at": 1,
        "updated_at": 2,
        "tasks": [
            {"id":"task_1","name":"n","type":"implement","model":null,
             "model_rationale":null,"status":"pending","status_label":null,
             "section_ids":[],"body":"","intent":null,"assigned_tab_id":null,
             "peer":null,"executor_role":"rust-backend-dev"}
        ]
    }"#;
    let p: Plan = serde_json::from_str(raw).unwrap();
    assert_eq!(
        p.tasks[0].executor_role.as_deref(),
        Some("rust-backend-dev")
    );
    let json = serde_json::to_string(&p).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.tasks[0].executor_role.as_deref(),
        Some("rust-backend-dev")
    );
}

#[test]
fn task_status_is_closed_enum() {
    let t: Result<TaskStatus, _> = serde_json::from_str("\"reviewing\"");
    assert!(t.is_err(), "unknown status must reject");
    let ok: TaskStatus = serde_json::from_str("\"in_progress\"").unwrap();
    assert_eq!(ok, TaskStatus::InProgress);
    let ok: TaskStatus = serde_json::from_str("\"in_review\"").unwrap();
    assert_eq!(ok, TaskStatus::InReview);
}

#[test]
fn task_priority_is_closed_enum() {
    let bad: Result<TaskPriority, _> = serde_json::from_str("\"urgent\"");
    assert!(bad.is_err(), "unknown priority must reject");
    let ok: TaskPriority = serde_json::from_str("\"p1\"").unwrap();
    assert_eq!(ok, TaskPriority::P1);
}

#[test]
fn planner_ref_tab_roundtrips() {
    let mut p = Plan::new("P".into(), "/r".into(), Default::default());
    p.planner = Some(PlannerRef::Tab {
        tab_id: "tab_plan".into(),
        session_id: Some("sid".into()),
        title: None,
    });
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"kind\":\"tab\""));
    assert!(json.contains("\"tab_id\":\"tab_plan\""));
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.planner, p.planner);
}

#[test]
fn planner_ref_waived_roundtrips() {
    let mut p = Plan::new("P".into(), "/r".into(), Default::default());
    p.planner = Some(PlannerRef::Waived {
        note: "solo — no review pairing needed".into(),
    });
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"kind\":\"waived\""));
    assert!(json.contains("solo"));
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.planner, p.planner);
}

#[test]
fn planner_ref_defaults_none_on_old_plan() {
    let raw = r#"{
        "id": "pl_old2",
        "title": "Old",
        "status": "active",
        "owner": {},
        "project_path": "/repo",
        "created_at": 1,
        "updated_at": 2
    }"#;
    let p: Plan = serde_json::from_str(raw).unwrap();
    assert_eq!(p.planner, None);
}

#[test]
fn legacy_loopspec_json_loads_without_new_interpretation() {
    let raw = r#"{
        "id":"loop_1",
        "target":"accepted target",
        "budget":3,
        "count":1,
        "stop_when":"oracle holds",
        "status":"running"
    }"#;
    let loop_spec: LoopSpec = serde_json::from_str(raw).unwrap();
    assert_eq!(loop_spec.id, "loop_1");
    assert_eq!(loop_spec.target, "accepted target");
    assert_eq!(loop_spec.budget, 3);
    assert_eq!(loop_spec.count, 1);
    assert_eq!(loop_spec.stop_when.as_deref(), Some("oracle holds"));
    assert_eq!(loop_spec.status, LoopStatus::Running);
    assert_eq!(loop_spec.stagnation, 0);
    assert_eq!(loop_spec.deviation, None);
    assert_eq!(loop_spec.last_iteration_at, None);

    let json = serde_json::to_string(&loop_spec).unwrap();
    let back: LoopSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back, loop_spec);
}

#[test]
fn planning_state_defaults_drafting_on_old_plan() {
    let raw = r#"{
        "id": "pl_old3",
        "title": "Old",
        "status": "active",
        "owner": {},
        "project_path": "/repo",
        "created_at": 1,
        "updated_at": 2
    }"#;
    let p: Plan = serde_json::from_str(raw).unwrap();
    assert_eq!(
        p.planning_state,
        super::super::planning_state::PlanningState::Drafting
    );
    assert!(p.planning_log.is_empty());
}

#[test]
fn old_plan_without_review_policy_defaults_to_wip_5() {
    let raw = r#"{
        "id": "pl_old4",
        "title": "Old",
        "status": "active",
        "owner": {},
        "project_path": "/repo",
        "created_at": 1,
        "updated_at": 2,
        "tasks": [
            {"id":"task_1","name":"n","type":"implement","model":null,
             "model_rationale":null,"status":"in_review","status_label":null,
             "section_ids":[],"body":"","intent":null,"assigned_tab_id":null,
             "peer":null}
        ]
    }"#;
    let p: Plan = serde_json::from_str(raw).unwrap();
    assert_eq!(p.review_policy.wip_limit, DEFAULT_REVIEW_WIP_LIMIT);
    assert_eq!(p.review_policy.credit, 0);
    assert_eq!(p.tasks[0].review_round, 0);
    assert_eq!(p.tasks[0].review_oracle, None);
    assert!(p.tasks[0].review_evidence.is_empty());
    assert_eq!(p.tasks[0].review_override, None);
    assert!(p.tasks[0].review_audit.is_empty());
    assert!(p.tasks[0].execution_intent.requires_repository());
}

#[test]
fn review_evidence_and_policy_roundtrip() {
    let mut p = Plan::new("P".into(), "/r".into(), Default::default());
    p.review_policy = ReviewPolicy {
        wip_limit: 3,
        credit: -2,
        capacity_grants: vec![],
    };
    let mut t = Task {
        files: Default::default(),
        id: "task_1".into(),
        name: "n".into(),
        kind: "implement".into(),
        model: None,
        speed: None,
        model_rationale: None,
        personality: None,
        personality_provenance: None,
        acceptance_criteria: vec!["ac".into()],
        external_refs: vec![],
        test_plan: Some("Oracle: run x".into()),
        qa_plan: None,
        correctness: None,
        status: TaskStatus::InReview,
        review_stage: ReviewStage::Review,
        qa_state: None,
        review_round: 1,
        review_oracle: Some("sig".into()),
        review_evidence: vec![ReviewEvidence {
            id: "rev_1".into(),
            from: OrchestratorRef::from_tab("qa1"),
            verdict: ReviewVerdict::Bounce,
            note: "needs fix".into(),
            at: 5,
            changed_executable_surfaces: false,
            distinct_oracle: false,
        }],
        key: None,
        review_override: Some(ReviewOverride {
            owner: "andrey".into(),
            reason: "P0 hotfix".into(),
            reproduction: "steps to repro".into(),
            at: 6,
        }),
        review_audit: vec![ReviewAuditEntry {
            id: "aud_1".into(),
            owner: "andrey".into(),
            reason: "P0 hotfix".into(),
            reproduction: "steps to repro".into(),
            round: 2,
            at: 7,
        }],
        status_label: None,
        priority: TaskPriority::P1,
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
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
        lane: TaskLaneMetadata::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: ElementCanvas::default(),
        milestones: vec![],
        execution_attempts: vec![],
        pause_reason: None,
        interruption: Default::default(),
        attachments: vec![],
    };
    t.review_round = 1;
    p.tasks.push(t);
    let json = serde_json::to_string(&p).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.review_policy, p.review_policy);
    assert_eq!(back.tasks[0].review_evidence, p.tasks[0].review_evidence);
    assert_eq!(back.tasks[0].review_override, p.tasks[0].review_override);
    assert_eq!(back.tasks[0].review_audit, p.tasks[0].review_audit);
}

#[test]
fn planning_state_roundtrips() {
    use super::super::planning_state::{PlanningAuthor, PlanningState, PlanningTransition};
    let mut p = Plan::new("P".into(), "/r".into(), Default::default());
    p.planning_state = PlanningState::Approved;
    p.planning_log = vec![PlanningTransition {
        from: PlanningState::StructureReady,
        to: PlanningState::Approved,
        by: PlanningAuthor::User,
        note: None,
        at: 9,
    }];
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"planning_state\":\"approved\""));
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.planning_state, PlanningState::Approved);
    assert_eq!(back.planning_log, p.planning_log);
}
