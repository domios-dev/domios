use super::read_projections::{column, decode, record_query};
use super::ui_projections::{ListCursorView, ListWindowView, TaskView};
use super::{db_error, PlanDatabase};
use crate::service_plan::{
    ui_projections::{ListWindowQuery, PlanViewProjection},
    PlanError,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    connection
        .create_scalar_function(
            "domios_casefold",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(context.get::<String>(0)?.to_lowercase()),
        )
        .map_err(db_error)
}

pub(super) fn window(
    connection: &Connection,
    id: &str,
    query: &ListWindowQuery,
) -> Result<(Vec<TaskView>, ListWindowView), PlanError> {
    if query.after_position.is_some() != query.after_task_id.is_some() {
        return Err(PlanError::Validation(
            "List cursor requires position and task id".into(),
        ));
    }
    let status = match query.status.as_deref().unwrap_or("all") {
        "all" => "1",
        "not_cancelled" => "t.status <> 'cancelled'",
        "todo" => "t.status IN ('pending','paused')",
        "in_progress" => "t.status='in_progress'",
        "in_review" => "t.status='in_review'",
        "active" => "t.status NOT IN ('done','cancelled','paused')",
        "done" => "t.status='done'",
        "blocked" => "t.status='blocked'",
        "cancelled" => "t.status='cancelled'",
        _ => return Err(PlanError::Validation("Invalid list status filter".into())),
    };
    if let Some(ids) = &query.task_ids {
        let ids: Vec<String> = serde_json::from_str(ids)
            .map_err(|_| PlanError::Validation("Task ids must be an array".into()))?;
        if ids.len() > 25 {
            return Err(PlanError::Validation(
                "At most 25 task ids per window".into(),
            ));
        }
    }
    let target = query
        .task_ref
        .as_deref()
        .map(|reference| super::task_page_queries::resolve_detail_task(connection, id, reference))
        .transpose()?;
    let section_labels = "WITH section_labels AS MATERIALIZED (
        SELECT section_id,json_extract(row_json,'$.title') AS title FROM plan_sections WHERE plan_id=?1 ORDER BY section_id
    ), task_memberships AS MATERIALIZED (
        SELECT t.task_id,ids.value AS section_id FROM plan_tasks t,json_each(t.row_json,'$.section_ids') ids
        WHERE t.plan_id=?1 AND ?2<>''
    )";
    let filter = format!("t.plan_id=?1 AND {status} AND (?3 IS NULL OR t.task_id=?3) AND (?4 IS NULL OR t.task_id IN (SELECT value FROM json_each(?4))) AND (?2='' OR instr(domios_casefold(t.task_id || ' ' || coalesce(json_extract(t.row_json,'$.handle'),'') || ' ' || coalesce(json_extract(t.row_json,'$.name'),'') || ' ' || coalesce(json_extract(t.row_json,'$.key'),'') || ' ' || coalesce(json_extract(t.row_json,'$.status_label'),'') || ' ' || coalesce(json_extract(t.row_json,'$.pause_reason'),'') || ' ' || coalesce(t.assigned_tab_id,'') || ' ' || coalesce((SELECT group_concat(s.title,' ') FROM section_labels s CROSS JOIN task_memberships ids WHERE ids.task_id=t.task_id AND s.section_id=ids.section_id),'')),?2)>0)");
    let search = query.search.as_deref().unwrap_or("").trim().to_lowercase();
    let limit = query.page_size.unwrap_or(25).clamp(1, 50);
    record_query();
    let sql = format!("{section_labels} SELECT t.position,t.task_id,{} FROM plan_tasks t WHERE {filter} AND (?5 IS NULL OR (t.position,t.task_id)>(?5,?6)) ORDER BY t.position,t.task_id LIMIT ?7", super::ui_projections::TASK_SUMMARY);
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let rows = statement
        .query_map(
            params![
                id,
                search,
                target,
                query.task_ids,
                query.after_position,
                query.after_task_id,
                (limit + 1) as i64
            ],
            |row| {
                Ok((
                    column::<i64>(row, 0)?,
                    column::<String>(row, 1)?,
                    column::<String>(row, 2)?,
                ))
            },
        )
        .map_err(db_error)?;
    let mut rows = rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let cursor = if more {
        rows.last().map(|row| ListCursorView {
            position: row.0,
            task_id: row.1.clone(),
        })
    } else {
        None
    };
    let tasks = rows
        .into_iter()
        .map(|(_, _, raw)| decode::<TaskView>(id, &raw))
        .collect::<Result<Vec<_>, _>>()?;
    record_query();
    let total: usize = connection
        .query_row(
            &format!("{section_labels} SELECT count(*) FROM plan_tasks t WHERE {filter}"),
            params![id, search, target, query.task_ids],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let cancelled: usize = connection
        .query_row(
            "SELECT count(*) FROM plan_tasks WHERE plan_id=?1 AND status='cancelled'",
            [id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    Ok((
        tasks,
        ListWindowView {
            next_cursor: cursor,
            total,
            cancelled,
        },
    ))
}

pub(super) fn apply_task_counts(
    connection: &Connection,
    id: &str,
    value: &mut Value,
) -> Result<(), PlanError> {
    if let Some(tasks) = value["tasks"].as_array_mut() {
        for task in tasks {
            let task_id = task["id"].as_str().unwrap_or_default();
            record_query();
            let (criteria,milestones): (usize,usize) = connection.query_row(
                "SELECT coalesce(json_array_length(row_json,'$.acceptance_criteria'),0),(SELECT count(*) FROM plan_task_milestones m WHERE m.plan_id=t.plan_id AND m.task_id=t.task_id) FROM plan_tasks t WHERE plan_id=?1 AND task_id=?2",
                params![id,task_id],|row| Ok((row.get(0)?,row.get(1)?))).map_err(db_error)?;
            task["criteria_count"] = json!(criteria);
            task["milestone_count"] = json!(milestones);
        }
    }
    Ok(())
}

impl PlanDatabase {
    pub(in crate::service_plan) fn ui_plan_metadata_window(
        &self,
        id: &str,
        query: &ListWindowQuery,
    ) -> Result<Value, PlanError> {
        self.read_window(id, query, PlanViewProjection::TaskMetadata)
    }
    pub(in crate::service_plan) fn ui_plan_list_window(
        &self,
        id: &str,
        query: &ListWindowQuery,
    ) -> Result<Value, PlanError> {
        self.read_window(id, query, PlanViewProjection::List)
    }
    fn read_window(
        &self,
        id: &str,
        query: &ListWindowQuery,
        view: PlanViewProjection,
    ) -> Result<Value, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        let value =
            super::ui_projections::projection_with_list(&transaction, id, view, Some(query))?;
        transaction.commit().map_err(db_error)?;
        serde_json::to_value(value).map_err(super::json_error)
    }
}

#[cfg(test)]
mod tests;
