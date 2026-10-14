use super::*;
use crate::plan::{
    OrchestratorRef, Plan, QaState, ReviewStage, Task, TaskLaneMetadata, TaskPriority, TaskStatus,
};
use codeterm_types::config::schema::OrchestrationDigestsConfig;

pub(super) fn minimal_task(
    id: &str,
    name: &str,
    status: TaskStatus,
    status_changed_at: u64,
) -> Task {
    Task {
        files: Default::default(),
        id: id.into(),
        name: name.into(),
        kind: "implement".into(),
        model: None,
        model_rationale: None,
        personality: None,
        personality_provenance: None,
        acceptance_criteria: vec![],
        external_refs: vec![],
        test_plan: None,
        qa_plan: None,
        correctness: None,
        status,
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
        status_changed_at,
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
        canvas: Default::default(),
        milestones: vec![],
        execution_attempts: vec![],
        interruption: Default::default(),
        pause_reason: None,
        attachments: vec![],
    }
}

pub(super) fn plan_with_counts(in_review: usize, in_progress: usize) -> Plan {
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
    let plan = plan_with_counts(11, 0);
    let counters = plan_digest_counters(&plan, 10 * 3_600_000, 0);
    let fired = evaluate_digest_rules(&cfg, &counters);
    assert_eq!(fired.len(), 1);
    assert_eq!(counters.in_review, 11);
    assert_eq!(counters.oldest_in_review_age_ms, 9 * 3_600_000);
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
    assert_eq!(fired[0].0.condition, DigestCondition::InProgressPressure);
}

#[test]
fn cleanup_rules_use_strict_configurable_thresholds() {
    let cfg = OrchestrationDigestsConfig::default();
    let cleanup: Vec<_> = cfg
        .rules
        .iter()
        .filter(|rule| rule.condition != DigestCondition::InProgressPressure)
        .collect();
    assert_eq!(cleanup.len(), 5);
    for rule in cleanup {
        assert_eq!(rule.threshold, 10);
        for threshold in [10, 4] {
            let rule = DigestRuleConfig {
                threshold,
                ..rule.clone()
            };
            for count in [threshold - 1, threshold, threshold + 1] {
                let mut counters = plan_digest_counters(&plan_with_counts(count as usize, 0), 0, 0);
                counters.jobs = count as usize;
                counters.agent_tabs = count as usize;
                counters.idle_utility_shells = count as usize;
                counters.finished_lanes = count as usize;
                assert_eq!(
                    rule_fires(&rule, &counters),
                    count > threshold,
                    "{:?}: {count}/{threshold}",
                    rule.condition
                );
            }
        }
    }
}

#[test]
fn cleanup_templates_substitute_counts_and_candidate_workers() {
    let mut counters = plan_digest_counters(&plan_with_counts(11, 2), 3_600_000, 3);
    counters.jobs = 12;
    counters.agent_tabs = 13;
    counters.idle_utility_shells = 14;
    counters.finished_lanes = 15;
    counters.lane_candidates = vec!["opaque-lane".into()];
    counters.idle_workers = vec!["opaque-worker (opaque-id)".into()];
    for (condition, count) in [
        (DigestCondition::InReviewGt, 11),
        (DigestCondition::JobsGt, 12),
        (DigestCondition::AgentTabsGt, 13),
        (DigestCondition::IdleUtilityShellsGt, 14),
        (DigestCondition::FinishedLanesGt, 15),
    ] {
        let rule = DigestRuleConfig {
            condition,
            template: "{plan_id}|{count}|{in_review}|{jobs}|{agent_tabs}|{idle_utility_shells}|{idle_workers}|{in_progress}|{stalled}|{oldest_h}|{finished_lanes}|{lane_candidates}".into(),
            ..Default::default()
        };
        assert_eq!(
            render_digest_template(&rule, &counters),
            format!("pl_test|{count}|11|12|13|14|opaque-worker (opaque-id)|2|3|0|15|opaque-lane")
        );
    }
}

#[test]
fn cleanup_rules_share_existing_per_rule_rate_limit() {
    let counter = crate::service_plan::reminders::TurnCounter::default();
    let cfg = OrchestrationDigestsConfig::default();
    for rule in cfg
        .rules
        .iter()
        .filter(|rule| rule.condition != DigestCondition::InProgressPressure)
    {
        assert!(counter.digest_rule_due("owner", &rule.id, 1000, rule.rate_limit_secs));
        counter.mark_digest_rule_fired("owner", &rule.id, 1000);
        assert!(!counter.digest_rule_due(
            "owner",
            &rule.id,
            1000 + rule.rate_limit_secs - 1,
            rule.rate_limit_secs
        ));
        assert!(counter.digest_rule_due(
            "owner",
            &rule.id,
            1000 + rule.rate_limit_secs,
            rule.rate_limit_secs
        ));
        assert!(counter.digest_rule_due("other-owner", &rule.id, 1000, rule.rate_limit_secs));
    }
}

#[test]
fn disabled_cleanup_rules_do_not_render() {
    let mut cfg = OrchestrationDigestsConfig::default();
    let mut counters = plan_digest_counters(&plan_with_counts(11, 0), 0, 0);
    counters.jobs = 11;
    counters.agent_tabs = 11;
    counters.idle_utility_shells = 11;
    counters.finished_lanes = 11;
    for rule in &mut cfg.rules {
        rule.enabled = false;
    }
    assert!(evaluate_digest_rules(&cfg, &counters).is_empty());
    for rule in &mut cfg.rules {
        rule.enabled = true;
    }
    cfg.enabled = false;
    assert!(evaluate_digest_rules(&cfg, &counters).is_empty());
}
