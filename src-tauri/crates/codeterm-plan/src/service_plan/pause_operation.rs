use super::mutations::plan_contains_id;
use super::{PlanError, PlanStore};
use crate::plan::{
    AgentRef, Milestone, Plan, PlanMode, PlanPauseAgent, PlanPauseOperation, PlanPauseSchedule,
    PlanPauseStage, PlanPauseStepOutcome, PlanResumeRecord, PLAN_PAUSE_EFFECTS,
};

#[derive(Debug, Clone, PartialEq)]
pub enum PlanPauseBegin {
    Applied {
        operation: PlanPauseOperation,
        stride_id: String,
    },
    AlreadyPaused {
        operation: Option<PlanPauseOperation>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanResumeBegin {
    Applied {
        restored_mode: PlanMode,
        operation: Option<PlanPauseOperation>,
    },
    NotPaused {
        mode: PlanMode,
    },
}

/// Reminders, heartbeat nudges and plan system notices stay silent while paused.
pub fn plan_notices_suppressed(plan: &Plan) -> bool {
    plan.mode == PlanMode::Paused
}

/// The pause operation still owning the plan's paused state, if any.
pub fn open_pause_operation(plan: &Plan) -> Option<&PlanPauseOperation> {
    if plan.mode != PlanMode::Paused {
        return None;
    }
    plan.pause_operations
        .last()
        .filter(|operation| operation.resume.is_none())
}

impl PlanStore {
    pub fn begin_plan_pause_operation(
        &self,
        plan_id: &str,
        reason: &str,
        from: AgentRef,
        agents: Vec<PlanPauseAgent>,
        schedules: Vec<PlanPauseSchedule>,
        now_ms: u64,
    ) -> Result<PlanPauseBegin, PlanError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(PlanError::PauseRequiresReason);
        }
        self.with_plan(plan_id, now_ms, |p| {
            if p.mode == PlanMode::Paused {
                return Ok(PlanPauseBegin::AlreadyPaused {
                    operation: open_pause_operation(p).cloned(),
                });
            }
            let operation_id = crate::plan::ids::mint_id("pz", |candidate| {
                p.pause_operations.iter().any(|op| op.id == candidate)
            });
            let stride_id = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            let paused_from_mode = p.mode.clone();
            let operation = PlanPauseOperation {
                id: operation_id,
                paused_at: now_ms,
                paused_from_mode: paused_from_mode.clone(),
                reason: reason.to_string(),
                from: from.clone(),
                agents,
                schedules,
                effects: PLAN_PAUSE_EFFECTS.to_vec(),
                stage: PlanPauseStage::Applying,
                resume: None,
            };
            p.paused_from_mode = Some(paused_from_mode);
            p.mode = PlanMode::Paused;
            p.pause_operations.push(operation.clone());
            p.strides.push(Milestone {
                id: stride_id.clone(),
                from,
                kind: "decision".into(),
                text: format!("paused: {reason}"),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(PlanPauseBegin::Applied {
                operation,
                stride_id,
            })
        })
    }

    pub fn record_plan_pause_outcomes(
        &self,
        plan_id: &str,
        operation_id: &str,
        agent_outcomes: &[(String, PlanPauseStepOutcome)],
        schedule_outcomes: &[(String, PlanPauseStepOutcome)],
        now_ms: u64,
    ) -> Result<PlanPauseOperation, PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let operation = operation_mut(p, operation_id)?;
            for (tab_id, outcome) in agent_outcomes {
                if let Some(agent) = operation.agents.iter_mut().find(|a| &a.tab_id == tab_id) {
                    agent.pause_notice = outcome.clone();
                }
            }
            for (job_id, outcome) in schedule_outcomes {
                if let Some(schedule) = operation.schedules.iter_mut().find(|s| &s.job_id == job_id)
                {
                    schedule.pause_outcome = outcome.clone();
                }
            }
            operation.stage = operation.pause_stage_from_outcomes();
            Ok(operation.clone())
        })
    }

    /// Restores the paused-from mode and claims the open pause record in one
    /// transition, so a concurrent or repeated resume observes `NotPaused`.
    pub fn begin_plan_resume_operation(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<PlanResumeBegin, PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if p.team_id
                .as_ref()
                .is_none_or(|team_id| team_id.trim().is_empty())
            {
                return Err(PlanError::ActivePlanRequiresTeam {
                    plan_id: plan_id.into(),
                });
            }
            if p.mode != PlanMode::Paused {
                return Ok(PlanResumeBegin::NotPaused {
                    mode: p.mode.clone(),
                });
            }
            let restored_mode = p.paused_from_mode.take().unwrap_or(PlanMode::Execution);
            let open_index = p
                .pause_operations
                .iter()
                .rposition(|operation| operation.resume.is_none())
                .filter(|index| *index + 1 == p.pause_operations.len());
            p.mode = restored_mode.clone();
            let operation = open_index.map(|index| {
                let operation = &mut p.pause_operations[index];
                operation.resume = Some(PlanResumeRecord {
                    resumed_at: now_ms,
                    restored_mode: restored_mode.clone(),
                    stage: PlanPauseStage::Applying,
                });
                operation.clone()
            });
            Ok(PlanResumeBegin::Applied {
                restored_mode,
                operation,
            })
        })
    }

    pub fn complete_plan_resume_operation(
        &self,
        plan_id: &str,
        operation_id: &str,
        agent_outcomes: &[(String, PlanPauseStepOutcome)],
        schedule_outcomes: &[(String, PlanPauseStepOutcome)],
        now_ms: u64,
    ) -> Result<PlanPauseOperation, PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let operation = operation_mut(p, operation_id)?;
            for (tab_id, outcome) in agent_outcomes {
                if let Some(agent) = operation.agents.iter_mut().find(|a| &a.tab_id == tab_id) {
                    agent.resume_notice = Some(outcome.clone());
                }
            }
            for (job_id, outcome) in schedule_outcomes {
                if let Some(schedule) = operation.schedules.iter_mut().find(|s| &s.job_id == job_id)
                {
                    schedule.resume_outcome = Some(outcome.clone());
                }
            }
            let stage = operation.resume_stage_from_outcomes();
            if let Some(resume) = operation.resume.as_mut() {
                resume.stage = stage;
            }
            Ok(operation.clone())
        })
    }
}

fn operation_mut<'a>(
    plan: &'a mut Plan,
    operation_id: &str,
) -> Result<&'a mut PlanPauseOperation, PlanError> {
    plan.pause_operations
        .iter_mut()
        .find(|operation| operation.id == operation_id)
        .ok_or(PlanError::NotFound)
}
