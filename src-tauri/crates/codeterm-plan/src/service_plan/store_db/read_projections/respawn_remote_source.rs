use super::{column, decode, record_query};
use crate::plan::TaskExecutionAttempt;
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::params;

impl PlanStore {
    pub fn task_respawn_acceptance_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Vec<String>, PlanError> {
        self.database.task_respawn_acceptance_projection(plan, task)
    }
    pub fn respawn_remote_source_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<TaskExecutionAttempt>, PlanError> {
        self.database
            .respawn_remote_source_projection(plan, task, attempt)
    }
}

impl PlanDatabase {
    fn task_respawn_acceptance_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Vec<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw):(bool,Option<String>)=connection.query_row(&format!(
            "{selected} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT CASE WHEN json_type(row_json,'$.acceptance_criteria') IS NULL THEN '[]' ELSE row_json->'$.acceptance_criteria' END FROM selected_task)",
            selected=super::super::task_authority_queries::SELECTED_TASK_IN_PLAN
        ),params![plan,task],|row|Ok((column(row,0)?,column(row,1)?))).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
    fn respawn_remote_source_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<TaskExecutionAttempt>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,task_exists,raw):(bool,bool,Option<String>)=connection.query_row(&format!(
            "{selected} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),EXISTS(SELECT 1 FROM selected_task),
                (SELECT a.row_json FROM selected_task t JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                    WHERE json_extract(a.row_json,'$.id')=?3 AND json_extract(a.row_json,'$.mesh_peer') IS NOT NULL ORDER BY a.position LIMIT 1)",
            selected=super::super::task_authority_queries::SELECTED_TASK_IN_PLAN
        ),params![plan,task,attempt],|row|Ok((column(row,0)?,column(row,1)?,column(row,2)?))).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        if !task_exists {
            return Err(PlanError::TaskNotFound);
        }
        raw.map(|raw| decode(plan, &raw)).transpose()
    }
}

#[cfg(test)]
mod tests;
