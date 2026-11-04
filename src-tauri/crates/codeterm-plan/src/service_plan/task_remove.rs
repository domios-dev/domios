use super::{PlanError, PlanStore};

impl PlanStore {
    pub fn rm_task(&self, plan_id: &str, task_id: &str, now_ms: u64) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        let (ids, receipt) = self.database.remove_tasks(plan_id, Some(task_id), now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        for id in ids {
            self.remove_task_attachment_dir(plan_id, &id)?;
        }
        Ok(())
    }

    pub fn rm_all_tasks(&self, plan_id: &str, now_ms: u64) -> Result<usize, PlanError> {
        let turn = self.mutation_writer.enter();
        let (ids, receipt) = self.database.remove_tasks(plan_id, None, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(ids.len())
    }
}

#[cfg(test)]
mod tests;
