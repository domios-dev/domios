use super::super::document_queries::regression::unsigned_timestamp;
use super::*;
use crate::service_plan::task_authority_projection::TaskRespawnSourceRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_respawn_source_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<TaskRespawnSourceRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let closed = unsigned_timestamp("a.row_json", "closed_at", false);
        let retired = unsigned_timestamp("a.row_json", "retired_at", false);
        let raw:Option<String>=connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT json_object('attempt_id',json_extract(a.row_json,'$.id'),
                'executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'),
                'closed',json(CASE WHEN json_extract(a.row_json,'$.closed_at') IS NULL THEN 'false'
                    WHEN length({closed})>0 THEN 'true' END),
                'retired',json(CASE WHEN json_extract(a.row_json,'$.retired_at') IS NULL THEN 'false'
                    WHEN length({retired})>0 THEN 'true' END))
             FROM selected_task t JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
             ORDER BY a.position DESC LIMIT 1"),params![plan,task],|row|column(row,0),
        ).optional().map_err(db_error)?;
        raw.map(|raw| decode(plan, &raw)).transpose()
    }
}
