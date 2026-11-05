use super::{column, decode, record_query};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::task_attempts::ExecutorSourceRead;
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
    ) -> Result<Option<ExecutorSourceRead>, PlanError> {
        self.database
            .executor_source_projection(plan, task, attempt)
    }

    pub fn executor_sources_for_tab_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<(String, String, ExecutorSourceRead)>, PlanError> {
        self.database.executor_sources_for_tab_projection(tab)
    }
}

impl PlanDatabase {
    fn executor_source_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<ExecutorSourceRead>, PlanError> {
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
    ) -> Result<Vec<(String, String, ExecutorSourceRead)>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare(&format!(
                "WITH latest AS MATERIALIZED (
                    SELECT plan_id,task_id,max(position) AS position FROM plan_task_attempts
                    WHERE executor_tab_id=?1 AND json_extract(row_json,'$.executor_tab_id')=?1
                        AND json_extract(row_json,'$.closed_at') IS NULL GROUP BY plan_id,task_id
                ) SELECT p.id,t.task_id,{SOURCE} FROM latest selected
                JOIN plans p ON p.id=selected.plan_id
                JOIN plan_tasks t ON t.plan_id=selected.plan_id AND t.task_id=selected.task_id
                JOIN plan_task_attempts a ON a.plan_id=selected.plan_id AND a.task_id=selected.task_id AND a.position=selected.position
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
