use std::collections::HashSet;

use codeterm_types::agent::role_kind::builtins::{ORCHESTRATOR, PLANNER};
use codeterm_types::plan::{
    AgentRef, Plan, PlanMode, PlanPauseAgent, PlanPauseEffect, PlanPauseOperation, PlanPauseRole,
    PlanPauseSchedule, PlanPauseStepOutcome, PlannerRef, PLAN_PAUSE_EFFECTS,
};
use serde::Serialize;
use tauri::Manager;

use super::plan::pause_operation::{open_pause_operation, PlanPauseBegin, PlanResumeBegin};
use super::plan::{PlanError, PlanStore};
use super::team::TeamPlanError;
use super::ServiceContext;
use crate::cron::store::CronJob;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab};

/// Side effects a plan pause or resume performs outside the plan record.
pub(crate) trait PlanPausePorts {
    fn send_notice(&self, tab_id: &str, text: &str) -> Result<(), String>;
    fn schedule_enabled(&self, job_id: &str) -> Option<bool>;
    fn set_schedule_enabled(&self, job_id: &str, enabled: bool) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanLifecycleKind {
    Pause,
    Resume,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct PlanLifecyclePreviewAgent {
    pub(crate) tab_id: String,
    pub(crate) name: String,
    pub(crate) role: PlanPauseRole,
    pub(crate) available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct PlanLifecyclePreviewSchedule {
    pub(crate) job_id: String,
    pub(crate) name: String,
    pub(crate) prior_enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct PlanLifecyclePreview {
    pub(crate) kind: PlanLifecycleKind,
    pub(crate) plan_id: String,
    pub(crate) plan_title: String,
    pub(crate) mode: PlanMode,
    pub(crate) applicable: bool,
    pub(crate) not_applicable_reason: Option<String>,
    pub(crate) operation_id: Option<String>,
    pub(crate) restore_mode: Option<PlanMode>,
    pub(crate) agents: Vec<PlanLifecyclePreviewAgent>,
    pub(crate) schedules: Vec<PlanLifecyclePreviewSchedule>,
    pub(crate) effects: Vec<PlanPauseEffect>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct PlanLifecycleOutcome {
    pub(crate) kind: PlanLifecycleKind,
    pub(crate) plan_id: String,
    pub(crate) applied: bool,
    pub(crate) mode: PlanMode,
    pub(crate) stride_id: Option<String>,
    pub(crate) operation: Option<PlanPauseOperation>,
}

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

fn preview_agent(agent: &PlanPauseAgent, available: bool) -> PlanLifecyclePreviewAgent {
    PlanLifecyclePreviewAgent {
        tab_id: agent.tab_id.clone(),
        name: agent.name.clone(),
        role: agent.role,
        available,
    }
}

fn preview_schedule(schedule: &PlanPauseSchedule) -> PlanLifecyclePreviewSchedule {
    PlanLifecyclePreviewSchedule {
        job_id: schedule.job_id.clone(),
        name: schedule.name.clone(),
        prior_enabled: schedule.prior_enabled,
    }
}

pub(crate) fn pause_preview(
    plan: &Plan,
    agents: &[PlanPauseAgent],
    schedules: &[PlanPauseSchedule],
) -> PlanLifecyclePreview {
    let applicable = plan.mode != PlanMode::Paused;
    PlanLifecyclePreview {
        kind: PlanLifecycleKind::Pause,
        plan_id: plan.id.clone(),
        plan_title: plan.title.clone(),
        mode: plan.mode.clone(),
        applicable,
        not_applicable_reason: (!applicable).then(|| "Plan is already paused".to_string()),
        operation_id: open_pause_operation(plan).map(|operation| operation.id.clone()),
        restore_mode: None,
        agents: if applicable {
            agents
                .iter()
                .map(|agent| preview_agent(agent, true))
                .collect()
        } else {
            vec![]
        },
        schedules: if applicable {
            schedules.iter().map(preview_schedule).collect()
        } else {
            vec![]
        },
        effects: if applicable {
            PLAN_PAUSE_EFFECTS.to_vec()
        } else {
            vec![]
        },
    }
}

pub(crate) fn resume_preview(
    plan: &Plan,
    is_live: impl Fn(&PlanPauseAgent) -> bool,
) -> PlanLifecyclePreview {
    let reason = if plan.mode != PlanMode::Paused {
        Some("Plan is not paused")
    } else if non_empty(plan.team_id.as_deref()).is_none() {
        Some("Plan has no Team to resume into")
    } else {
        None
    };
    let applicable = reason.is_none();
    let operation = open_pause_operation(plan).filter(|_| applicable);
    PlanLifecyclePreview {
        kind: PlanLifecycleKind::Resume,
        plan_id: plan.id.clone(),
        plan_title: plan.title.clone(),
        mode: plan.mode.clone(),
        applicable,
        not_applicable_reason: reason.map(str::to_string),
        operation_id: operation.map(|op| op.id.clone()),
        restore_mode: applicable
            .then(|| plan.paused_from_mode.clone().unwrap_or(PlanMode::Execution)),
        agents: operation
            .map(|op| {
                op.agents
                    .iter()
                    .map(|agent| preview_agent(agent, is_live(agent)))
                    .collect()
            })
            .unwrap_or_default(),
        schedules: operation
            .map(|op| {
                op.schedules
                    .iter()
                    .filter(|s| s.pause_outcome == PlanPauseStepOutcome::Disabled)
                    .map(preview_schedule)
                    .collect()
            })
            .unwrap_or_default(),
        effects: match (applicable, operation) {
            (false, _) => vec![],
            (true, Some(op)) => op.effects.clone(),
            (true, None) => PLAN_PAUSE_EFFECTS.to_vec(),
        },
    }
}

fn mode_label(mode: &PlanMode) -> &'static str {
    match mode {
        PlanMode::Planning => "planning",
        PlanMode::Approved => "approved",
        PlanMode::Execution => "execution",
        PlanMode::Paused => "paused",
        PlanMode::Complete => "complete",
    }
}

fn pause_notice_text(plan_id: &str, title: &str, reason: &str) -> String {
    format!(
        "[plan paused: {plan_id}] \"{title}\" is paused: {reason}\n\
         Checkpoint the current step, start no new plan work, and wait for a continue notice."
    )
}

fn resume_notice_text(plan_id: &str, title: &str, mode: &PlanMode) -> String {
    format!(
        "[plan resumed: {plan_id}] \"{title}\" is back in {} mode.\nContinue your assigned plan work.",
        mode_label(mode)
    )
}

fn send_outcome(ports: &dyn PlanPausePorts, tab_id: &str, text: &str) -> PlanPauseStepOutcome {
    match ports.send_notice(tab_id, text) {
        Ok(()) => PlanPauseStepOutcome::Delivered,
        Err(error) => PlanPauseStepOutcome::Failed { error },
    }
}

fn restore_schedule(ports: &dyn PlanPausePorts, job_id: &str) -> PlanPauseStepOutcome {
    match ports.schedule_enabled(job_id) {
        None => PlanPauseStepOutcome::Skipped {
            reason: "schedule no longer exists".into(),
        },
        Some(true) => PlanPauseStepOutcome::Skipped {
            reason: "schedule already enabled".into(),
        },
        Some(false) => match ports.set_schedule_enabled(job_id, true) {
            Ok(()) => PlanPauseStepOutcome::Enabled,
            Err(error) => PlanPauseStepOutcome::Failed { error },
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_plan_pause(
    store: &PlanStore,
    plan_id: &str,
    reason: &str,
    from: AgentRef,
    agents: Vec<PlanPauseAgent>,
    schedules: Vec<PlanPauseSchedule>,
    ports: &dyn PlanPausePorts,
    now_ms: u64,
) -> Result<PlanLifecycleOutcome, PlanError> {
    let (operation, stride_id) =
        match store.begin_plan_pause_operation(plan_id, reason, from, agents, schedules, now_ms)? {
            PlanPauseBegin::AlreadyPaused { operation } => {
                return Ok(PlanLifecycleOutcome {
                    kind: PlanLifecycleKind::Pause,
                    plan_id: plan_id.into(),
                    applied: false,
                    mode: PlanMode::Paused,
                    stride_id: None,
                    operation,
                })
            }
            PlanPauseBegin::Applied {
                operation,
                stride_id,
            } => (operation, stride_id),
        };
    let schedule_outcomes: Vec<_> = operation
        .schedules
        .iter()
        .map(|schedule| {
            let outcome = match ports.set_schedule_enabled(&schedule.job_id, false) {
                Ok(()) => PlanPauseStepOutcome::Disabled,
                Err(error) => PlanPauseStepOutcome::Failed { error },
            };
            (schedule.job_id.clone(), outcome)
        })
        .collect();
    let title = store
        .get(plan_id)
        .map(|plan| plan.title)
        .unwrap_or_default();
    let notice = pause_notice_text(plan_id, &title, &operation.reason);
    let agent_outcomes: Vec<_> = operation
        .agents
        .iter()
        .map(|agent| {
            (
                agent.tab_id.clone(),
                send_outcome(ports, &agent.tab_id, &notice),
            )
        })
        .collect();
    let operation = store.record_plan_pause_outcomes(
        plan_id,
        &operation.id,
        &agent_outcomes,
        &schedule_outcomes,
        now_ms,
    )?;
    Ok(PlanLifecycleOutcome {
        kind: PlanLifecycleKind::Pause,
        plan_id: plan_id.into(),
        applied: true,
        mode: PlanMode::Paused,
        stride_id: Some(stride_id),
        operation: Some(operation),
    })
}

pub(crate) fn execute_plan_resume(
    store: &PlanStore,
    plan_id: &str,
    is_live: &dyn Fn(&PlanPauseAgent) -> bool,
    ports: &dyn PlanPausePorts,
    now_ms: u64,
) -> Result<PlanLifecycleOutcome, PlanError> {
    let outcome = |applied: bool, mode: PlanMode, operation: Option<PlanPauseOperation>| {
        PlanLifecycleOutcome {
            kind: PlanLifecycleKind::Resume,
            plan_id: plan_id.into(),
            applied,
            mode,
            stride_id: None,
            operation,
        }
    };
    let (restored_mode, operation) = match store.begin_plan_resume_operation(plan_id, now_ms)? {
        PlanResumeBegin::NotPaused { mode } => return Ok(outcome(false, mode, None)),
        PlanResumeBegin::Applied {
            restored_mode,
            operation,
        } => (restored_mode, operation),
    };
    let Some(operation) = operation else {
        return Ok(outcome(true, restored_mode, None));
    };
    let schedule_outcomes: Vec<_> = operation
        .schedules
        .iter()
        .filter(|schedule| schedule.pause_outcome == PlanPauseStepOutcome::Disabled)
        .map(|schedule| {
            (
                schedule.job_id.clone(),
                restore_schedule(ports, &schedule.job_id),
            )
        })
        .collect();
    let live: Vec<bool> = operation.agents.iter().map(is_live).collect();
    let title = store
        .get(plan_id)
        .map(|plan| plan.title)
        .unwrap_or_default();
    let notice = resume_notice_text(plan_id, &title, &restored_mode);
    let agent_outcomes: Vec<_> = operation
        .agents
        .iter()
        .zip(live)
        .map(|(agent, live)| {
            let result = if live {
                send_outcome(ports, &agent.tab_id, &notice)
            } else {
                PlanPauseStepOutcome::Unavailable
            };
            (agent.tab_id.clone(), result)
        })
        .collect();
    let operation = store.complete_plan_resume_operation(
        plan_id,
        &operation.id,
        &agent_outcomes,
        &schedule_outcomes,
        now_ms,
    )?;
    Ok(outcome(true, restored_mode, Some(operation)))
}

struct ServicePlanPausePorts<'a> {
    svc: &'a ServiceContext,
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
        let cron = self.svc.app_handle.try_state::<crate::CronState>()?;
        let enabled = cron.lock().get(job_id).map(|job| job.enabled);
        enabled
    }

    fn set_schedule_enabled(&self, job_id: &str, enabled: bool) -> Result<(), String> {
        let cron = self
            .svc
            .app_handle
            .try_state::<crate::CronState>()
            .ok_or("cron store unavailable")?;
        let wake = self
            .svc
            .app_handle
            .try_state::<crate::CronWake>()
            .ok_or("cron wake unavailable")?;
        self.svc
            .cron_enable(cron.inner(), wake.inner(), job_id, enabled)
            .map(|_| ())
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
        let Some(cron) = self.app_handle.try_state::<crate::CronState>() else {
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

    pub(crate) fn plan_pause_preview(
        &self,
        plan_id: &str,
    ) -> Result<PlanLifecyclePreview, TeamPlanError> {
        let plan = self.lifecycle_plan(plan_id)?;
        let agents = {
            let mux = self.mux.read();
            live_plan_agents(&mux, &plan)
        };
        let schedules = self.enabled_plan_schedules(&plan);
        Ok(pause_preview(&plan, &agents, &schedules))
    }

    pub(crate) fn plan_resume_preview(
        &self,
        plan_id: &str,
    ) -> Result<PlanLifecyclePreview, TeamPlanError> {
        let plan = self.lifecycle_plan(plan_id)?;
        let live = self.live_recorded_agents(&plan);
        Ok(resume_preview(&plan, |agent| live.contains(&agent.tab_id)))
    }

    pub(crate) fn pause_plan_operation(
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

    pub(crate) fn resume_plan_operation(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<PlanLifecycleOutcome, TeamPlanError> {
        {
            let mux = self.mux.read();
            self.valid_live_plan_team(&mux, plan_id)?;
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
