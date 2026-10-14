mod lanes;

use crate::plan::{Plan, TaskStatus};
use codeterm_types::config::schema::{
    DigestCondition, DigestRuleConfig, OrchestrationDigestsConfig,
};

pub const DEFAULT_IN_REVIEW_THRESHOLD: u32 = 10;
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
    pub jobs: usize,
    pub agent_tabs: usize,
    pub idle_utility_shells: usize,
    pub idle_workers: Vec<String>,
    pub finished_lanes: usize,
    pub lane_candidates: Vec<String>,
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
    let (finished_lanes, lane_candidates) = lanes::recorded_finished_lanes(plan);
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
        jobs: 0,
        agent_tabs: 0,
        idle_utility_shells: 0,
        idle_workers: Vec::new(),
        finished_lanes,
        lane_candidates,
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
        DigestCondition::JobsGt => counters.jobs > rule.threshold as usize,
        DigestCondition::FinishedLanesGt => counters.finished_lanes > rule.threshold as usize,
        DigestCondition::AgentTabsGt => counters.agent_tabs > rule.threshold as usize,
        DigestCondition::IdleUtilityShellsGt => {
            counters.idle_utility_shells > rule.threshold as usize
        }
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
        .replace(
            "{count}",
            &match rule.condition {
                DigestCondition::JobsGt => counters.jobs,
                DigestCondition::FinishedLanesGt => counters.finished_lanes,
                DigestCondition::AgentTabsGt => counters.agent_tabs,
                DigestCondition::IdleUtilityShellsGt => counters.idle_utility_shells,
                _ => counters.in_review,
            }
            .to_string(),
        )
        .replace("{finished_lanes}", &counters.finished_lanes.to_string())
        .replace("{lane_candidates}", &counters.lane_candidates.join(", "))
        .replace("{in_review}", &counters.in_review.to_string())
        .replace("{jobs}", &counters.jobs.to_string())
        .replace("{agent_tabs}", &counters.agent_tabs.to_string())
        .replace(
            "{idle_utility_shells}",
            &counters.idle_utility_shells.to_string(),
        )
        .replace(
            "{idle_workers}",
            &if counters.idle_workers.is_empty() {
                "none".into()
            } else {
                counters.idle_workers.join(", ")
            },
        )
        .replace("{oldest_h}", &oldest_h.to_string())
        .replace("{in_progress}", &counters.in_progress.to_string())
        .replace("{stalled}", &counters.stalled_lanes.to_string())
}

#[cfg(test)]
mod tests;
