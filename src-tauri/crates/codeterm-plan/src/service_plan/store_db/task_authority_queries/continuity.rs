use super::*;
use crate::service_plan::{TaskContinuityDigest, TaskContinuityRead};

mod digest;

pub(in crate::service_plan::store_db) fn register_functions(
    connection: &rusqlite::Connection,
) -> Result<(), PlanError> {
    digest::register(connection)
}

pub(super) const CONTINUITY_DIGEST: &str = "(SELECT domios_task_continuity(phase,document,plan_id ORDER BY phase,position) FROM (
    SELECT 0 AS phase,0 AS position,plan_id,json_remove(row_json,'$.milestones','$.execution_attempts') AS document FROM selected_task
    UNION ALL SELECT 1,m.seq,m.plan_id,m.row_json FROM plan_task_milestones m
        JOIN selected_task t ON t.plan_id=m.plan_id AND t.task_id=m.task_id
    UNION ALL SELECT 2,a.position,a.plan_id,a.row_json FROM plan_task_attempts a
        JOIN selected_task t ON t.plan_id=a.plan_id AND t.task_id=a.task_id))";

impl PlanDatabase {
    pub(in crate::service_plan) fn task_continuity_projection(
        &self,
        reference: &str,
    ) -> Result<TaskContinuityRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let row: Option<(String,String,String,Option<String>,u64,Option<String>,Option<String>,Vec<u8>)> = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT plan_id,task_id,json_quote(json_extract(row_json,'$.status')),assigned_tab_id,
                coalesce(json_extract(row_json,'$.interruption.owner_epoch'),0),
                json_extract(row_json,'$.interruption.owner_tab_id'),json_extract(row_json,'$.interruption.owner_session_id'),
                {CONTINUITY_DIGEST} FROM selected_task"),
            [reference], |row| Ok((column(row,0)?,column(row,1)?,column(row,2)?,column(row,3)?,column(row,4)?,column(row,5)?,column(row,6)?,column(row,7)?)),
        ).optional().map_err(db_error)?;
        let (
            plan_id,
            task_id,
            status,
            assigned_tab_id,
            owner_epoch,
            owner_tab_id,
            owner_session_id,
            bytes,
        ) = row.ok_or(PlanError::TaskNotFound)?;
        Ok(TaskContinuityRead {
            status: decode(&plan_id, &status)?,
            digest: TaskContinuityDigest::from_bytes(bytes)?,
            plan_id,
            task_id,
            assigned_tab_id,
            owner_epoch,
            owner_tab_id,
            owner_session_id,
        })
    }
}
