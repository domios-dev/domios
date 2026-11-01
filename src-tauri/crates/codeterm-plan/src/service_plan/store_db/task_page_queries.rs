use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};

const BOARD_COLUMNS: [&str; 5] = ["pending", "in_progress", "in_review", "blocked", "done"];
const PAGE_SIZE_LIMIT: usize = 50;

#[derive(Debug)]
struct TaskBoardRow {
    task_id: String,
    position: i64,
    created_at: u64,
    name: String,
    status: String,
    updated_at: u64,
    status_changed_at: u64,
    handle: Option<String>,
    key: Option<String>,
    assigned_tab_id: Option<String>,
    lane_tab_id: Option<String>,
    lane_branch: Option<String>,
    taken_by_role: Option<String>,
    pause_reason: Option<String>,
    model: Option<String>,
    peer: Option<String>,
}

impl TaskBoardRow {
    fn into_json(self) -> Value {
        json!({
            "id": self.task_id,
            "position": self.position,
            "name": self.name,
            "status": self.status,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "status_changed_at": self.status_changed_at,
            "handle": self.handle,
            "key": self.key,
            "assigned_tab_id": self.assigned_tab_id,
            "lane": {
                "tab_id": self.lane_tab_id,
                "branch": self.lane_branch,
            },
            "taken_by_role": self.taken_by_role,
            "pause_reason": self.pause_reason,
            "model": self.model,
            "peer": self.peer,
        })
    }
}

#[derive(Default)]
struct TaskStatusCounts {
    pending: u64,
    paused: u64,
    in_progress: u64,
    in_review: u64,
    blocked: u64,
    done: u64,
    cancelled: u64,
}

impl TaskStatusCounts {
    fn as_json(&self) -> Value {
        let pending = self.pending + self.paused;
        json!({
            "pending": pending,
            "in_progress": self.in_progress,
            "in_review": self.in_review,
            "blocked": self.blocked,
            "done": self.done,
            "cancelled": self.cancelled,
            "total": pending + self.in_progress + self.in_review + self.blocked + self.done + self.cancelled,
        })
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn task_board_page(
        &self,
        plan_id: &str,
        status: Option<&str>,
        limit: usize,
        after: Option<(u64, i64, &str)>,
    ) -> Result<Value, PlanError> {
        let connection = self.connection.lock();
        let page_size = limit.clamp(1, PAGE_SIZE_LIMIT);
        if let Some(status) = status {
            let rows = load_column(&connection, plan_id, status, page_size, after)?;
            return Ok(json!({
                "plan_id": plan_id,
                "column": page_value(status, rows, page_size),
            }));
        }

        let counts = load_counts(&connection, plan_id)?;
        let mut columns = Map::new();
        for column in BOARD_COLUMNS {
            let rows = load_column(&connection, plan_id, column, page_size, None)?;
            columns.insert(column.to_string(), page_value(column, rows, page_size));
        }
        Ok(json!({
            "plan_id": plan_id,
            "counts": counts.as_json(),
            "columns": columns,
        }))
    }
}

fn load_counts(connection: &Connection, plan_id: &str) -> Result<TaskStatusCounts, PlanError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT status, COUNT(*) FROM plan_tasks WHERE plan_id = ?1 GROUP BY status",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([plan_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })
        .map_err(db_error)?;
    let mut counts = TaskStatusCounts::default();
    for row in rows {
        let (status, count) = row.map_err(db_error)?;
        match status.as_str() {
            "pending" => counts.pending = count,
            "paused" => counts.paused = count,
            "in_progress" => counts.in_progress = count,
            "in_review" => counts.in_review = count,
            "blocked" => counts.blocked = count,
            "done" => counts.done = count,
            "cancelled" => counts.cancelled = count,
            _ => {}
        }
    }
    Ok(counts)
}

fn status_predicate(status: &str) -> Option<&'static str> {
    match status {
        "pending" => Some("status IN ('pending', 'paused')"),
        "in_progress" => Some("status = 'in_progress'"),
        "in_review" => Some("status = 'in_review'"),
        "blocked" => Some("status = 'blocked'"),
        "done" => Some("status = 'done'"),
        "cancelled" => Some("status = 'cancelled'"),
        _ => None,
    }
}

fn load_column(
    connection: &Connection,
    plan_id: &str,
    status: &str,
    limit: usize,
    after: Option<(u64, i64, &str)>,
) -> Result<Vec<TaskBoardRow>, PlanError> {
    let Some(predicate) = status_predicate(status) else {
        return Ok(Vec::new());
    };
    let projection = "SELECT task_id, position,
        coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0),
        substr(coalesce(json_extract(row_json, '$.name'), ''), 1, 512),
        status,
        coalesce(CAST(json_extract(row_json, '$.updated_at') AS INTEGER), 0),
        coalesce(CAST(json_extract(row_json, '$.status_changed_at') AS INTEGER), 0),
        substr(json_extract(row_json, '$.handle'), 1, 96),
        substr(json_extract(row_json, '$.key'), 1, 96),
        substr(json_extract(row_json, '$.assigned_tab_id'), 1, 96),
        substr(json_extract(row_json, '$.lane.tab_id'), 1, 96),
        substr(json_extract(row_json, '$.lane.branch'), 1, 160),
        substr(json_extract(row_json, '$.taken_by_role'), 1, 96),
        substr(json_extract(row_json, '$.pause_reason'), 1, 240),
        substr(json_extract(row_json, '$.model'), 1, 120),
        substr(json_extract(row_json, '$.peer'), 1, 120)
        FROM plan_tasks WHERE plan_id = ?1 AND ";
    let sql = if after.is_some() {
        format!(
            "{projection}{predicate}
             AND (coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0), position, task_id)
                 > (?2, ?3, ?4)
             ORDER BY coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0), position, task_id
             LIMIT ?5"
        )
    } else {
        format!(
            "{projection}{predicate}
             ORDER BY coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0), position, task_id
             LIMIT ?2"
        )
    };
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let map_row = |row: &rusqlite::Row<'_>| {
        Ok(TaskBoardRow {
            task_id: row.get(0)?,
            position: row.get(1)?,
            created_at: row.get::<_, i64>(2)?.max(0) as u64,
            name: row.get(3)?,
            status: row.get(4)?,
            updated_at: row.get::<_, i64>(5)?.max(0) as u64,
            status_changed_at: row.get::<_, i64>(6)?.max(0) as u64,
            handle: row.get(7)?,
            key: row.get(8)?,
            assigned_tab_id: row.get(9)?,
            lane_tab_id: row.get(10)?,
            lane_branch: row.get(11)?,
            taken_by_role: row.get(12)?,
            pause_reason: row.get(13)?,
            model: row.get(14)?,
            peer: row.get(15)?,
        })
    };
    let rows = match after {
        Some((created_at, position, task_id)) => statement
            .query_map(
                params![
                    plan_id,
                    created_at as i64,
                    position,
                    task_id,
                    (limit + 1) as i64
                ],
                map_row,
            )
            .map_err(db_error)?,
        None => statement
            .query_map(params![plan_id, (limit + 1) as i64], map_row)
            .map_err(db_error)?,
    };
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

fn page_value(status: &str, mut rows: Vec<TaskBoardRow>, page_size: usize) -> Value {
    let has_more = rows.len() > page_size;
    rows.truncate(page_size);
    let next_cursor = has_more
        .then(|| {
            rows.last().map(|last| {
                json!({
                    "created_at": last.created_at,
                    "position": last.position,
                    "task_id": last.task_id,
                })
            })
        })
        .flatten();
    json!({
        "status": status,
        "items": rows.into_iter().map(TaskBoardRow::into_json).collect::<Vec<_>>(),
        "next_cursor": next_cursor,
    })
}
