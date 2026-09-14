use serde::{Deserialize, Serialize};

use super::types::{AgentRef, PlanMode};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanPauseRole {
    Orchestrator,
    Planner,
    Worker,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanPauseEffect {
    PlanReminders,
    HeartbeatNudges,
    SystemNotifications,
    TeamSchedules,
}

pub const PLAN_PAUSE_EFFECTS: [PlanPauseEffect; 4] = [
    PlanPauseEffect::PlanReminders,
    PlanPauseEffect::HeartbeatNudges,
    PlanPauseEffect::SystemNotifications,
    PlanPauseEffect::TeamSchedules,
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PlanPauseStepOutcome {
    #[default]
    Pending,
    Delivered,
    Disabled,
    Enabled,
    Unavailable,
    Skipped {
        reason: String,
    },
    Failed {
        error: String,
    },
}

impl PlanPauseStepOutcome {
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Pending)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanPauseAgent {
    pub tab_id: String,
    pub name: String,
    pub role: PlanPauseRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pause_notice: PlanPauseStepOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_notice: Option<PlanPauseStepOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanPauseSchedule {
    pub job_id: String,
    pub name: String,
    pub prior_enabled: bool,
    #[serde(default)]
    pub pause_outcome: PlanPauseStepOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_outcome: Option<PlanPauseStepOutcome>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanPauseStage {
    Applying,
    Complete,
    Incomplete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanResumeRecord {
    pub resumed_at: u64,
    pub restored_mode: PlanMode,
    pub stage: PlanPauseStage,
}

/// One pause operation on a plan. Immutable once its resume is recorded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanPauseOperation {
    pub id: String,
    pub paused_at: u64,
    pub paused_from_mode: PlanMode,
    pub reason: String,
    #[serde(default)]
    pub from: AgentRef,
    #[serde(default)]
    pub agents: Vec<PlanPauseAgent>,
    #[serde(default)]
    pub schedules: Vec<PlanPauseSchedule>,
    #[serde(default)]
    pub effects: Vec<PlanPauseEffect>,
    pub stage: PlanPauseStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<PlanResumeRecord>,
}

impl PlanPauseOperation {
    pub fn pause_stage_from_outcomes(&self) -> PlanPauseStage {
        let failed = self.agents.iter().any(|a| a.pause_notice.is_failure())
            || self.schedules.iter().any(|s| s.pause_outcome.is_failure());
        if failed {
            PlanPauseStage::Incomplete
        } else {
            PlanPauseStage::Complete
        }
    }

    pub fn resume_stage_from_outcomes(&self) -> PlanPauseStage {
        let failed = self
            .agents
            .iter()
            .filter_map(|a| a.resume_notice.as_ref())
            .chain(
                self.schedules
                    .iter()
                    .filter_map(|s| s.resume_outcome.as_ref()),
            )
            .any(PlanPauseStepOutcome::is_failure);
        if failed {
            PlanPauseStage::Incomplete
        } else {
            PlanPauseStage::Complete
        }
    }
}
