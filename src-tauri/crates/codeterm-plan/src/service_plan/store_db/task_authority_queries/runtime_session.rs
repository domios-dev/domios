use super::*;
use crate::service_plan::TaskRuntimeSessionRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_runtime_session_projection(
        &self,
        task_ref: &str,
        attempt_id: &str,
        executor: &str,
        include_lane: bool,
    ) -> Result<TaskRuntimeSessionRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (plan_id,owner_epoch,attempt,lane): (String,u64,Option<String>,Option<String>) = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT t.plan_id,coalesce(json_extract(t.row_json,'$.interruption.owner_epoch'),0),
                (SELECT json_object('session_id',json_extract(a.row_json,'$.session_id'),
                    'session_binding_nonce',json_extract(a.row_json,'$.session_binding_nonce'),
                    'closed_at',json_extract(a.row_json,'$.closed_at'),'retired_at',json_extract(a.row_json,'$.retired_at'))
                 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                    AND json_extract(a.row_json,'$.id')=?2 AND a.executor_tab_id=?3 ORDER BY a.position LIMIT 1),
                CASE WHEN ?4 THEN coalesce(json_extract(t.row_json,'$.lane'),'{{}}') END
             FROM selected_task t"),
            params![task_ref,attempt_id,executor,include_lane],|row|Ok((column(row,0)?,column(row,1)?,column(row,2)?,column(row,3)?)),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        Ok(TaskRuntimeSessionRead {
            attempt: attempt.map(|raw| decode(&plan_id, &raw)).transpose()?,
            lane: lane.map(|raw| decode(&plan_id, &raw)).transpose()?,
            plan_id,
            owner_epoch,
        })
    }
}
