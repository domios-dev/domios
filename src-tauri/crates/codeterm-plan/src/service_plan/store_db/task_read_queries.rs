use super::db_error;
use super::read_projections::{column, decode, record_query};
use crate::plan::Task;
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};

pub(super) const TASK_HISTORY_LIMIT: usize = 50;
pub(super) const TASK_ATTEMPT_LIMIT: usize = 8;

mod detail;

pub(super) fn selected_task(
    connection: &Connection,
    plan_id: &str,
    task_id: &str,
) -> Result<Task, PlanError> {
    record_query();
    let history_limit = TASK_HISTORY_LIMIT as i64;
    let attempt_limit = TASK_ATTEMPT_LIMIT as i64;
    let document: String = connection.query_row(
        "SELECT json_set(row_json,
            '$.review_evidence',json((SELECT json_group_array(json(value)) FROM (
                SELECT value FROM (SELECT key,value FROM json_each(t.row_json,'$.review_evidence') ORDER BY key DESC LIMIT ?3) ORDER BY key))),
            '$.review_audit',json((SELECT json_group_array(json(value)) FROM (
                SELECT value FROM (SELECT key,value FROM json_each(t.row_json,'$.review_audit') ORDER BY key DESC LIMIT ?3) ORDER BY key))))
         FROM plan_tasks t WHERE plan_id = ?1 AND task_id = ?2",
        params![plan_id,task_id,history_limit], |row| column(row,0),
    ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
    let mut task: Task = decode(plan_id, &document)?;
    record_query();
    let mut milestones = connection
        .prepare_cached(
            "SELECT row_json FROM (SELECT seq,row_json FROM plan_task_milestones
         WHERE plan_id=?1 AND task_id=?2 ORDER BY seq DESC LIMIT ?3) ORDER BY seq",
        )
        .map_err(db_error)?;
    for document in milestones
        .query_map(params![plan_id, task_id, history_limit], |row| {
            column::<String>(row, 0)
        })
        .map_err(db_error)?
    {
        task.milestones
            .push(decode(plan_id, &document.map_err(db_error)?)?);
    }
    record_query();
    let mut attempts = connection
        .prepare_cached(
            "SELECT row_json FROM (SELECT position,row_json FROM plan_task_attempts
         WHERE plan_id=?1 AND task_id=?2 ORDER BY position DESC LIMIT ?3) ORDER BY position",
        )
        .map_err(db_error)?;
    for document in attempts
        .query_map(params![plan_id, task_id, attempt_limit], |row| {
            column::<String>(row, 0)
        })
        .map_err(db_error)?
    {
        task.execution_attempts
            .push(decode(plan_id, &document.map_err(db_error)?)?);
    }
    Ok(task)
}
