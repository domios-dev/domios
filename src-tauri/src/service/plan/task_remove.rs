use super::readback::pending_after_verified;
use super::{PlanError, PlanStore};

impl PlanStore {
    pub fn rm_task(&self, plan_id: &str, task_id: &str, now_ms: u64) -> Result<(), PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let canonical_id = plan
            .tasks
            .iter()
            .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
            .map(|task| task.id.clone())
            .ok_or(PlanError::TaskNotFound)?;
        let write = self.with_plan(plan_id, now_ms, |p| {
            let pos = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            p.tasks.remove(pos);
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_removed(plan_id, task_id),
            Some(task_id.to_string()),
        )?;
        self.remove_task_attachment_dir(&plan, &canonical_id)
    }

    pub fn rm_all_tasks(&self, plan_id: &str, now_ms: u64) -> Result<usize, PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let count = p.tasks.len();
            p.tasks.clear();
            p.heartbeat.current_focus = None;
            p.heartbeat.active_lanes.clear();
            p.mode = crate::plan::PlanMode::Planning;
            Ok(count)
        })
    }
}
