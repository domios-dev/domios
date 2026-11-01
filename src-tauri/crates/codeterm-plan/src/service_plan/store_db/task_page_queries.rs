use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

const BOARD_COLUMNS: [&str; 5] = ["pending", "in_progress", "in_review", "blocked", "done"];
const PAGE_SIZE_LIMIT: usize = 50;
const LOOP_DETAILS_LIMIT: usize = 8;

#[derive(Debug)]
struct TaskBoardRow {
    task_id: String,
    position: i64,
    created_at: u64,
    name: String,
    status: String,
    updated_at: u64,
    status_changed_at: u64,
    activity_at: u64,
    handle: Option<String>,
    key: Option<String>,
    assigned_tab_id: Option<String>,
    lane_tab_id: Option<String>,
    lane_branch: Option<String>,
    taken_by_role: Option<String>,
    pause_reason: Option<String>,
    model: Option<String>,
    peer: Option<String>,
    review_stage: Option<String>,
    qa_state: Option<String>,
    status_label: Option<String>,
    qa_queue_position: u64,
    group_key: Option<String>,
    group_label: Option<String>,
    group_color: Option<String>,
    section_title: Option<String>,
    latest_note: Option<String>,
    historical_executor: Option<Value>,
    competition_badge: Option<Value>,
    loop_badge: Option<Value>,
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
            "activity_at": self.activity_at,
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
            "review_stage": self.review_stage,
            "qa_state": self.qa_state,
            "status_label": self.status_label,
            "qa_queue_position": self.qa_queue_position,
            "locator": if self.group_key.is_some() || self.section_title.is_some() {
                json!({
                    "groupKey": self.group_key,
                    "groupLabel": self.group_label,
                    "groupColor": self.group_color,
                    "section": self.section_title,
                })
            } else {
                Value::Null
            },
            "latest_note": self.latest_note,
            "historical_executor": self.historical_executor,
            "competition_badge": self.competition_badge,
            "loop_badge": self.loop_badge,
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

fn task_loop_badge(loops: Option<&Value>, task_id: &str, handle: Option<&str>) -> Option<Value> {
    let matches: Vec<&Value> = loops?
        .as_array()?
        .iter()
        .filter(|loop_spec| {
            loop_spec.get("status").and_then(Value::as_str) == Some("running")
                && loop_spec
                    .get("target")
                    .and_then(Value::as_str)
                    .is_some_and(|target| {
                        let target = target.trim();
                        target == task_id.trim()
                            || handle.is_some_and(|handle| target == handle.trim())
                    })
        })
        .collect();
    if matches.is_empty() {
        return None;
    }
    let count = matches.iter().fold(0_u64, |sum, loop_spec| {
        sum.saturating_add(
            loop_spec
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
        )
    });
    if count < 2 {
        return None;
    }

    let budget = matches
        .iter()
        .filter_map(|loop_spec| loop_spec.get("budget").and_then(Value::as_u64))
        .max()
        .unwrap_or_default();
    let warning = matches.iter().any(|loop_spec| {
        loop_spec
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            >= loop_spec
                .get("budget")
                .and_then(Value::as_u64)
                .unwrap_or_default()
            || loop_spec
                .get("stagnation")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                >= 2
    });
    let stop_when = if matches.len() == 1 {
        bounded_text(matches[0].get("stop_when").and_then(Value::as_str), 160)
    } else {
        None
    };
    let tooltip_lines = matches
        .iter()
        .take(LOOP_DETAILS_LIMIT)
        .map(|loop_spec| {
            let id = bounded_text(loop_spec.get("id").and_then(Value::as_str), 96)
                .unwrap_or_else(|| "loop".to_string());
            let loop_count = loop_spec
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let loop_budget = loop_spec
                .get("budget")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let stop = bounded_text(loop_spec.get("stop_when").and_then(Value::as_str), 160)
                .unwrap_or_else(|| "(none recorded)".to_string());
            let stagnating = loop_spec
                .get("stagnation")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                >= 2;
            format!(
                "{id}: iteration {loop_count}/{loop_budget} · running · stop_when: {stop}{}",
                if stagnating { " · stagnating" } else { "" }
            )
        })
        .collect::<Vec<_>>();
    let omitted = matches.len().saturating_sub(LOOP_DETAILS_LIMIT);
    let tooltip = if omitted == 0 {
        tooltip_lines.join("\n")
    } else {
        format!("{}\n{omitted} additional loops", tooltip_lines.join("\n"))
    };
    let visible_loops = matches
        .iter()
        .take(LOOP_DETAILS_LIMIT)
        .map(|loop_spec| {
            json!({
                "id": bounded_text(loop_spec.get("id").and_then(Value::as_str), 96).unwrap_or_else(|| "loop".to_string()),
                "target": bounded_text(loop_spec.get("target").and_then(Value::as_str), 96).unwrap_or_default(),
                "budget": loop_spec.get("budget").and_then(Value::as_u64).unwrap_or_default(),
                "count": loop_spec.get("count").and_then(Value::as_u64).unwrap_or_default(),
                "stop_when": bounded_text(loop_spec.get("stop_when").and_then(Value::as_str), 160),
                "status": "running",
                "stagnation": loop_spec.get("stagnation").and_then(Value::as_u64),
            })
        })
        .collect::<Vec<_>>();
    Some(json!({
        "count": count,
        "budget": budget,
        "status": "running",
        "stopWhen": stop_when,
        "warning": warning,
        "tooltip": tooltip,
        "loops": visible_loops,
    }))
}

fn bounded_text(value: Option<&str>, max_chars: usize) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(max_chars).collect())
}

fn competition_badge(
    enabled: bool,
    count: u64,
    judge_hint: Option<String>,
    models: Vec<String>,
    live_count: u64,
    live_attempts: Vec<Value>,
) -> Option<Value> {
    if !enabled {
        return None;
    }
    Some(json!({
        "count": count,
        "live_count": live_count,
        "judge_hint": judge_hint,
        "models": models,
        "live_attempts": live_attempts,
    }))
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
        let header = load_header(&connection, plan_id)?;
        let loops = header.get("loops");
        if let Some(status) = status {
            let rows = load_column(&connection, plan_id, status, page_size, after, loops)?;
            return Ok(json!({
                "plan_id": plan_id,
                "column": page_value(status, rows, page_size),
            }));
        }

        let counts = load_counts(&connection, plan_id)?;
        let mut columns = Map::new();
        for column in BOARD_COLUMNS {
            let rows = load_column(&connection, plan_id, column, page_size, None, loops)?;
            columns.insert(column.to_string(), page_value(column, rows, page_size));
        }
        Ok(json!({
            "plan_id": plan_id,
            "counts": counts.as_json(),
            "columns": columns,
        }))
    }

    pub(in crate::service_plan) fn task_detail(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Value, PlanError> {
        let connection = self.connection.lock();
        let header = load_header(&connection, plan_id)?;
        let mut task = load_task_row(&connection, plan_id, task_id)?;
        let milestones = load_json_rows(
            &connection,
            "SELECT row_json FROM plan_task_milestones WHERE plan_id = ?1 AND task_id = ?2 ORDER BY seq",
            plan_id,
            task_id,
        )?;
        let attempts = load_json_rows(
            &connection,
            "SELECT row_json FROM plan_task_attempts WHERE plan_id = ?1 AND task_id = ?2 ORDER BY position",
            plan_id,
            task_id,
        )?;
        let fields = task
            .as_object_mut()
            .ok_or_else(|| quarantined_task(plan_id, "task row is not an object"))?;
        fields.insert("milestones".into(), Value::Array(milestones));
        fields.insert("execution_attempts".into(), Value::Array(attempts));

        let section_ids = fields
            .get("section_ids")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let sections = load_task_sections(&connection, plan_id, &section_ids)?;
        let has_open_subtasks: bool = connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM plan_tasks
                    WHERE plan_id = ?1
                      AND json_extract(row_json, '$.parent_task_id') = ?2
                      AND status NOT IN ('done', 'cancelled')
                )",
                params![plan_id, task_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        let loop_badge = task_loop_badge(
            header.get("loops"),
            task_id,
            fields.get("handle").and_then(Value::as_str),
        );
        let context = json!({
            "plan_id": plan_id,
            "title": header.get("title").cloned().unwrap_or(Value::Null),
            "status": header.get("status").cloned().unwrap_or(Value::Null),
            "team_id": header.get("team_id").cloned().unwrap_or(Value::Null),
            "owner": header.get("owner").cloned().unwrap_or(Value::Null),
            "sections": sections,
            "has_open_subtasks": has_open_subtasks,
        });
        Ok(json!({
            "plan_id": plan_id,
            "task_id": task_id,
            "task": task,
            "context": context,
            "loop_badge": loop_badge,
        }))
    }
}

fn load_header(connection: &Connection, plan_id: &str) -> Result<Value, PlanError> {
    let header: Option<String> = connection
        .query_row(
            "SELECT header_json FROM plans WHERE id = ?1",
            [plan_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    let Some(header) = header else {
        return Err(PlanError::NotFound);
    };
    serde_json::from_str(&header).map_err(|error| PlanError::Quarantined {
        plan_id: plan_id.to_string(),
        reason: format!("plan header is invalid JSON: {error}"),
    })
}

fn load_task_row(
    connection: &Connection,
    plan_id: &str,
    task_id: &str,
) -> Result<Value, PlanError> {
    let task: Option<String> = connection
        .query_row(
            "SELECT row_json FROM plan_tasks WHERE plan_id = ?1 AND task_id = ?2",
            params![plan_id, task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    let Some(task) = task else {
        return Err(PlanError::TaskNotFound);
    };
    serde_json::from_str(&task)
        .map_err(|error| quarantined_task(plan_id, &format!("task row is invalid JSON: {error}")))
}

fn load_json_rows(
    connection: &Connection,
    sql: &str,
    plan_id: &str,
    task_id: &str,
) -> Result<Vec<Value>, PlanError> {
    let mut statement = connection.prepare_cached(sql).map_err(db_error)?;
    let rows = statement
        .query_map(params![plan_id, task_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    rows.map(|row| {
        let document = row.map_err(db_error)?;
        serde_json::from_str(&document).map_err(|error| {
            quarantined_task(
                plan_id,
                &format!("task history row is invalid JSON: {error}"),
            )
        })
    })
    .collect()
}

fn load_task_sections(
    connection: &Connection,
    plan_id: &str,
    section_ids: &Value,
) -> Result<Vec<Value>, PlanError> {
    let section_ids = serde_json::to_string(section_ids).map_err(|error| {
        quarantined_task(plan_id, &format!("task section ids are invalid: {error}"))
    })?;
    let mut statement = connection
        .prepare_cached(
            "SELECT s.section_id, substr(coalesce(json_extract(s.row_json, '$.title'), ''), 1, 240)
             FROM json_each(?2) AS requested
             JOIN plan_sections AS s ON s.plan_id = ?1 AND s.section_id = requested.value
             ORDER BY CAST(requested.key AS INTEGER)",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![plan_id, section_ids], |row| {
            Ok(json!({ "id": row.get::<_, String>(0)?, "title": row.get::<_, String>(1)? }))
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

fn quarantined_task(plan_id: &str, reason: &str) -> PlanError {
    PlanError::Quarantined {
        plan_id: plan_id.to_string(),
        reason: reason.to_string(),
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
        "pending" => Some("CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END = 'pending'"),
        "in_progress" => Some("CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END = 'in_progress'"),
        "in_review" => Some("CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END = 'in_review'"),
        "blocked" => Some("CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END = 'blocked'"),
        "done" => Some("CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END = 'done'"),
        "cancelled" => Some("CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END = 'cancelled'"),
        _ => None,
    }
}

fn load_column(
    connection: &Connection,
    plan_id: &str,
    status: &str,
    limit: usize,
    after: Option<(u64, i64, &str)>,
    loops: Option<&Value>,
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
        substr(json_extract(row_json, '$.peer'), 1, 120),
        substr(json_extract(row_json, '$.review_stage'), 1, 32),
        substr(json_extract(row_json, '$.qa_state'), 1, 32),
        substr(json_extract(row_json, '$.status_label'), 1, 240),
        substr(json_extract(row_json, '$.group'), 1, 96),
        (SELECT substr(json_extract(task_group.value, '$.label'), 1, 128)
         FROM plans AS board_plan, json_each(board_plan.header_json, '$.task_groups') AS task_group
         WHERE board_plan.id = plan_tasks.plan_id
           AND json_extract(task_group.value, '$.key') = json_extract(plan_tasks.row_json, '$.group')
         LIMIT 1),
        (SELECT substr(json_extract(task_group.value, '$.color'), 1, 32)
         FROM plans AS board_plan, json_each(board_plan.header_json, '$.task_groups') AS task_group
         WHERE board_plan.id = plan_tasks.plan_id
           AND json_extract(task_group.value, '$.key') = json_extract(plan_tasks.row_json, '$.group')
         LIMIT 1),
        (SELECT substr(json_extract(section.row_json, '$.title'), 1, 160)
         FROM plan_sections AS section
         WHERE section.plan_id = plan_tasks.plan_id
           AND section.section_id = json_extract(plan_tasks.row_json, '$.section_ids[0]')
         LIMIT 1),
        (SELECT substr(json_extract(milestone.row_json, '$.text'), 1, 512)
         FROM plan_task_milestones AS milestone
         WHERE milestone.plan_id = plan_tasks.plan_id AND milestone.task_id = plan_tasks.task_id
         ORDER BY milestone.seq DESC LIMIT 1),
        (SELECT json_object(
            'tab_id', substr(coalesce(nullif(json_extract(attempt.row_json, '$.tab_id'), ''), nullif(json_extract(attempt.row_json, '$.executor_tab_id'), '')), 1, 96),
            'tab_title', coalesce(nullif(substr(json_extract(attempt.row_json, '$.tab_title'), 1, 96), ''), nullif(substr(json_extract(attempt.row_json, '$.executor_title'), 1, 96), '')),
            'role', substr(json_extract(attempt.row_json, '$.role'), 1, 96),
            'model', substr(json_extract(attempt.row_json, '$.model'), 1, 120),
            'branch', substr(json_extract(attempt.row_json, '$.branch'), 1, 160),
            'report_status', coalesce(substr(json_extract(attempt.row_json, '$.report_status'), 1, 96), substr(json_extract(attempt.row_json, '$.report.status'), 1, 96)),
            'outcome', substr(json_extract(attempt.row_json, '$.outcome'), 1, 96)
         )
         FROM plan_task_attempts AS attempt
         WHERE attempt.plan_id = plan_tasks.plan_id AND attempt.task_id = plan_tasks.task_id
           AND (coalesce(nullif(json_extract(attempt.row_json, '$.tab_id'), ''), nullif(json_extract(attempt.row_json, '$.executor_tab_id'), ''), nullif(json_extract(attempt.row_json, '$.tab_title'), ''), nullif(json_extract(attempt.row_json, '$.executor_title'), '')) IS NOT NULL)
         ORDER BY attempt.position DESC LIMIT 1),
        coalesce(CAST(json_extract(row_json, '$.competition.enabled') AS INTEGER), 0),
        max(2, coalesce(CAST(json_extract(row_json, '$.competition.attempts') AS INTEGER), 2)),
        substr(json_extract(row_json, '$.competition.judge_hint'), 1, 160),
        (SELECT json_group_array(model) FROM (
            SELECT substr(value, 1, 120) AS model
            FROM json_each(plan_tasks.row_json, '$.competition.models')
            LIMIT 8
        )),
        (SELECT COUNT(*) FROM plan_task_attempts AS live_attempt
         WHERE live_attempt.plan_id = plan_tasks.plan_id AND live_attempt.task_id = plan_tasks.task_id
           AND nullif(json_extract(live_attempt.row_json, '$.competition_group_id'), '') IS NOT NULL
           AND json_extract(live_attempt.row_json, '$.closed_at') IS NULL),
        (SELECT json_group_array(json_object(
            'tab_id', attempt.tab_id,
            'tab_title', attempt.tab_title,
            'model', attempt.model,
            'competition_lane', attempt.competition_lane
         )) FROM (
            SELECT
              substr(coalesce(nullif(json_extract(live_attempt.row_json, '$.tab_id'), ''), nullif(json_extract(live_attempt.row_json, '$.executor_tab_id'), '')), 1, 96) AS tab_id,
              coalesce(nullif(substr(json_extract(live_attempt.row_json, '$.tab_title'), 1, 96), ''), nullif(substr(json_extract(live_attempt.row_json, '$.executor_title'), 1, 96), '')) AS tab_title,
              substr(json_extract(live_attempt.row_json, '$.model'), 1, 120) AS model,
              CAST(json_extract(live_attempt.row_json, '$.competition_lane') AS INTEGER) AS competition_lane
            FROM plan_task_attempts AS live_attempt
            WHERE live_attempt.plan_id = plan_tasks.plan_id AND live_attempt.task_id = plan_tasks.task_id
              AND nullif(json_extract(live_attempt.row_json, '$.competition_group_id'), '') IS NOT NULL
              AND json_extract(live_attempt.row_json, '$.closed_at') IS NULL
            ORDER BY live_attempt.position DESC
            LIMIT 6
         ) AS attempt),
        CASE WHEN status = 'in_review'
                  AND json_extract(row_json, '$.review_stage') = 'qa'
                  AND json_extract(row_json, '$.qa_state') = 'queued'
             THEN (SELECT COUNT(*) FROM plan_tasks AS queued
                   WHERE queued.plan_id = plan_tasks.plan_id
                     AND CASE WHEN queued.status IN ('pending', 'paused') THEN 'pending' ELSE queued.status END = 'in_review'
                     AND json_extract(queued.row_json, '$.review_stage') = 'qa'
                     AND json_extract(queued.row_json, '$.qa_state') = 'queued'
                     AND queued.position <= plan_tasks.position)
             ELSE 0 END,
        max(
            coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0),
            coalesce(CAST(json_extract(row_json, '$.updated_at') AS INTEGER), 0),
            coalesce(CAST(json_extract(row_json, '$.status_changed_at') AS INTEGER), 0),
            coalesce((SELECT MAX(CAST(json_extract(milestone.row_json, '$.ts') AS INTEGER))
                      FROM plan_task_milestones AS milestone
                      WHERE milestone.plan_id = plan_tasks.plan_id
                        AND milestone.task_id = plan_tasks.task_id), 0)
        )
        FROM plan_tasks WHERE plan_id = ?1 AND ";
    let created_at_expr = "coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0)";
    let sql = if after.is_some() {
        format!(
            "{projection}{predicate}
             AND ({created_at_expr}, position, task_id) > (?2, ?3, ?4)
             ORDER BY {created_at_expr}, position, task_id
             LIMIT ?5"
        )
    } else {
        format!(
            "{projection}{predicate}
             ORDER BY {created_at_expr}, position, task_id
             LIMIT ?2"
        )
    };
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let map_row = |row: &rusqlite::Row<'_>| {
        let created_at = row.get::<_, i64>(2)?.max(0) as u64;
        let updated_at = row.get::<_, i64>(5)?.max(0) as u64;
        let status_changed_at = row.get::<_, i64>(6)?.max(0) as u64;
        let historical_executor = row
            .get::<_, Option<String>>(24)?
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
        let competition_enabled = row.get::<_, i64>(25)? != 0;
        let competition_attempts = row.get::<_, i64>(26)?.max(2) as u64;
        let judge_hint = row.get::<_, Option<String>>(27)?;
        let models = row
            .get::<_, Option<String>>(28)?
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();
        let live_competition_count = row.get::<_, i64>(29)?.max(0) as u64;
        let live_competition_attempts = row
            .get::<_, Option<String>>(30)?
            .and_then(|raw| serde_json::from_str::<Vec<Value>>(&raw).ok())
            .unwrap_or_default();
        let qa_queue_position = row.get::<_, i64>(31)?.max(0) as u64;
        let activity_at = row.get::<_, i64>(32)?.max(0) as u64;
        let competition_badge = competition_badge(
            competition_enabled,
            competition_attempts,
            judge_hint,
            models,
            live_competition_count,
            live_competition_attempts,
        );
        let loop_badge = task_loop_badge(
            loops,
            &row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(7)?.as_deref(),
        );
        Ok(TaskBoardRow {
            task_id: row.get(0)?,
            position: row.get(1)?,
            created_at,
            name: row.get(3)?,
            status: row.get(4)?,
            updated_at,
            status_changed_at,
            activity_at,
            handle: row.get(7)?,
            key: row.get(8)?,
            assigned_tab_id: row.get(9)?,
            lane_tab_id: row.get(10)?,
            lane_branch: row.get(11)?,
            taken_by_role: row.get(12)?,
            pause_reason: row.get(13)?,
            model: row.get(14)?,
            peer: row.get(15)?,
            review_stage: row.get(16)?,
            qa_state: row.get(17)?,
            status_label: row.get(18)?,
            qa_queue_position,
            group_key: row.get(19)?,
            group_label: row.get(20)?,
            group_color: row.get(21)?,
            section_title: row.get(22)?,
            latest_note: row.get(23)?,
            historical_executor,
            competition_badge,
            loop_badge,
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
