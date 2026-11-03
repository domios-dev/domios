use super::db_error;
use crate::service_plan::change_feed::CommittedPlanChange;
use crate::service_plan::PlanError;
use rusqlite::{Connection, OptionalExtension};

pub(super) fn revision(connection: &Connection, plan_id: &str) -> Result<i64, PlanError> {
    connection
        .query_row(
            "SELECT coalesce(json_extract(header_json, '$._revision'), 0) FROM plans WHERE id=?1",
            [plan_id],
            |row| row.get(0),
        )
        .optional()
        .map(|revision| revision.unwrap_or(0))
        .map_err(db_error)
}

#[cfg(test)]
mod tests;

pub(super) fn next_revision(previous: i64, operation_id: i64) -> Result<i64, PlanError> {
    previous
        .checked_add(1)
        .map(|next| next.max(operation_id))
        .ok_or_else(|| PlanError::Io("Plan revision exhausted".into()))
}

pub(super) fn change(
    plan_id: &str,
    revision: i64,
    previous_revision: i64,
    task_ids: Vec<String>,
    reset: bool,
    deleted: bool,
) -> CommittedPlanChange {
    CommittedPlanChange {
        plan_id: plan_id.into(),
        revision: revision.to_string(),
        previous_revision: previous_revision.to_string(),
        task_ids,
        reset,
        deleted,
    }
}
