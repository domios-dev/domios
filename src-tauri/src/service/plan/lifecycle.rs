use super::mutations::plan_contains_id;
use super::{PlanError, PlanStore};
use crate::plan::{AgentRef, Milestone, TaskStatus};

// Park/unpark task lifecycle. Parking defers an open task WITHOUT closing it:
// it leaves the heartbeat open-work set (see TaskStatus::counts_as_open_work),
// so nudges no longer treat it as pending dispatch. Both transitions record a
// working-point stride so the deferral (and its reason) is auditable.
impl PlanStore {
    pub fn park_task(
        &self,
        plan_id: &str,
        task_id: &str,
        reason: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(PlanError::ParkRequiresReason);
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
                    to: TaskStatus::Parked,
                });
            }
            mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            let task = &mut p.tasks[idx];
            if task.status != TaskStatus::Parked {
                task.status_changed_at = now_ms;
            }
            task.status = TaskStatus::Parked;
            task.park_reason = Some(reason.to_string());
            let assigned = task.assigned_pane_id.clone();
            super::task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
            task.assigned_pane_id = None;
            task.lane.pane_id = None;
            task.milestones.push(Milestone {
                id: mid.clone(),
                from,
                kind: "decision".into(),
                text: format!("parked: {reason}"),
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

    pub fn unpark_task(
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
            if p.tasks[idx].status != TaskStatus::Parked {
                return Err(PlanError::TransitionDenied {
                    from: p.tasks[idx].status.clone(),
                    to: TaskStatus::Pending,
                });
            }
            mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            let task = &mut p.tasks[idx];
            task.status = TaskStatus::Pending;
            task.status_changed_at = now_ms;
            task.park_reason = None;
            task.milestones.push(Milestone {
                id: mid.clone(),
                from,
                kind: "decision".into(),
                text: "unparked: returned to pending".into(),
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
