use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DigestCondition {
    #[default]
    InReviewGt,
    InProgressPressure,
    JobsGt,
    AgentTabsGt,
    IdleUtilityShellsGt,
    FinishedLanesGt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DigestRuleConfig {
    pub id: String,
    pub enabled: bool,
    pub condition: DigestCondition,
    pub threshold: u32,
    pub stalled_threshold: u32,
    pub rate_limit_secs: u64,
    pub template: String,
}

impl Default for DigestRuleConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: false,
            condition: DigestCondition::InReviewGt,
            threshold: 10,
            stalled_threshold: 1,
            rate_limit_secs: 3600,
            template: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrchestrationDigestsConfig {
    pub enabled: bool,
    pub rules: Vec<DigestRuleConfig>,
}

fn default_in_review_digest_template() -> String {
    "You have a lot of in_review tasks ({in_review}) — time to clean up: `codeterm plan review accept {plan_id} <task> --note-from-pad <note>` or `codeterm plan review bounce {plan_id} <task> --note-from-pad <note>`".into()
}

fn default_in_progress_digest_template() -> String {
    "{in_progress} lanes in flight ({stalled} stalled) — run `codeterm plan in-progress {plan_id}` then `codeterm plan reconcile {plan_id}`".into()
}

pub fn default_digest_rules() -> Vec<DigestRuleConfig> {
    vec![
        DigestRuleConfig {
            id: "in_review_pressure".into(),
            enabled: true,
            condition: DigestCondition::InReviewGt,
            threshold: 10,
            stalled_threshold: 0,
            rate_limit_secs: 3600,
            template: default_in_review_digest_template(),
        },
        DigestRuleConfig {
            id: "jobs_pressure".into(),
            enabled: true,
            condition: DigestCondition::JobsGt,
            threshold: 10,
            template: "You have a lot of jobs ({jobs}) — time to clean up: `codeterm blueprint delete --stopped`; delete unused crons with `codeterm cron delete <id>`".into(),
            ..DigestRuleConfig::default()
        },
        DigestRuleConfig {
            id: "agent_tabs_pressure".into(),
            enabled: true,
            condition: DigestCondition::AgentTabsGt,
            threshold: 10,
            template: "You have a lot of Workers/agent tabs ({agent_tabs}) — time to clean up: `codeterm tab close --tab <id>`; idle done/accepted Workers: {idle_workers}".into(),
            ..DigestRuleConfig::default()
        },
        DigestRuleConfig {
            id: "idle_utility_shells_pressure".into(),
            enabled: true,
            condition: DigestCondition::IdleUtilityShellsGt,
            threshold: 10,
            template: "You have a lot of idle utility shells ({idle_utility_shells}) — time to clean up: `codeterm tab close --tab <id>`".into(),
            ..DigestRuleConfig::default()
        },
        DigestRuleConfig {
            id: "finished_lanes_pressure".into(),
            enabled: true,
            condition: DigestCondition::FinishedLanesGt,
            threshold: 10,
            template: "You have a lot of finished-task lanes ({finished_lanes}) — time to clean up: {lane_candidates}; inspect `codeterm plan state {plan_id}`; only remove clean ancestry-merged lanes with `codeterm plan lane-remove <path> --branch <lane>` after `git merge-base --is-ancestor <lane-tip> <target-tip>` succeeds; retain dirty/unmerged lanes".into(),
            ..DigestRuleConfig::default()
        },
        DigestRuleConfig {
            id: "in_progress_pressure".into(),
            enabled: true,
            condition: DigestCondition::InProgressPressure,
            threshold: 8,
            stalled_threshold: 1,
            rate_limit_secs: 3600,
            template: default_in_progress_digest_template(),
        },
    ]
}

impl Default for OrchestrationDigestsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: default_digest_rules(),
        }
    }
}
