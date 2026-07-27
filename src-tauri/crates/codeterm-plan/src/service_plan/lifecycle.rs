use super::mutations::plan_contains_id;
use super::{PlanError, PlanStore};
use crate::plan::{AgentRef, Milestone, PlanMode, TaskStatus};

impl PlanStore {
    pub fn pause_plan(
        &self,
        plan_id: &str,
        reason: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(PlanError::PauseRequiresReason);
        }
        let mut mid = String::new();
        self.with_plan(plan_id, now_ms, |p| {
            if p.mode == PlanMode::Paused {
                return Ok(());
            }
            mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            p.paused_from_mode = Some(p.mode.clone());
            p.mode = PlanMode::Paused;
            p.strides.push(Milestone {
                id: mid.clone(),
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
            Ok(())
        })?;
        Ok(mid)
    }

    pub fn resume_plan(&self, plan_id: &str, now_ms: u64) -> Result<PlanMode, PlanError> {
        let mut mode = PlanMode::Planning;
        self.with_plan(plan_id, now_ms, |p| {
            if p.team_id
                .as_ref()
                .is_none_or(|team_id| team_id.trim().is_empty())
            {
                return Err(PlanError::ActivePlanRequiresTeam {
                    plan_id: plan_id.into(),
                });
            }
            mode = p.paused_from_mode.take().unwrap_or(PlanMode::Execution);
            if p.mode == PlanMode::Paused {
                p.mode = mode.clone();
            } else {
                mode = p.mode.clone();
            }
            Ok(())
        })?;
        Ok(mode)
    }

    pub fn pause_task(
        &self,
        plan_id: &str,
        task_id: &str,
        reason: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(PlanError::TaskPauseRequiresReason);
        }
        let mut mid = String::new();
        self.with_plan(plan_id, now_ms, |p| {
            let idx = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if p.tasks[idx].status.is_terminal() {
                return Err(PlanError::TransitionDenied {
                    from: p.tasks[idx].status.clone(),
                    to: TaskStatus::Paused,
                });
            }
            if p.tasks[idx].status == TaskStatus::InReview {
                return Err(PlanError::TransitionDenied {
                    from: TaskStatus::InReview,
                    to: TaskStatus::Paused,
                });
            }
            mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            let task = &mut p.tasks[idx];
            if task.status != TaskStatus::Paused {
                task.status_changed_at = now_ms;
            }
            task.status = TaskStatus::Paused;
            task.pause_reason = Some(reason.to_string());
            let assigned = task.assigned_tab_id.clone();
            super::task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
            task.assigned_tab_id = None;
            task.lane.tab_id = None;
            task.milestones.push(Milestone {
                id: mid.clone(),
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
            task.updated_at = now_ms;
            Ok(())
        })?;
        Ok(mid)
    }

    pub fn resume_task(
        &self,
        plan_id: &str,
        task_id: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let mut mid = String::new();
        self.with_plan(plan_id, now_ms, |p| {
            let idx = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if p.tasks[idx].status != TaskStatus::Paused {
                return Err(PlanError::TransitionDenied {
                    from: p.tasks[idx].status.clone(),
                    to: TaskStatus::Pending,
                });
            }
            mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            let task = &mut p.tasks[idx];
            task.status = TaskStatus::Pending;
            task.status_changed_at = now_ms;
            task.pause_reason = None;
            task.milestones.push(Milestone {
                id: mid.clone(),
                from,
                kind: "decision".into(),
                text: "resumed: returned to pending".into(),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            task.updated_at = now_ms;
            Ok(())
        })?;
        Ok(mid)
    }
}

#[cfg(test)]
mod tests;
