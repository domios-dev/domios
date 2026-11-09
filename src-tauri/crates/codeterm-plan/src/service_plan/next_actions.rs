use serde::Serialize;

use crate::plan::{LoopStatus, QaState, ReviewStage, TaskStatus};

use super::state::PlanStateSnapshot;

mod input;
pub(super) use input::{ActionLane, ActionLoop, NextActionInput};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum NextActionKind {
    ApprovePlan,
    Gate,
    Advance,
    Reconcile,
    ConsiderStop,
    Dispatch,
    ClaimQa,
    SpawnQA,
    ParkedForLaterGroups,
    Continue,
    ReviewWipBlocked,
    WaitingOnDependency,
    AdoptPlan,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NextAction {
    pub priority: u8,
    pub kind: NextActionKind,
    pub task_id: Option<String>,
    pub loop_id: Option<String>,
    pub message: String,
}

// name and surface any approval blockers. Shared wording lives in prompts.
fn planning_next_action_message(s: &NextActionInput) -> String {
    let peer = crate::service_plan::peer_mandate::planner_peer_label(&s.planner_resolution);
    let mut msg = super::prompts_plan::orch_defer_structure_line(&peer);
    if s.planning_approval_blockers.is_empty() {
        msg.push_str(" Structure looks ready — run the quality doctor, then approve.");
    } else {
        msg.push_str(" Approval blockers: ");
        msg.push_str(&s.planning_approval_blockers.join("; "));
        msg.push('.');
    }
    msg
}

fn dependency_wait_names(waits: &[super::state::DependencyWait]) -> String {
    waits
        .iter()
        .map(|wait| {
            if wait.missing {
                format!("{} (missing)", wait.task_id)
            } else {
                let status = wait
                    .status
                    .as_ref()
                    .map(|status| format!("{status:?}"))
                    .unwrap_or_else(|| "unknown".to_string());
                format!("{} ({status})", wait.task_id)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// Running loop with no headroom left: at or one short of budget, or past 80%.
pub(super) fn loop_near_limit(l: &ActionLoop) -> bool {
    if l.status != LoopStatus::Running || l.budget == 0 {
        return false;
    }
    l.count + 1 >= l.budget || (l.count as f64) >= 0.8 * (l.budget as f64)
}

/// A flagged orphan plan has no owner to act; adoption precedes any dispatch.
pub fn adopt_orphaned_plan_action(plan_id: &str, reason: &str) -> NextAction {
    NextAction {
        priority: 1,
        kind: NextActionKind::AdoptPlan,
        task_id: None,
        loop_id: None,
        message: format!("{reason}: domios team adopt-plan <team-id> {plan_id}"),
    }
}

/// Deterministic, advisory next-action ranking over a plan state snapshot.
/// Priority ascending (1 = act first). Never mutates anything.
pub fn compute_next_actions(s: &PlanStateSnapshot) -> Vec<NextAction> {
    compute_next_actions_from(&NextActionInput::from(s))
}

pub(super) fn compute_next_actions_from(s: &NextActionInput) -> Vec<NextAction> {
    let mut actions = Vec::new();

    if !s.execution_tasks_unlocked {
        actions.push(NextAction {
            priority: 1,
            kind: NextActionKind::ApprovePlan,
            task_id: None,
            loop_id: None,
            message: planning_next_action_message(s),
        });
    }

    for lane in &s.open_lanes {
        if lane.status == TaskStatus::InReview {
            if lane.review_stage == ReviewStage::Qa {
                let kind = if lane.qa_state == Some(QaState::Queued) {
                    NextActionKind::ClaimQa
                } else {
                    NextActionKind::SpawnQA
                };
                let message = if lane.qa_state == Some(QaState::Queued) {
                    format!(
                        "claim QA {} ({}) with `domios plan qa next {} --claim --note \"<scope>\"`",
                        lane.task_id, lane.name, s.plan_id
                    )
                } else {
                    format!("QA running for {} ({})", lane.task_id, lane.name)
                };
                actions.push(NextAction {
                    priority: 1,
                    kind,
                    task_id: Some(lane.task_id.to_string()),
                    loop_id: None,
                    message,
                });
                continue;
            }
            actions.push(NextAction {
                priority: 1,
                kind: NextActionKind::Gate,
                task_id: Some(lane.task_id.to_string()),
                loop_id: None,
                message: format!("gate {} ({})", lane.task_id, lane.name),
            });
        }
    }

    let parked_count = s
        .open_lanes
        .iter()
        .filter(|lane| lane.presentation == super::state::TaskPresentation::ParkedForLaterGroup)
        .count();
    if parked_count > 0 {
        actions.push(NextAction {
            priority: 2,
            kind: NextActionKind::ParkedForLaterGroups,
            task_id: None,
            loop_id: None,
            message: format!("{parked_count} tasks parked for later groups"),
        });
    }

    for lane in &s.blocked_lanes {
        actions.push(NextAction {
            priority: 2,
            kind: NextActionKind::Advance,
            task_id: Some(lane.task_id.to_string()),
            loop_id: None,
            message: format!("advance/unblock {} ({})", lane.task_id, lane.name),
        });
    }

    // Dependency-gated lanes must remain visible as pending work while their unblocking task runs.
    for lane in s.open_lanes.iter().filter(|lane| {
        lane.waiting_on_dependencies
            && lane.presentation != super::state::TaskPresentation::ParkedForLaterGroup
    }) {
        actions.push(NextAction {
            priority: 2,
            kind: NextActionKind::WaitingOnDependency,
            task_id: Some(lane.task_id.to_string()),
            loop_id: None,
            message: format!(
                "{} ({}) waiting on: {}",
                lane.task_id,
                lane.name,
                dependency_wait_names(&lane.dependency_waits)
            ),
        });
    }

    if s.drift_total > 0 {
        actions.push(NextAction {
            priority: 3,
            kind: NextActionKind::Reconcile,
            task_id: None,
            loop_id: None,
            message: format!("reconcile ({} drift item(s))", s.drift_total),
        });
    }

    let has_unblocked_work = s.open_lanes.iter().any(|lane| {
        lane.presentation == super::state::TaskPresentation::ActionablePending
            && !lane.waiting_on_dependencies
    });
    let qa_only = !s.open_lanes.is_empty()
        && s.open_lanes.iter().all(|lane| {
            lane.status == TaskStatus::InReview && lane.review_stage == ReviewStage::Qa
        });
    if s.execution_tasks_unlocked && has_unblocked_work && !s.has_live_agents && !qa_only {
        actions.push(NextAction {
            priority: 1,
            kind: NextActionKind::Continue,
            task_id: None,
            loop_id: None,
            message: format!(
                "continue plan {}: open unblocked work has zero live workers",
                s.plan_id
            ),
        });
    }

    for l in &s.loops {
        if loop_near_limit(l) {
            actions.push(NextAction {
                priority: 4,
                kind: NextActionKind::ConsiderStop,
                task_id: None,
                loop_id: Some(l.id.to_string()),
                message: format!(
                    "consider stop: loop {} nearly exhausted ({}/{})",
                    l.id, l.count, l.budget
                ),
            });
        }
    }

    let pending_lanes = s
        .open_lanes
        .iter()
        .filter(|lane| {
            lane.presentation == super::state::TaskPresentation::ActionablePending
                && !lane.waiting_on_dependencies
        })
        .collect::<Vec<_>>();
    for lane in pending_lanes {
        let message = if let Some(summary) = lane.competition.as_deref() {
            format!(
                "{} {} ({summary}): plan.task.compete route for plan {} task {}",
                crate::service_plan::competition::COMPETITIVE_DISPATCH_NUDGE,
                lane.task_id,
                s.plan_id,
                lane.task_id
            )
        } else {
            format!("dispatch {} ({})", lane.task_id, lane.name)
        };
        let mut gates = s
            .pending_regression_gates
            .iter()
            .chain(s.pending_group_retrospectives)
            .map(|gate| gate.key.as_str())
            .collect::<Vec<_>>();
        gates.sort_unstable();
        gates.dedup();
        let message = if gates.is_empty() {
            message
        } else {
            format!(
                "{message} — terminal-group gate advisory (dispatch allowed): {}",
                gates.join(", ")
            )
        };
        actions.push(NextAction {
            priority: 5,
            kind: NextActionKind::Dispatch,
            task_id: Some(lane.task_id.to_string()),
            loop_id: None,
            message,
        });
    }

    actions.sort_by_key(|a| a.priority);
    actions
}

#[cfg(test)]
mod tests;
