use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::ui_projections::task_reads::{PlanTaskHistoryItem, PlanTaskHistoryPage};
use crate::service_plan::{task_history::TaskHistoryKind, PlanError};
use rusqlite::{params, Connection};
use serde_json::Value;

pub(super) struct HistoryPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<i64>,
}

pub(super) fn page<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    plan_id: &str,
    task_id: &str,
    kind: TaskHistoryKind,
    before: Option<i64>,
    limit: usize,
) -> Result<HistoryPage<T>, PlanError> {
    let (table, position) = match kind {
        TaskHistoryKind::Milestones => ("plan_task_milestones", "seq"),
        TaskHistoryKind::Attempts => ("plan_task_attempts", "position"),
    };
    record_query();
    let sql = format!("SELECT {position},row_json FROM {table} WHERE plan_id=?1 AND task_id=?2 AND (?3 IS NULL OR {position}<?3) ORDER BY {position} DESC LIMIT ?4");
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let limit = limit.clamp(1, 50);
    let rows = statement
        .query_map(
            params![plan_id, task_id, before, (limit + 1) as i64],
            |row| Ok((column::<i64>(row, 0)?, column::<String>(row, 1)?)),
        )
        .map_err(db_error)?;
    let mut rows = rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    let next_cursor = if has_more {
        rows.last().map(|row| row.0)
    } else {
        None
    };
    let items = rows
        .into_iter()
        .rev()
        .map(|(_, raw)| decode(plan_id, &raw))
        .collect::<Result<_, _>>()?;
    Ok(HistoryPage { items, next_cursor })
}

impl PlanDatabase {
    pub(in crate::service_plan) fn task_history_page(
        &self,
        plan_id: &str,
        task_ref: &str,
        kind: TaskHistoryKind,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Value, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        let task_id =
            super::task_page_queries::resolve_detail_task(&transaction, plan_id, task_ref)?;
        let (items, next_cursor) = match kind {
            TaskHistoryKind::Milestones => {
                let page = page(&transaction, plan_id, &task_id, kind, before, limit)?;
                (
                    page.items
                        .into_iter()
                        .map(PlanTaskHistoryItem::Milestone)
                        .collect(),
                    page.next_cursor,
                )
            }
            TaskHistoryKind::Attempts => {
                let page = page(&transaction, plan_id, &task_id, kind, before, limit)?;
                (
                    page.items
                        .into_iter()
                        .map(PlanTaskHistoryItem::Attempt)
                        .collect(),
                    page.next_cursor,
                )
            }
        };
        let value = serde_json::to_value(PlanTaskHistoryPage {
            plan_id: plan_id.into(),
            task_id,
            revision: super::change_feed::revision(&transaction, plan_id)?.to_string(),
            items,
            next_cursor,
        })
        .map_err(super::json_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
