use super::*;
use crate::service_plan::{RemoteRecoveryAttemptRead, RemoteWorkerBindingRead};

const REMOTE_BINDING_FIELDS: &str = "'id',json_extract(a.row_json,'$.id'),
    'executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'),
    'mesh_peer',json_extract(a.row_json,'$.mesh_peer'),
    'session_binding_nonce',json_extract(a.row_json,'$.session_binding_nonce'),
    'closed_at',json_extract(a.row_json,'$.closed_at'),'retired_at',json_extract(a.row_json,'$.retired_at')";

impl PlanDatabase {
    pub(in crate::service_plan) fn remote_worker_binding_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<RemoteWorkerBindingRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String> = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT json_object({REMOTE_BINDING_FIELDS})
                FROM selected_task t JOIN plans p ON p.id=t.plan_id AND p.status='active' JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                WHERE json_extract(a.row_json,'$.id')=?3 ORDER BY a.position LIMIT 1"),
            params![plan,task,attempt],|row| column(row,0),
        ).optional().map_err(db_error)?;
        raw.map(|raw| decode(plan, &raw)).transpose()
    }

    pub(in crate::service_plan) fn remote_recovery_attempts_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Vec<RemoteRecoveryAttemptRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection.prepare(&format!("{SELECTED_TASK_IN_PLAN}
            SELECT CASE WHEN a.position IS NOT NULL THEN json_object({REMOTE_BINDING_FIELDS},
                'acknowledged_at',json_extract(a.row_json,'$.acknowledged_at'),
                'assigned_at',json_extract(a.row_json,'$.assigned_at'),
                'remote_dispatch_state',json_extract(a.row_json,'$.remote_dispatch_state')) END
            FROM selected_task t JOIN plans p ON p.id=t.plan_id AND p.status='active' LEFT JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                AND json_extract(a.row_json,'$.mesh_peer') IS NOT NULL
                AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL
                AND json_extract(a.row_json,'$.report') IS NULL ORDER BY a.position")).map_err(db_error)?;
        let mut rows = statement.query(params![plan, task]).map_err(db_error)?;
        let mut found = false;
        let mut attempts = Vec::new();
        while let Some(row) = rows.next().map_err(db_error)? {
            found = true;
            if let Some(raw) = column::<Option<String>>(row, 0).map_err(db_error)? {
                attempts.push(decode(plan, &raw)?);
            }
        }
        if !found {
            return Err(PlanError::TaskNotFound);
        }
        Ok(attempts)
    }
}
