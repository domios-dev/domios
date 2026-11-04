use super::{column, decode, record_query};
use crate::plan::TaskExecutionAttempt;
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::{params, OptionalExtension};

const SOURCE: &str = "json_object(
    'id',json_extract(a.row_json,'$.id'),
    'executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'),
    'provider',json_extract(a.row_json,'$.provider'),
    'session_id',json_extract(a.row_json,'$.session_id'),
    'session_binding_nonce',json_extract(a.row_json,'$.session_binding_nonce'),
    'mesh_peer',json_extract(a.row_json,'$.mesh_peer'))";

impl PlanStore {
    pub fn executor_source_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<TaskExecutionAttempt>, PlanError> {
        self.database
            .executor_source_projection(plan, task, attempt)
    }

    pub fn executor_sources_for_tab_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<(String, String, TaskExecutionAttempt)>, PlanError> {
        self.database.executor_sources_for_tab_projection(tab)
    }
}

impl PlanDatabase {
    fn executor_source_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<TaskExecutionAttempt>, PlanError> {
        self.ensure_plan_exists(plan)?;
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String> = connection.query_row(
            &format!("{} SELECT CASE WHEN a.attempt_id IS NOT NULL THEN {SOURCE} END
                FROM selected_task t LEFT JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                    AND json_extract(a.row_json,'$.id')=?3 ORDER BY a.position LIMIT 1",
                super::super::task_authority_queries::SELECTED_TASK_IN_PLAN),
            params![plan,task,attempt], |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        raw.map(|raw| decode(plan, &raw)).transpose()
    }

    fn executor_sources_for_tab_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<(String, String, TaskExecutionAttempt)>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare(&format!(
                "SELECT p.id,t.task_id,{SOURCE} FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
                JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                WHERE a.position=(SELECT max(candidate.position) FROM plan_task_attempts candidate
                    WHERE candidate.plan_id=t.plan_id AND candidate.task_id=t.task_id
                        AND json_extract(candidate.row_json,'$.executor_tab_id')=?1
                        AND json_extract(candidate.row_json,'$.closed_at') IS NULL)
                ORDER BY p.updated_at DESC,p.id,t.position LIMIT 2"
            ))
            .map_err(db_error)?;
        let rows = statement
            .query_map([tab], |row| {
                Ok((
                    column::<String>(row, 0)?,
                    column::<String>(row, 1)?,
                    column::<String>(row, 2)?,
                ))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (plan, task, raw) = row.map_err(db_error)?;
            let attempt = decode(&plan, &raw)?;
            Ok((plan, task, attempt))
        })
        .collect()
    }
}

#[cfg(test)]
mod tests;
