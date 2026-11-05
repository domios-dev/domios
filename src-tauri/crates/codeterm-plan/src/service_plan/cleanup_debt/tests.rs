use crate::plan::{
    OrchestratorRef, Plan, QaState, ReviewStage, Task, TaskLaneMetadata, TaskPriority, TaskStatus,
};

use super::*;

#[test]
fn detects_terminal_task_cleanup_debt_from_lane_metadata() {
    let mut plan = Plan::new("Cleanup".into(), "/repo".into(), OrchestratorRef::default());
    let task = Task {
        files: Default::default(),
        id: "task_1".into(),
        name: "Worker lane".into(),
        kind: "implement".into(),
        model: None,
        speed: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        external_refs: vec![],
        test_plan: None,
        qa_plan: None,
        correctness: None,
        status: TaskStatus::Done,
        review_stage: ReviewStage::Review,
        qa_state: Option::<QaState>::None,
        review_round: 0,
        review_oracle: None,
        review_evidence: vec![],
        key: None,
        review_override: None,
        review_audit: vec![],
        status_label: None,
        priority: TaskPriority::P2,
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
        lane: TaskLaneMetadata {
            tab_id: Some("tab-1".into()),
            worktree_path: Some("/tmp/lane".into()),
            branch: Some("lane/x".into()),
            cleanup_hints: vec!["waiting for integration".into()],
            ..Default::default()
        },
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
    };
    plan.tasks.push(task);
    let items = detect_plan_cleanup_debt(&plan, |tab| tab == "tab-1");
    assert_eq!(items.len(), 1);
    assert!(items[0]
        .issues
        .iter()
        .any(|issue| issue.detail.contains("worker tab tab-1")));
}
