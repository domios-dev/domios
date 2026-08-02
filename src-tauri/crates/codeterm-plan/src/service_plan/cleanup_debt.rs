use std::path::Path;

use crate::plan::cleanup_debt::{
    detect_cleanup_debt, render_cleanup_debt_hint, CleanupDebtInput, CleanupDebtTask,
};
use crate::plan::Plan;

fn plan_project_path(plan: &Plan) -> &str {
    if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    }
}

pub fn detect_plan_cleanup_debt<F>(plan: &Plan, mut tab_live: F) -> Vec<CleanupDebtTask>
where
    F: FnMut(&str) -> bool,
{
    plan.tasks
        .iter()
        .filter_map(|task| {
            let tab_id = task
                .lane
                .tab_id
                .as_deref()
                .or(task.assigned_tab_id.as_deref());
            let tab_live = tab_id.map(|tab| tab_live(tab)).unwrap_or(false);
            let worktree_path = task.lane.worktree_path.as_deref();
            let worktree_exists = worktree_path.is_some_and(|path| Path::new(path).exists());
            detect_cleanup_debt(&CleanupDebtInput {
                plan_id: &plan.id,
                task_id: &task.id,
                task_name: &task.name,
                task_status: task_status_label(&task.status),
                is_terminal: task.status.is_terminal(),
                cleanup_status: task.lane.cleanup_status.as_deref(),
                cleanup_hints: &task.lane.cleanup_hints,
                tab_id,
                tab_live,
                worktree_path,
                worktree_exists,
                branch: task.lane.branch.as_deref(),
                project_path: plan_project_path(plan),
            })
        })
        .collect()
}

pub fn cleanup_debt_hints(items: &[CleanupDebtTask]) -> Vec<String> {
    items.iter().map(render_cleanup_debt_hint).collect()
}

fn task_status_label(status: &crate::plan::TaskStatus) -> &'static str {
    match status {
        crate::plan::TaskStatus::Pending => "pending",
        crate::plan::TaskStatus::InProgress => "in_progress",
        crate::plan::TaskStatus::InReview => "in_review",
        crate::plan::TaskStatus::Blocked => "blocked",
        crate::plan::TaskStatus::Paused => "paused",
        crate::plan::TaskStatus::Done => "done",
        crate::plan::TaskStatus::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use crate::plan::{
        OrchestratorRef, Plan, QaState, ReviewStage, Task, TaskLaneMetadata, TaskPriority,
        TaskStatus,
    };

    use super::*;

    #[test]
    fn detects_terminal_task_cleanup_debt_from_lane_metadata() {
        let mut plan = Plan::new("Cleanup".into(), "/repo".into(), OrchestratorRef::default());
        let task = Task {
            id: "task_1".into(),
            name: "Worker lane".into(),
            kind: "implement".into(),
            model: None,
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
}
