use super::*;
use crate::service_plan::TaskCheckpointAdoptionRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_checkpoint_adoption_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskCheckpointAdoptionRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (plan_id,checkpoint): (String,Option<String>) = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT plan_id,json_extract(row_json,'$.interruption.checkpoint') FROM selected_task"),
            [task_ref], |row| Ok((column(row,0)?,column(row,1)?)),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        Ok(TaskCheckpointAdoptionRead {
            checkpoint: checkpoint.map(|raw| decode(&plan_id, &raw)).transpose()?,
            plan_id,
        })
    }
}
