use codeterm_types::plan::{
    AgentRef, Plan, PlanMode, PlanPauseAgent, PlanPauseEffect, PlanPauseOperation, PlanPauseRole,
    PlanPauseSchedule, PlanPauseStepOutcome, PLAN_PAUSE_EFFECTS,
};
use serde::Serialize;

use super::pause_operation::{open_pause_operation, PlanPauseBegin, PlanResumeBegin};
use super::{PlanError, PlanStore};

/// Side effects a plan pause or resume performs outside the plan record.
pub trait PlanPausePorts {
    fn send_notice(&self, tab_id: &str, text: &str) -> Result<(), String>;
    fn schedule_enabled(&self, job_id: &str) -> Option<bool>;
    fn set_schedule_enabled(&self, job_id: &str, enabled: bool) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanLifecycleKind {
    Pause,
    Resume,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanLifecyclePreviewAgent {
    pub tab_id: String,
    pub name: String,
    pub role: PlanPauseRole,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanLifecyclePreviewSchedule {
    pub job_id: String,
    pub name: String,
    pub prior_enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanLifecyclePreview {
    pub kind: PlanLifecycleKind,
    pub plan_id: String,
    pub plan_title: String,
    pub mode: PlanMode,
    pub applicable: bool,
    pub not_applicable_reason: Option<String>,
    pub operation_id: Option<String>,
    pub restore_mode: Option<PlanMode>,
    pub agents: Vec<PlanLifecyclePreviewAgent>,
    pub schedules: Vec<PlanLifecyclePreviewSchedule>,
    pub effects: Vec<PlanPauseEffect>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanLifecycleOutcome {
    pub kind: PlanLifecycleKind,
    pub plan_id: String,
    pub applied: bool,
    pub mode: PlanMode,
    pub stride_id: Option<String>,
    pub operation: Option<PlanPauseOperation>,
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

pub fn pause_preview(
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

pub fn resume_preview(
    plan: &Plan,
    is_live: impl Fn(&PlanPauseAgent) -> bool,
) -> PlanLifecyclePreview {
    let reason = if plan.mode != PlanMode::Paused {
        Some("Plan is not paused")
    } else if plan
        .team_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .is_none()
    {
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
pub fn execute_plan_pause(
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

pub fn execute_plan_resume(
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

#[cfg(test)]
mod tests;
