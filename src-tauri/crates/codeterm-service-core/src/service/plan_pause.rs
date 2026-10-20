use std::collections::HashSet;

use codeterm_types::agent::role_kind::builtins::{ORCHESTRATOR, PLANNER};
use codeterm_types::plan::{
    AgentRef, Plan, PlanMode, PlanPauseAgent, PlanPauseRole, PlanPauseSchedule,
    PlanPauseStepOutcome, PlannerRef,
};

use super::plan::pause_operation::open_pause_operation;
use super::plan::PlanError;
use super::team_core::TeamPlanError;
use super::ServiceContext;
use crate::cron::store::CronJob;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab};

pub use codeterm_plan::service_plan::pause::{
    execute_plan_pause, execute_plan_resume, pause_preview, resume_preview, PlanLifecycleOutcome,
    PlanLifecyclePreview, PlanPausePorts,
};

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn planner_tab_id(plan: &Plan) -> Option<&str> {
    match plan.planner.as_ref()? {
        PlannerRef::Tab { tab_id, .. } => non_empty(Some(tab_id)),
        PlannerRef::Waived { .. } => None,
    }
}

fn pause_role(plan: &Plan, tab: &Tab) -> Option<PlanPauseRole> {
    let is_tab = |id: Option<&str>| id == Some(tab.id.as_str());
    if is_tab(non_empty(plan.owner.tab_id.as_deref())) || tab.role_kind_id() == Some(ORCHESTRATOR) {
        return Some(PlanPauseRole::Orchestrator);
    }
    if is_tab(planner_tab_id(plan)) || tab.role_kind_id() == Some(PLANNER) {
        return Some(PlanPauseRole::Planner);
    }
    (tab.role == Some(AgentRole::Worker)).then_some(PlanPauseRole::Worker)
}

fn role_rank(role: PlanPauseRole) -> u8 {
    match role {
        PlanPauseRole::Orchestrator => 0,
        PlanPauseRole::Planner => 1,
        PlanPauseRole::Worker => 2,
    }
}

fn display_name(tab: &Tab) -> String {
    let title = tab.title.trim();
    if title.is_empty() {
        tab.id.clone()
    } else {
        title.to_string()
    }
}

/// Live Orchestrator, Planner and Worker agents bound to the plan or its Team.
pub(crate) fn live_plan_agents(mux: &MultiplexerState, plan: &Plan) -> Vec<PlanPauseAgent> {
    let team_id = non_empty(plan.team_id.as_deref());
    let owner = non_empty(plan.owner.tab_id.as_deref());
    let planner = planner_tab_id(plan);
    let mut agents: Vec<PlanPauseAgent> = mux
        .all_tabs()
        .filter(|tab| {
            let id = Some(tab.id.as_str());
            let bound = team_id.is_some_and(|team| tab.team_id.as_deref() == Some(team))
                || id == owner
                || id == planner;
            bound && super::plan_reconcile::tab_has_live_agent(tab)
        })
        .filter_map(|tab| {
            Some(PlanPauseAgent {
                tab_id: tab.id.clone(),
                name: display_name(tab),
                role: pause_role(plan, tab)?,
                team_id: tab.team_id.clone(),
                session_id: tab.agent_session_id.clone(),
                pause_notice: PlanPauseStepOutcome::Pending,
                resume_notice: None,
            })
        })
        .collect();
    agents.sort_by(|a, b| {
        role_rank(a.role)
            .cmp(&role_rank(b.role))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.tab_id.cmp(&b.tab_id))
    });
    agents
}

/// A recorded agent is still reachable when its own tab is live in the same Team.
pub(crate) fn recorded_agent_is_live(mux: &MultiplexerState, agent: &PlanPauseAgent) -> bool {
    mux.get_tab(&agent.tab_id).is_some_and(|tab| {
        super::plan_reconcile::tab_has_live_agent(tab)
            && (agent.team_id.is_none() || tab.team_id == agent.team_id)
    })
}

pub(crate) fn enabled_team_schedules<'a>(
    jobs: impl IntoIterator<Item = &'a CronJob>,
    team_id: &str,
) -> Vec<PlanPauseSchedule> {
    jobs.into_iter()
        .filter(|job| job.enabled && job.team_id.as_deref() == Some(team_id))
        .map(|job| PlanPauseSchedule {
            job_id: job.id.clone(),
            name: job.name.clone(),
            prior_enabled: job.enabled,
            pause_outcome: PlanPauseStepOutcome::Pending,
            resume_outcome: None,
        })
        .collect()
}

pub struct ServicePlanPausePorts<'a> {
    pub svc: &'a ServiceContext,
}

impl PlanPausePorts for ServicePlanPausePorts<'_> {
    fn send_notice(&self, tab_id: &str, text: &str) -> Result<(), String> {
        let envelope = super::messenger::MessageEnvelope::codeterm_reminder("plan");
        self.svc
            .gateway_deliver(
                tab_id,
                envelope,
                text,
                super::message_gateway::DeliveryClass::SystemNotice,
            )
            .map(|_| ())
    }

    fn schedule_enabled(&self, job_id: &str) -> Option<bool> {
        let cron = self.svc.try_ext::<crate::CronState>()?;
        let enabled = cron.lock().get(job_id).map(|job| job.enabled);
        enabled
    }

    fn set_schedule_enabled(&self, job_id: &str, enabled: bool) -> Result<(), String> {
        let cron = self
            .svc
            .try_ext::<crate::CronState>()
            .ok_or("cron store unavailable")?;
        let wake = self
            .svc
            .try_ext::<crate::CronWake>()
            .ok_or("cron wake unavailable")?;
        let set = || {
            self.svc
                .cron_enable(cron, wake, job_id, enabled)
                .map(|_| ())
        };
        // Storage blocks until COMMIT; a runtime worker hands its queue to another thread first.
        match tokio::runtime::Handle::try_current() {
            Ok(runtime)
                if runtime.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread =>
            {
                tokio::task::block_in_place(set)
            }
            _ => set(),
        }
    }
}

impl ServiceContext {
    fn lifecycle_plan(&self, plan_id: &str) -> Result<Plan, TeamPlanError> {
        self.plan_store
            .get(plan_id)
            .ok_or(TeamPlanError::Plan(PlanError::NotFound))
    }

    fn enabled_plan_schedules(&self, plan: &Plan) -> Vec<PlanPauseSchedule> {
        let Some(team_id) = non_empty(plan.team_id.as_deref()) else {
            return vec![];
        };
        let Some(cron) = self.try_ext::<crate::CronState>() else {
            return vec![];
        };
        let store = cron.lock();
        let schedules = enabled_team_schedules(store.list(), team_id);
        schedules
    }

    fn live_recorded_agents(&self, plan: &Plan) -> HashSet<String> {
        let Some(operation) = open_pause_operation(plan) else {
            return HashSet::new();
        };
        let mux = self.mux.read();
        operation
            .agents
            .iter()
            .filter(|agent| recorded_agent_is_live(&mux, agent))
            .map(|agent| agent.tab_id.clone())
            .collect()
    }

    pub fn plan_pause_preview(&self, plan_id: &str) -> Result<PlanLifecyclePreview, TeamPlanError> {
        let plan = self.lifecycle_plan(plan_id)?;
        let agents = {
            let mux = self.mux.read();
            live_plan_agents(&mux, &plan)
        };
        let schedules = self.enabled_plan_schedules(&plan);
        Ok(pause_preview(&plan, &agents, &schedules))
    }

    pub fn plan_resume_preview(
        &self,
        plan_id: &str,
    ) -> Result<PlanLifecyclePreview, TeamPlanError> {
        let plan = self.lifecycle_plan(plan_id)?;
        let live = self.live_recorded_agents(&plan);
        Ok(resume_preview(&plan, |agent| live.contains(&agent.tab_id)))
    }

    pub fn pause_plan_operation(
        &self,
        plan_id: &str,
        reason: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<PlanLifecycleOutcome, TeamPlanError> {
        let plan = self.lifecycle_plan(plan_id)?;
        let (agents, schedules) = if plan.mode == PlanMode::Paused {
            (vec![], vec![])
        } else {
            let agents = {
                let mux = self.mux.read();
                live_plan_agents(&mux, &plan)
            };
            (agents, self.enabled_plan_schedules(&plan))
        };
        let outcome = execute_plan_pause(
            &self.plan_store,
            plan_id,
            reason,
            from,
            agents,
            schedules,
            &ServicePlanPausePorts { svc: self },
            now_ms,
        )
        .map_err(TeamPlanError::Plan)?;
        self.emit_plan_changed(plan_id);
        self.reconcile_time_reminder_eligibility();
        Ok(outcome)
    }

    pub fn resume_plan_operation(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<PlanLifecycleOutcome, TeamPlanError> {
        {
            let mux = self.mux.read();
            super::team_core::valid_live_plan_team(self, &mux, plan_id)?;
        }
        let plan = self.lifecycle_plan(plan_id)?;
        let live = self.live_recorded_agents(&plan);
        let outcome = execute_plan_resume(
            &self.plan_store,
            plan_id,
            &|agent| live.contains(&agent.tab_id),
            &ServicePlanPausePorts { svc: self },
            now_ms,
        )
        .map_err(TeamPlanError::Plan)?;
        self.emit_plan_changed(plan_id);
        self.reconcile_time_reminder_eligibility();
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
