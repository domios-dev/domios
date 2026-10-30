use crate::{
    multiplexer::types::Tab,
    plan::{Plan, PlanMode, PlanStatus, PlannerRef, TaskStatus},
};
use codeterm_pty::multiplexer::restore_policy::RestoreDecision;
use codeterm_types::pty::TimelineState;
use std::collections::HashSet;

/// Active plans with an open attempt on an in-progress task; `continuing_tabs` confirms the mode.
pub(super) fn continuing_candidates() -> crate::service::plan::PlanMatch {
    crate::service::plan::PlanMatch::active().and(
        crate::service::plan::AttemptMatch {
            state: crate::service::plan::AttemptState::Open,
            task_statuses: Some(vec![TaskStatus::InProgress]),
            ..Default::default()
        }
        .into_match(),
    )
}

pub(super) fn continuing_tabs(plans: &[Plan]) -> HashSet<String> {
    let mut tabs = HashSet::new();
    for plan in plans {
        if plan.status != PlanStatus::Active
            || matches!(plan.mode, PlanMode::Paused | PlanMode::Complete)
        {
            continue;
        }
        let mut continuing = false;
        for task in &plan.tasks {
            if task.status != TaskStatus::InProgress {
                continue;
            }
            for attempt in &task.execution_attempts {
                if attempt.closed_at.is_none() && attempt.retired_at.is_none() {
                    continuing = true;
                    if attempt.mesh_peer.is_none() {
                        tabs.insert(attempt.executor_tab_id.clone());
                    }
                }
            }
        }
        if continuing {
            tabs.extend(plan.owner.tab_id.iter().cloned());
            if let Some(PlannerRef::Tab { tab_id, .. }) = &plan.planner {
                tabs.insert(tab_id.clone());
            }
        }
    }
    tabs
}

pub(super) fn decision(tab: &Tab, continuing: bool, shell: RestoreDecision) -> RestoreDecision {
    if tab.pty_id.is_some() {
        return RestoreDecision::Start;
    }
    if tab.agent_type.is_none() || tab.protected {
        return shell;
    }
    let busy = tab.agent_state.as_ref().is_some_and(|state| {
        matches!(
            state.state,
            TimelineState::Working | TimelineState::Question
        )
    }) || tab.agent_runtime_state.as_ref().is_some_and(|state| {
        state
            .get("active_turn_started_at_ms")
            .is_some_and(|value| value.is_u64())
    });
    let interrupted = tab
        .turn_resume
        .as_ref()
        .is_some_and(|mark| mark.settled.is_none());
    if continuing || busy || interrupted {
        RestoreDecision::Start
    } else {
        RestoreDecision::Freeze
    }
}

#[cfg(test)]
mod tests;
