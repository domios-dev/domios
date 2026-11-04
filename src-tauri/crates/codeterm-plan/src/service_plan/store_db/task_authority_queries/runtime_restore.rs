use super::*;
use crate::service_plan::{RuntimeAttemptRestoreQuery, RuntimeAttemptRestoreRead};

impl PlanDatabase {
    pub(in crate::service_plan) fn runtime_attempt_restore_projection(
        &self,
        query: &RuntimeAttemptRestoreQuery<'_>,
    ) -> Result<RuntimeAttemptRestoreRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: String = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK}, candidates_for_tab AS (
                SELECT json_extract(a.row_json,'$.id') AS id,a.position FROM plan_task_attempts a
                JOIN selected_task t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
                WHERE a.executor_tab_id=?2 AND json_extract(a.row_json,'$.executor_tab_id')=?2 AND a.open=1 ORDER BY a.position)
             SELECT json_object('plan_id',t.plan_id,
                'candidates',json((SELECT json_group_array(id) FROM (SELECT id FROM candidates_for_tab ORDER BY position))),
                'restart_acknowledged_at',CASE WHEN ?3 IS NOT NULL AND ?4 IS NOT NULL AND ?5 IS NOT NULL AND ?5=?6
                    THEN (SELECT json_extract(a.row_json,'$.acknowledged_at') FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                        AND json_extract(a.row_json,'$.id')=(SELECT id FROM candidates_for_tab ORDER BY position LIMIT 1)
                        AND json_extract(a.row_json,'$.acknowledged_at') IS NOT NULL
                        AND json_extract(a.row_json,'$.provider')=?3 AND json_extract(a.row_json,'$.session_id')=?4
                        AND json_extract(a.row_json,'$.session_binding_nonce')=?5
                        ORDER BY a.position LIMIT 1) END) FROM selected_task t"),
            params![query.task_ref,query.executor_tab_id,query.provider,query.session_id,query.nonce,query.reported_nonce],
            |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        decode("runtime attempt restoration", &raw)
    }
}
