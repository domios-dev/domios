use super::{column, db_error, decode, record_query, PlanDatabase};
use crate::service_plan::{
    document_projection::cleanup::{CleanupPlanRead, CleanupTaskRead},
    PlanError,
};
use rusqlite::OptionalExtension;

impl PlanDatabase {
    pub(in crate::service_plan) fn cleanup_debt_input(
        &self,
        id: &str,
        task: Option<&str>,
    ) -> Result<Option<CleanupPlanRead>, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        record_query();
        let document: Option<String> = transaction.query_row(
            "SELECT json_object('id',id,'project_path',project_path,'canonical_project_path',coalesce(json_extract(header_json,'$.canonical_project_path'),'')) FROM plans WHERE id=?1",
            [id],|row| column(row,0),
        ).optional().map_err(db_error)?;
        let Some(document) = document else {
            return Ok(None);
        };
        let mut plan: CleanupPlanRead = decode(id, &document)?;
        record_query();
        plan.tasks = {
            let mut statement = transaction.prepare_cached(
                "SELECT json_object('id',task_id,'name',json_extract(row_json,'$.name'),'status',status,
                    'cleanup_status',json_extract(row_json,'$.lane.cleanup_status'),
                    'cleanup_hints',CASE WHEN json_type(row_json,'$.lane.cleanup_hints') IS NULL THEN json('[]') ELSE json_extract(row_json,'$.lane.cleanup_hints') END,
                    'tab_id',coalesce(json_extract(row_json,'$.lane.tab_id'),json_extract(row_json,'$.assigned_tab_id')),
                    'worktree_path',json_extract(row_json,'$.lane.worktree_path'),'branch',json_extract(row_json,'$.lane.branch'))
                 FROM plan_tasks WHERE plan_id=?1 AND (?2 IS NULL OR task_id=?2) AND status IN ('done','cancelled') ORDER BY position"
            ).map_err(db_error)?;
            let rows = statement
                .query_map(rusqlite::params![id, task], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            rows.map(|row| decode::<CleanupTaskRead>(id, &row.map_err(db_error)?))
                .collect::<Result<Vec<_>, PlanError>>()?
        };
        transaction.commit().map_err(db_error)?;
        Ok(Some(plan))
    }
}
