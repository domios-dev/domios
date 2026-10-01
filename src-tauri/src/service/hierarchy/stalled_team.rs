use std::collections::{HashMap, HashSet};

use parking_lot::Mutex;

use crate::plan::{Plan, PlanMode, PlanStatus, TaskStatus};
use crate::service::plan::reminders::NudgeKind;
use crate::service::ServiceContext;
use codeterm_plan::service_plan::state::orchestrator_actionable_task_ids;

use super::plan_reminder_nudge::PlanReminderTarget;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StallKey {
    plan_id: String,
    actionable_task_ids: Vec<String>,
}

#[derive(Default)]
pub(crate) struct StallEpisodeTracker {
    nudged: Mutex<HashMap<String, StallKey>>,
}

impl StallEpisodeTracker {
    fn reset(&self, plan_id: &str) {
        self.nudged.lock().remove(plan_id);
    }

    fn observe(&self, plan_id: &str, key: Option<StallKey>) -> Option<StallKey> {
        let mut nudged = self.nudged.lock();
        let next = match key {
            Some(key) if nudged.get(plan_id) == Some(&key) => None,
            Some(key) => {
                nudged.insert(plan_id.to_string(), key.clone());
                Some(key)
            }
            None => {
                nudged.remove(plan_id);
                None
            }
        };
        drop(nudged);
        next
    }
}

pub(crate) fn stall_episode(
    plan: &Plan,
    live_worker_tabs: &HashSet<String>,
) -> Option<StallKey> {
    if plan.status != PlanStatus::Active || plan.mode != PlanMode::Execution {
        return None;
    }
    let mut actionable_task_ids = orchestrator_actionable_task_ids(plan);
    if actionable_task_ids.is_empty()
        || task_bound_worker_tab_ids(plan)
            .iter()
            .any(|tab_id| live_worker_tabs.contains(tab_id))
    {
        return None;
    }
    actionable_task_ids.sort_unstable();
    Some(StallKey {
        plan_id: plan.id.clone(),
        actionable_task_ids,
    })
}

fn task_bound_worker_tab_ids(plan: &Plan) -> HashSet<String> {
    plan.tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.status,
                TaskStatus::Paused | TaskStatus::Done | TaskStatus::Cancelled
            )
        })
        .flat_map(|task| {
            task.assigned_tab_id
                .iter()
                .chain(
                    task.execution_attempts
                        .iter()
                        .filter(|attempt| attempt.closed_at.is_none())
                        .map(|attempt| &attempt.executor_tab_id),
                )
                .filter(|tab_id| !tab_id.trim().is_empty())
                .cloned()
        })
        .collect()
}

fn maybe_nudge_stalled_team(
    plan: &Plan,
    live_worker_tabs: &HashSet<String>,
    orchestrator_idle: bool,
    tracker: &StallEpisodeTracker,
    deliver: impl FnOnce(&StallKey) -> Result<(), String>,
) -> Result<bool, String> {
    if !orchestrator_idle {
        return Ok(false);
    }
    let Some(key) = tracker.observe(&plan.id, stall_episode(plan, live_worker_tabs)) else {
        return Ok(false);
    };
    deliver(&key)?;
    Ok(true)
}

impl ServiceContext {
    pub(crate) fn nudge_stalled_teams_for_idle_orchestrator(&self, tab_id: &str) {
        for plan in self
            .plan_store
            .list_full()
            .into_iter()
            .filter(|plan| plan.owner.tab_id.as_deref() == Some(tab_id))
        {
            self.nudge_stalled_team_for_plan(&plan, true, None);
        }
    }

    pub(crate) fn nudge_stalled_teams_for_gone_tab(
        &self,
        gone_tab_id: &str,
        plan_ids: &[String],
    ) {
        for plan_id in plan_ids {
            let Some(plan) = self.plan_store.get(plan_id) else {
                continue;
            };
            let orchestrator_idle = plan
                .owner
                .tab_id
                .as_deref()
                .is_some_and(|tab_id| self.orchestrator_is_idle(tab_id));
            self.nudge_stalled_team_for_plan(&plan, orchestrator_idle, Some(gone_tab_id));
        }
    }

    fn nudge_stalled_team_for_plan(
        &self,
        plan: &Plan,
        orchestrator_idle: bool,
        gone_tab_id: Option<&str>,
    ) {
        let Some(orchestrator_tab_id) = plan.owner.tab_id.as_deref() else {
            return;
        };
        if !crate::config::load_config().orchestration.plan_reminders {
            self.stalled_team_nudges.reset(&plan.id);
            return;
        }
        let live_worker_tabs = self.live_task_worker_tabs(plan, gone_tab_id);
        if let Err(error) = maybe_nudge_stalled_team(
            plan,
            &live_worker_tabs,
            orchestrator_idle,
            &self.stalled_team_nudges,
            |key| {
                self.deliver_plan_reminder_target(
                    orchestrator_tab_id,
                    &PlanReminderTarget::Owner {
                        plan_id: key.plan_id.clone(),
                    },
                    NudgeKind::StalledTeam,
                )
            },
        ) {
            log::warn!(
                "[stalled-team] nudge failed for plan {}: {}",
                plan.id,
                error
            );
        }
    }

    fn live_task_worker_tabs(&self, plan: &Plan, gone_tab_id: Option<&str>) -> HashSet<String> {
        let mux = self.mux.read();
        task_bound_worker_tab_ids(plan)
            .into_iter()
            .filter(|tab_id| Some(tab_id.as_str()) != gone_tab_id)
            .filter(|tab_id| {
                mux.resolve_tab_id(tab_id)
                    .and_then(|resolved| mux.get_tab(&resolved))
                    .is_some_and(crate::service::plan_reconcile::tab_has_live_agent)
            })
            .collect()
    }

    fn orchestrator_is_idle(&self, tab_id: &str) -> bool {
        use crate::agent::offline_state_classifier::TimelineState;
        use crate::multiplexer::types::AgentRole;

        let mux = self.mux.read();
        mux.resolve_tab_id(tab_id)
            .and_then(|resolved| mux.get_tab(&resolved))
            .is_some_and(|tab| {
                tab.role == Some(AgentRole::Orchestrator)
                    && tab
                        .agent_state
                        .as_ref()
                        .is_some_and(|state| state.state == TimelineState::Idle)
            })
    }
}

#[cfg(test)]
mod tests;
