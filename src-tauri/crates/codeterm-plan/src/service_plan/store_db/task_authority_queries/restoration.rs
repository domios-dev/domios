use super::*;
use crate::service_plan::{ReviewBindingRestoreRead, TaskBindingRestoreRead};

impl PlanDatabase {
    pub(in crate::service_plan) fn task_binding_restore_projection(
        &self,
        task_ref: &str,
        attempt_id: &str,
    ) -> Result<TaskBindingRestoreRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: String = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT json_object('plan_id',t.plan_id,'task_id',t.task_id,
                'team_id',json_extract(p.header_json,'$.team_id'),
                'attempt_count',(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')=?2),
                'attempt_nonce',(SELECT a.session_binding_nonce FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')=?2 ORDER BY a.position LIMIT 1))
             FROM selected_task t JOIN plans p ON p.id=t.plan_id"),
            params![task_ref,attempt_id], |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        decode("task binding restoration", &raw)
    }

    pub(in crate::service_plan) fn review_binding_restore_projection(
        &self,
        plan_id: &str,
        task_ref: &str,
    ) -> Result<ReviewBindingRestoreRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw): (bool,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('task_id',t.task_id,'status',json_extract(t.row_json,'$.status'),
                    'owner_epoch',coalesce(json_extract(t.row_json,'$.interruption.owner_epoch'),0),
                    'attempts',json(CASE WHEN json_extract(t.row_json,'$.status')='in_review' THEN (SELECT json_group_array(json(facts)) FROM (
                        SELECT json_object('id',json_extract(a.row_json,'$.id'),'executor_tab_id',a.executor_tab_id,
                            'session_binding_nonce',a.session_binding_nonce) AS facts
                        FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND a.open=1
                        ORDER BY a.position LIMIT 2)) ELSE '[]' END)) FROM selected_task t)"),
            params![plan_id,task_ref],|row| Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan_id, &raw.ok_or(PlanError::TaskNotFound)?)
    }
}
