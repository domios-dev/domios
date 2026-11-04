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
        let writer_turn = self.mutation_writer.enter();
        let task = self.task_checkout_projection(plan_id, task_id, false)?;
        if task.live_shared {
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
            super::super::shared_checkout::validate_shared_files_at(
                &self.checkout_peer_facts(&task.checkout)?,
                plan_id,
                &task.id,
                &super::super::shared_checkout::canonical_path(&task.checkout),
                &files,
            )
            .map_err(PlanError::InvalidPlanText)?;
        }
        let task_id = task.id;
        self.with_task_rows_in_writer(
            &writer_turn,
            plan_id,
            &task_id,
            super::super::store_db::hot_write::TaskHistory::Core,
            now,
            |edit| {
                let task = &mut edit.task;
                task.files = files;
                task.updated_at = now;
                Ok(())
            },
        )
    }
}

#[cfg(test)]
mod tests;
