use super::state::input::{StateInput, StateSource};
use serde::Serialize;

use super::prompts_plan::REGRESSION_MILESTONE_KIND;
use super::reflection::GROUP_CLOSE_MILESTONE_KIND;
use crate::plan::{AgentRef, Plan, RegressionGateOverride};

use super::{PlanError, PlanStore};

pub const GROUP_REGRESSION_LINK_PREFIX: &str = "group:";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegressionGate {
    pub key: String,
    pub label: String,
}

/// Outcome of evaluating a task dispatch against a plan's pending gates.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RegressionDispatchDecision {
    /// No terminal group has pending advisory records.
    Clear,
    /// Dispatch proceeds while the listed terminal-group records remain open.
    Advisory {
        regression_gates: Vec<RegressionGate>,
        retrospectives: Vec<RegressionGate>,
    },
}

pub fn regression_link(group_key: &str) -> String {
    format!("{GROUP_REGRESSION_LINK_PREFIX}{group_key}")
}

fn group_label(plan: &impl StateSource, key: &str) -> String {
    let input = plan.state_input();
    let plan = input.as_ref();
    plan.task_groups
        .iter()
        .find(|g| g.key == key)
        .map(|g| g.label.clone())
        .unwrap_or_else(|| key.to_string())
}

// >=1 Done required so an all-cancelled group (shipped nothing) never gates.
pub fn group_completion(plan: &impl StateSource, key: &str) -> Option<u64> {
    let input = plan.state_input();
    let plan = input.as_ref();
    let members: Vec<&_> = plan
        .tasks
        .iter()
        .filter(|t| t.group.as_deref() == Some(key))
        .collect();
    if members.is_empty() {
        return None;
    }
    if !members.iter().all(|t| t.status.is_terminal()) {
        return None;
    }
    if !members
        .iter()
        .any(|t| t.status == crate::plan::TaskStatus::Done)
    {
        return None;
    }
    members.iter().map(|t| t.status_changed_at).max()
}

fn latest_regression_ts(plan: &impl StateSource, key: &str) -> Option<u64> {
    let input = plan.state_input();
    let plan = input.as_ref();
    let link = regression_link(key);
    plan.strides
        .iter()
        .filter(|m| m.kind == REGRESSION_MILESTONE_KIND && m.links.iter().any(|l| l == &link))
        .map(|m| m.ts)
        .max()
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct RegressionGroupInput {
    pub key: String,
    pub label: String,
    pub completed_at: u64,
    pub regression_at: Option<u64>,
    pub override_at: Option<u64>,
    pub retrospective_at: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegressionInput {
    pub groups: Vec<RegressionGroupInput>,
}

impl From<&Plan> for RegressionInput {
    fn from(plan: &Plan) -> Self {
        Self::from(&StateInput::from(plan))
    }
}

impl From<&StateInput> for RegressionInput {
    fn from(plan: &StateInput) -> Self {
        let mut seen = std::collections::HashSet::new();
        let groups = plan
            .tasks
            .iter()
            .filter_map(|task| task.group.as_deref())
            .filter(|key| seen.insert(*key))
            .filter_map(|key| {
                group_completion(plan, key).map(|completed_at| {
                    let link = regression_link(key);
                    RegressionGroupInput {
                        key: key.to_owned(),
                        label: group_label(plan, key),
                        completed_at,
                        regression_at: latest_regression_ts(plan, key),
                        override_at: plan
                            .regression_gate_overrides
                            .iter()
                            .filter(|entry| entry.gates.iter().any(|gate| gate == key))
                            .map(|entry| entry.ts)
                            .max(),
                        retrospective_at: plan
                            .strides
                            .iter()
                            .filter(|stride| {
                                stride.kind == GROUP_CLOSE_MILESTONE_KIND
                                    && stride.links.contains(&link)
                            })
                            .map(|stride| stride.ts)
                            .max(),
                    }
                })
            })
            .collect();
        Self { groups }
    }
}

fn sorted_gates<'a>(groups: impl Iterator<Item = &'a RegressionGroupInput>) -> Vec<RegressionGate> {
    let mut gates: Vec<_> = groups
        .map(|group| RegressionGate {
            key: group.key.clone(),
            label: group.label.clone(),
        })
        .collect();
    gates.sort_by(|a, b| a.key.cmp(&b.key));
    gates
}

fn compute_gates(input: &RegressionInput, respect_override: bool) -> Vec<RegressionGate> {
    sorted_gates(
        input
            .groups
            .iter()
            .filter(|group| {
                !group
                    .regression_at
                    .is_some_and(|at| at >= group.completed_at)
            })
            .filter(|group| {
                !(respect_override && group.override_at.is_some_and(|at| at >= group.completed_at))
            }),
    )
}

fn retrospective_gates(input: &RegressionInput) -> Vec<RegressionGate> {
    sorted_gates(input.groups.iter().filter(|group| {
        !group
            .retrospective_at
            .is_some_and(|at| at >= group.completed_at)
    }))
}

pub fn pending_group_retrospectives(input: impl Into<RegressionInput>) -> Vec<RegressionGate> {
    retrospective_gates(&input.into())
}

pub fn pending_regression_gates(input: impl Into<RegressionInput>) -> Vec<RegressionGate> {
    compute_gates(&input.into(), false)
}

pub fn blocking_regression_gates(input: impl Into<RegressionInput>) -> Vec<RegressionGate> {
    compute_gates(&input.into(), true)
}

/// Decides whether terminal-group recordkeeping should accompany a dispatch.
/// Never mutates and never refuses dispatch.
pub fn regression_dispatch_advisory(
    regression_gates: &[RegressionGate],
    retrospectives: &[RegressionGate],
) -> RegressionDispatchDecision {
    if regression_gates.is_empty() && retrospectives.is_empty() {
        return RegressionDispatchDecision::Clear;
    }
    RegressionDispatchDecision::Advisory {
        regression_gates: regression_gates.to_vec(),
        retrospectives: retrospectives.to_vec(),
    }
}

pub fn regression_dispatch_decision(
    input: impl Into<RegressionInput>,
) -> RegressionDispatchDecision {
    let input = input.into();
    regression_dispatch_advisory(&compute_gates(&input, false), &retrospective_gates(&input))
}

impl PlanStore {
    // One plan-level working point per group, not a stride scattered per task.
    pub fn record_group_regression(
        &self,
        plan_id: &str,
        group_key: &str,
        note: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<RegressionGate, PlanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        let turn = self.mutation_writer.enter();
        let (gate, receipt) = self
            .database
            .record_group_regression_rows(plan_id, group_key, note, from, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(gate)
    }

    /// Record an authorized Orchestrator waiver of pending regression gates.
    /// Requires a reason and owner authorization; appends to the audit trail and
    /// never clears a gate implicitly. `gates` defaults to the currently blocking
    /// set so a bare override names exactly what it waives. Returns the record.
    pub fn record_regression_override(
        &self,
        plan_id: &str,
        gates: Vec<String>,
        reason: &str,
        actor: AgentRef,
        now_ms: u64,
    ) -> Result<RegressionGateOverride, PlanError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(PlanError::OverrideReasonRequired);
        }
        let turn = self.mutation_writer.enter();
        let (record, receipt) = self
            .database
            .record_regression_override_rows(plan_id, gates, reason, actor, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(record)
    }
}

#[cfg(test)]
mod tests;
