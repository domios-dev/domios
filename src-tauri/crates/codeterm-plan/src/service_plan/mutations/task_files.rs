use super::{PlanError, PlanStore};
use crate::plan::TaskFiles;

impl PlanStore {
    pub fn update_task_files(
        &self,
        plan_id: &str,
        task_id: &str,
        files: TaskFiles,
        now: u64,
    ) -> Result<(), PlanError> {
        let files = files.validated().map_err(PlanError::InvalidPlanText)?;
        let _writer_turn = self.mutation_writer.enter();
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .ok_or(PlanError::TaskNotFound)?;
        if super::super::shared_checkout::live_shared(task) {
            if task
                .files
                .edit
                .iter()
                .any(|path| !files.owns_edit_path(path))
            {
                return Err(PlanError::InvalidPlanText(
                    "cannot release edit paths from a live shared attempt; close the attempt first"
                        .into(),
                ));
            }
            super::super::shared_checkout::validate_shared_files(
                &self.checkout_peer_plans(&plan),
                &plan,
                task,
                &files,
            )
            .map_err(PlanError::InvalidPlanText)?;
        }
        self.with_plan_in_writer(plan_id, now, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            task.files = files;
            task.updated_at = now;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
