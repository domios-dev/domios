use super::super::document_queries::regression::unsigned_timestamp;
use super::*;
use crate::service_plan::TaskSpawnBindingRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_spawn_binding_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<TaskSpawnBindingRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let closed = unsigned_timestamp("a.row_json", "closed_at", false);
        let retired = unsigned_timestamp("a.row_json", "retired_at", false);
        let (exists,raw):(bool,Option<String>)=connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('lane',json(CASE WHEN json_type(t.row_json,'$.lane') IS NULL THEN '{{}}' ELSE t.row_json->'$.lane' END),
                    'attempt',json((SELECT json_object('id',json_extract(a.row_json,'$.id'),'executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'),
                        'session_id',json_extract(a.row_json,'$.session_id'),'session_binding_nonce',json_extract(a.row_json,'$.session_binding_nonce'),
                        'closed_at',json(CASE WHEN json_extract(a.row_json,'$.closed_at') IS NOT NULL THEN {closed} END),
                        'retired_at',json(CASE WHEN json_extract(a.row_json,'$.retired_at') IS NOT NULL THEN {retired} END))
                     FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')=?3 ORDER BY a.position LIMIT 1)))
                 FROM selected_task t JOIN plans p ON p.id=t.plan_id WHERE p.status='active')"),
            params![plan,task,attempt],|row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }

    pub(in crate::service_plan) fn task_attempt_exists_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(&format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM selected_task t JOIN plans p ON p.id=t.plan_id JOIN plan_task_attempts a
            ON a.plan_id=t.plan_id AND a.task_id=t.task_id WHERE p.status='active' AND json_extract(a.row_json,'$.id')=?3)"),params![plan,task,attempt],|row|column(row,0)).map_err(db_error)
    }
}
