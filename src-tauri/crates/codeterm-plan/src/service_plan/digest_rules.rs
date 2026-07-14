use crate::plan::{Plan, TaskStatus};
use codeterm_types::config::schema::{
    DigestCondition, DigestRuleConfig, OrchestrationDigestsConfig,
};

pub const DEFAULT_IN_REVIEW_THRESHOLD: u32 = 3;
pub const DEFAULT_IN_PROGRESS_THRESHOLD: u32 = 8;
pub const DEFAULT_STALLED_LANE_THRESHOLD: u32 = 1;
pub const DEFAULT_DIGEST_RATE_LIMIT_SECS: u64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDigestCounters {
    pub plan_id: String,
    pub in_review: usize,
    pub in_progress: usize,
    pub oldest_in_review_age_ms: u64,
    pub stalled_lanes: usize,
}

pub fn plan_digest_counters(plan: &Plan, now_ms: u64, stalled_lanes: usize) -> PlanDigestCounters {
    let in_review_tasks: Vec<_> = plan
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::InReview)
        .collect();
    let oldest_in_review_age_ms = in_review_tasks
        .iter()
        .map(|t| now_ms.saturating_sub(t.status_changed_at))
        .max()
        .unwrap_or(0);
    PlanDigestCounters {
        plan_id: plan.id.clone(),
        in_review: in_review_tasks.len(),
        in_progress: plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::InProgress)
            .count(),
        oldest_in_review_age_ms,
        stalled_lanes,
    }
}

pub fn evaluate_digest_rules<'a>(
    cfg: &'a OrchestrationDigestsConfig,
    counters: &PlanDigestCounters,
) -> Vec<(&'a DigestRuleConfig, String)> {
    if !cfg.enabled {
        return Vec::new();
    }
    cfg.rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter(|&rule| rule_fires(rule, counters))
        .map(|rule| (rule, render_digest_template(rule, counters)))
        .collect()
}

fn rule_fires(rule: &DigestRuleConfig, counters: &PlanDigestCounters) -> bool {
    match rule.condition {
        DigestCondition::InReviewGt => counters.in_review > rule.threshold as usize,
        DigestCondition::InProgressPressure => {
            counters.in_progress > rule.threshold as usize
                || counters.stalled_lanes >= rule.stalled_threshold.max(1) as usize
        }
    }
}

fn render_digest_template(rule: &DigestRuleConfig, counters: &PlanDigestCounters) -> String {
    let oldest_h = counters.oldest_in_review_age_ms / 3_600_000;
    rule.template
        .replace("{plan_id}", &counters.plan_id)
        .replace("{count}", &counters.in_review.to_string())
        .replace("{oldest_h}", &oldest_h.to_string())
        .replace("{in_progress}", &counters.in_progress.to_string())
        .replace("{stalled}", &counters.stalled_lanes.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{
        OrchestratorRef, Plan, QaState, ReviewStage, Task, TaskLaneMetadata, TaskPriority,
        TaskStatus,
    };
    use codeterm_types::config::schema::OrchestrationDigestsConfig;

    fn minimal_task(id: &str, name: &str, status: TaskStatus, status_changed_at: u64) -> Task {
        Task {
            id: id.into(),
            name: name.into(),
            kind: "implement".into(),
            model: None,
            model_rationale: None,
            personality: None,
            personality_provenance: None,
            acceptance_criteria: vec![],
            test_plan: None,
            correctness: None,
            status,
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
            status_changed_at,
            handle: None,
            tags: vec![],
            section_ids: vec![],
            body: String::new(),
            role_guidance: None,
            executor_role: None,
            competition: None,
            intent: None,
            parent_task_id: None,
            depends_on: vec![],
            assigned_pane_id: None,
            lane: TaskLaneMetadata::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: Default::default(),
            milestones: vec![],
            execution_attempts: vec![],
            pause_reason: None,
            attachments: vec![],
        }
    }

    fn plan_with_counts(in_review: usize, in_progress: usize) -> Plan {
        let mut plan = Plan::new("P".into(), "/repo".into(), OrchestratorRef::default());
        plan.id = "pl_test".into();
        for i in 0..in_review {
            plan.tasks.push(minimal_task(
                &format!("task_r{i}"),
                &format!("Review {i}"),
                TaskStatus::InReview,
                3_600_000 * (i as u64 + 1),
            ));
        }
        for i in 0..in_progress {
            plan.tasks.push(minimal_task(
                &format!("task_p{i}"),
                &format!("Progress {i}"),
                TaskStatus::InProgress,
                0,
            ));
        }
        plan
    }

    #[test]
    fn in_review_rule_fires_above_threshold_and_renders_oldest() {
        let cfg = OrchestrationDigestsConfig::default();
        let plan = plan_with_counts(5, 0);
        let counters = plan_digest_counters(&plan, 10 * 3_600_000, 0);
        let fired = evaluate_digest_rules(&cfg, &counters);
        assert_eq!(fired.len(), 1);
        assert!(fired[0].1.contains("5 reports await"));
        assert!(fired[0].1.contains("oldest 9h"));
        assert!(fired[0].1.contains("pl_test"));
    }

    #[test]
    fn rules_silent_when_healthy() {
        let cfg = OrchestrationDigestsConfig::default();
        let plan = plan_with_counts(2, 3);
        let counters = plan_digest_counters(&plan, 0, 0);
        assert!(evaluate_digest_rules(&cfg, &counters).is_empty());
    }

    #[test]
    fn in_progress_pressure_fires_on_stalled_lanes() {
        let cfg = OrchestrationDigestsConfig::default();
        let plan = plan_with_counts(0, 2);
        let counters = plan_digest_counters(&plan, 0, 2);
        let fired = evaluate_digest_rules(&cfg, &counters);
        assert_eq!(fired.len(), 1);
        assert!(fired[0].1.contains("2 stalled"));
    }
}
