use super::read_projections::column;
use super::{db_error, PlanDatabase};
use crate::plan::{PlanOrphan, PlanStatus};
use crate::service_plan::ui_projections::task_reads::*;
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

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
    historical_executor: Option<PlanTaskHistoricalExecutor>,
    competition_badge: Option<PlanTaskCompetitionBadge>,
    loop_badge: Option<PlanTaskLoopBadge>,
}

impl TaskBoardRow {
    fn into_summary(self) -> Result<PlanTaskBoardSummary, PlanError> {
        Ok(PlanTaskBoardSummary {
            id: self.task_id,
            status: serde_json::from_value(json!(self.status)).map_err(super::json_error)?,
            position: self.position,
            name: self.name,
            created_at: self.created_at,
            updated_at: self.updated_at,
            status_changed_at: self.status_changed_at,
            activity_at: self.activity_at,
            handle: self.handle,
            key: self.key,
            assigned_tab_id: self.assigned_tab_id,
            taken_by_role: self.taken_by_role,
            pause_reason: self.pause_reason,
            model: self.model,
            peer: self.peer,
            review_stage: self.review_stage,
            qa_state: self.qa_state,
            status_label: self.status_label,
            qa_queue_position: self.qa_queue_position,
            latest_note: self.latest_note,
            historical_executor: self.historical_executor,
            competition_badge: self.competition_badge,
            loop_badge: self.loop_badge,
            lane: PlanTaskBoardLane {
                tab_id: self.lane_tab_id,
                branch: self.lane_branch,
            },
            locator: if self.group_key.is_some() || self.section_title.is_some() {
                Some(PlanTaskBoardLocator {
                    group_key: self.group_key,
                    group_label: self.group_label,
                    group_color: self.group_color,
                    section: self.section_title,
                })
            } else {
                None
            },
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
    fn summary(&self) -> PlanTaskBoardCounts {
        let pending = self.pending + self.paused;
        PlanTaskBoardCounts {
            pending,
            in_progress: self.in_progress,
            in_review: self.in_review,
            blocked: self.blocked,
            done: self.done,
            cancelled: self.cancelled,
            total: pending
                + self.in_progress
                + self.in_review
                + self.blocked
                + self.done
                + self.cancelled,
        }
    }
}

fn task_loop_badge(
    loops: Option<&Value>,
    task_id: &str,
    handle: Option<&str>,
) -> Option<PlanTaskLoopBadge> {
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
        .map(|loop_spec| PlanTaskLoopSummary {
            id: bounded_text(loop_spec.get("id").and_then(Value::as_str), 96)
                .unwrap_or_else(|| "loop".to_string()),
            target: bounded_text(loop_spec.get("target").and_then(Value::as_str), 96)
                .unwrap_or_default(),
            budget: loop_spec
                .get("budget")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            count: loop_spec
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            stop_when: bounded_text(loop_spec.get("stop_when").and_then(Value::as_str), 160),
            status: crate::plan::LoopStatus::Running,
            stagnation: loop_spec.get("stagnation").and_then(Value::as_u64),
        })
        .collect::<Vec<_>>();
    Some(PlanTaskLoopBadge {
        count,
        budget,
        status: crate::plan::LoopStatus::Running,
        stop_when,
        warning,
        tooltip,
        loops: visible_loops,
    })
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
    live_attempts: Vec<PlanTaskCompetitionAttemptSummary>,
) -> Option<PlanTaskCompetitionBadge> {
    if !enabled {
        return None;
    }
    Some(PlanTaskCompetitionBadge {
        count,
        live_count,
        judge_hint,
        models,
        live_attempts,
    })
}

impl PlanDatabase {
    pub(in crate::service_plan) fn plan_team_binding_metadata(
        &self,
        plan_id: &str,
    ) -> Result<Option<(PlanStatus, Option<String>, bool)>, PlanError> {
        let connection = self.connection.lock();
        let row: Option<(String, Option<String>, Option<String>, Option<String>)> = connection
            .query_row(
                "SELECT status, team_id, json_type(header_json, '$.orphaned'),
                        json_extract(header_json, '$.orphaned')
                 FROM plans WHERE id = ?1",
                [plan_id],
                |row| {
                    Ok((
                        column(row, 0)?,
                        column(row, 1)?,
                        column(row, 2)?,
                        column(row, 3)?,
                    ))
                },
            )
            .optional()
            .map_err(db_error)?;
        let Some((raw_status, team_id, orphaned_type, orphaned_json)) = row else {
            return Ok(None);
        };
        let status = match raw_status.as_str() {
            "active" => PlanStatus::Active,
            "archived" => PlanStatus::Archived,
            _ => {
                return Err(PlanError::Quarantined {
                    plan_id: plan_id.to_string(),
                    reason: format!("plan has invalid status {raw_status:?}"),
                });
            }
        };
        let has_orphan_marker = match orphaned_type.as_deref() {
            None | Some("null") => false,
            Some("object") => {
                let raw = orphaned_json
                    .as_deref()
                    .ok_or_else(|| PlanError::Quarantined {
                        plan_id: plan_id.to_string(),
                        reason: "plan orphan marker is invalid".into(),
                    })?;
                serde_json::from_str::<PlanOrphan>(raw).map_err(|error| {
                    PlanError::Quarantined {
                        plan_id: plan_id.to_string(),
                        reason: format!("plan orphan marker is invalid: {error}"),
                    }
                })?;
                true
            }
            Some(_) => {
                return Err(PlanError::Quarantined {
                    plan_id: plan_id.to_string(),
                    reason: "plan orphan marker is invalid".into(),
                });
            }
        };
        let orphaned_active =
            status == PlanStatus::Active && team_id.is_none() && has_orphan_marker;
        Ok(Some((status, team_id, orphaned_active)))
    }

    pub(in crate::service_plan) fn task_board_page(
        &self,
        plan_id: &str,
        status: Option<&str>,
        limit: usize,
        after: Option<(u64, i64, &str)>,
    ) -> Result<Value, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        let page_size = limit.clamp(1, PAGE_SIZE_LIMIT);
        let header = load_header(&connection, plan_id)?;
        let revision = super::change_feed::revision(&connection, plan_id)?.to_string();
        let loops = header.get("loops");
        if let Some(status) = status {
            let rows = load_column(&connection, plan_id, status, page_size, after, loops)?;
            let page = serde_json::to_value(PlanTaskBoardPage {
                plan_id: plan_id.into(),
                revision,
                counts: None,
                columns: None,
                column: Some(page_value(status, rows, page_size)?),
            })
            .map_err(super::json_error)?;
            connection.commit().map_err(db_error)?;
            return Ok(page);
        }

        let counts = load_counts(&connection, plan_id)?;
        let mut columns = BTreeMap::new();
        for column in BOARD_COLUMNS {
            let rows = load_column(&connection, plan_id, column, page_size, None, loops)?;
            columns.insert(column.to_string(), page_value(column, rows, page_size)?);
        }
        let page = serde_json::to_value(PlanTaskBoardPage {
            plan_id: plan_id.into(),
            revision,
            counts: Some(counts.summary()),
            columns: Some(columns),
            column: None,
        })
        .map_err(super::json_error)?;
        connection.commit().map_err(db_error)?;
        Ok(page)
    }

    pub(in crate::service_plan) fn task_detail(
        &self,
        plan_id: &str,
        task_ref: &str,
    ) -> Result<Value, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        let header = load_header(&connection, plan_id)?;
        let resolved_id = resolve_detail_task(&connection, plan_id, task_ref)?;
        let task_id = resolved_id.as_str();
        let mut task = load_task_row(&connection, plan_id, task_id)?;
        let milestones = super::task_history_queries::page(
            &connection,
            plan_id,
            task_id,
            crate::service_plan::task_history::TaskHistoryKind::Milestones,
            None,
            super::task_read_queries::TASK_HISTORY_LIMIT,
        )?;
        let attempts = super::task_history_queries::page(
            &connection,
            plan_id,
            task_id,
            crate::service_plan::task_history::TaskHistoryKind::Attempts,
            None,
            super::task_read_queries::TASK_ATTEMPT_LIMIT,
        )?;
        let history = PlanTaskHistoryCursors {
            milestones: milestones.next_cursor,
            attempts: attempts.next_cursor,
        };
        task.milestones = milestones.items;
        task.execution_attempts = attempts.items;
        let sections = load_task_sections(&connection, plan_id, &task.section_ids)?;
        let has_open_subtasks: bool = connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM plan_tasks
                    WHERE plan_id = ?1
                      AND json_extract(row_json, '$.parent_task_id') = ?2
                      AND status NOT IN ('done', 'cancelled')
                )",
                params![plan_id, task_id],
                |row| column(row, 0),
            )
            .map_err(db_error)?;
        let loop_badge = task_loop_badge(header.get("loops"), task_id, task.handle.as_deref());
        let context = PlanTaskDetailContext {
            plan_id: plan_id.into(),
            title: serde_json::from_value(header["title"].clone()).map_err(super::json_error)?,
            status: serde_json::from_value(header["status"].clone()).map_err(super::json_error)?,
            team_id: serde_json::from_value(header["team_id"].clone())
                .map_err(super::json_error)?,
            owner: serde_json::from_value(header["owner"].clone()).map_err(super::json_error)?,
            sections,
            has_open_subtasks,
        };
        let detail = serde_json::to_value(PlanTaskDetail {
            plan_id: plan_id.into(),
            revision: super::change_feed::revision(&connection, plan_id)?.to_string(),
            task_id: task_id.into(),
            task,
            context,
            loop_badge,
            history,
        })
        .map_err(super::json_error)?;
        connection.commit().map_err(db_error)?;
        Ok(detail)
    }
}

pub(super) fn resolve_detail_task(
    connection: &Connection,
    plan_id: &str,
    reference: &str,
) -> Result<String, PlanError> {
    let exact: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND task_id=?2)",
            params![plan_id, reference],
            |row| column(row, 0),
        )
        .map_err(db_error)?;
    if exact {
        return Ok(reference.to_string());
    }
    if reference.is_empty() {
        return Err(PlanError::TaskNotFound);
    }
    let mut statement = connection.prepare_cached(
        "SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND (
            json_extract(row_json,'$.handle') COLLATE NOCASE = ?2 OR
            coalesce(json_extract(row_json,'$.key'), CASE WHEN substr(task_id,1,5)='task_' THEN substr(task_id,6) ELSE task_id END) COLLATE NOCASE = ?2)
         ORDER BY position LIMIT 2"
    ).map_err(db_error)?;
    let rows = statement
        .query_map(params![plan_id, reference], |row| column::<String>(row, 0))
        .map_err(db_error)?;
    let selected = rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
    match selected.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(PlanError::TaskNotFound),
        _ => Err(PlanError::Validation(format!(
            "task ref '{reference}' is ambiguous"
        ))),
    }
}

fn load_header(connection: &Connection, plan_id: &str) -> Result<Value, PlanError> {
    let header: Option<String> = connection
        .query_row(
            "SELECT json_object('title',title,'status',status,'team_id',team_id,
                'owner',json_extract(header_json,'$.owner'),'loops',json((
                    SELECT json_group_array(json_object(
                        'id',json_extract(value,'$.id'),'kind',json_extract(value,'$.kind'),
                        'target',json_extract(value,'$.target'),'status',json_extract(value,'$.status'),
                        'count',json_extract(value,'$.count'),'budget',json_extract(value,'$.budget'),
                        'stagnation',json_extract(value,'$.stagnation'),'stop_when',json_extract(value,'$.stop_when')))
                    FROM json_each(header_json,'$.loops') WHERE json_extract(value,'$.status')='running')))
             FROM plans WHERE id = ?1",
            [plan_id],
            |row| column(row, 0),
        )
        .optional()
        .map_err(db_error)?;
    let Some(header) = header else {
        return Err(PlanError::NotFound);
    };
    super::super::store_sync::parse(&header).map_err(|error| PlanError::Quarantined {
        plan_id: plan_id.to_string(),
        reason: format!("plan header is invalid JSON: {error}"),
    })
}

fn load_task_row(
    connection: &Connection,
    plan_id: &str,
    task_id: &str,
) -> Result<crate::plan::Task, PlanError> {
    let task: Option<String> = connection
        .query_row(
            "SELECT row_json FROM plan_tasks WHERE plan_id = ?1 AND task_id = ?2",
            params![plan_id, task_id],
            |row| column(row, 0),
        )
        .optional()
        .map_err(db_error)?;
    let Some(task) = task else {
        return Err(PlanError::TaskNotFound);
    };
    super::read_projections::decode(plan_id, &task)
}

fn load_task_sections(
    connection: &Connection,
    plan_id: &str,
    section_ids: &[String],
) -> Result<Vec<crate::service_plan::task_queries::TaskSection>, PlanError> {
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
            Ok(crate::service_plan::task_queries::TaskSection {
                id: column(row, 0)?,
                title: column(row, 1)?,
            })
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
            Ok((column::<String>(row, 0)?, column::<i64>(row, 1)? as u64))
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
        (SELECT label FROM board_groups WHERE group_key=(SELECT group_key FROM selected_groups WHERE task_id=plan_tasks.task_id) ORDER BY position LIMIT 1),
        (SELECT color FROM board_groups WHERE group_key=(SELECT group_key FROM selected_groups WHERE task_id=plan_tasks.task_id) ORDER BY position LIMIT 1),
        (SELECT title FROM board_sections
         WHERE section_id=(SELECT first_section FROM selected_groups WHERE task_id=plan_tasks.task_id)
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
             THEN (SELECT queue_position FROM qa_positions WHERE task_id=plan_tasks.task_id)
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
    let cursor = if after.is_some() {
        format!("AND ({created_at_expr},position,task_id)>(?2,?3,?4)")
    } else {
        String::new()
    };
    let limit_parameter = if after.is_some() { "?5" } else { "?2" };
    let lookups = format!("WITH board_groups AS MATERIALIZED (
        SELECT CAST(g.key AS INTEGER) AS position,json_extract(g.value,'$.key') AS group_key,
            substr(json_extract(g.value,'$.label'),1,128) AS label,
            substr(json_extract(g.value,'$.color'),1,32) AS color
        FROM plans p,json_each(p.header_json,'$.task_groups') g WHERE p.id=?1
     ), qa_positions AS MATERIALIZED (
        SELECT task_id,count(*) OVER(ORDER BY position RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS queue_position
        FROM plan_tasks WHERE plan_id=?1 AND status='in_review'
            AND json_extract(row_json,'$.review_stage')='qa' AND json_extract(row_json,'$.qa_state')='queued'
     ), selected_groups AS MATERIALIZED (
        SELECT task_id,json_extract(row_json,'$.group') AS group_key,
            json_extract(row_json,'$.section_ids[0]') AS first_section FROM plan_tasks
        WHERE plan_id=?1 AND {predicate} {cursor}
        ORDER BY {created_at_expr},position,task_id LIMIT {limit_parameter}
     ), board_sections AS MATERIALIZED (
        SELECT section_id,substr(json_extract(row_json,'$.title'),1,160) AS title
        FROM plan_sections WHERE plan_id=?1
            AND section_id IN (SELECT first_section FROM selected_groups)
     ) ");
    let sql = if after.is_some() {
        format!(
            "{lookups}{projection}{predicate}
             AND ({created_at_expr}, position, task_id) > (?2, ?3, ?4)
             ORDER BY {created_at_expr}, position, task_id
             LIMIT ?5"
        )
    } else {
        format!(
            "{lookups}{projection}{predicate}
             ORDER BY {created_at_expr}, position, task_id
             LIMIT ?2"
        )
    };
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let map_row = |row: &rusqlite::Row<'_>| -> Result<TaskBoardRow, PlanError> {
        let created_at = column::<i64>(row, 2).map_err(db_error)?.max(0) as u64;
        let updated_at = column::<i64>(row, 5).map_err(db_error)?.max(0) as u64;
        let status_changed_at = column::<i64>(row, 6).map_err(db_error)?.max(0) as u64;
        let historical_executor = column::<Option<String>>(row, 24)
            .map_err(db_error)?
            .map(|raw| super::read_projections::decode::<PlanTaskHistoricalExecutor>(plan_id, &raw))
            .transpose()?;
        let competition_enabled = column::<i64>(row, 25).map_err(db_error)? != 0;
        let competition_attempts = column::<i64>(row, 26).map_err(db_error)?.max(2) as u64;
        let judge_hint = column::<Option<String>>(row, 27).map_err(db_error)?;
        let models = column::<Option<String>>(row, 28)
            .map_err(db_error)?
            .map(|raw| super::read_projections::decode::<Vec<String>>(plan_id, &raw))
            .transpose()?
            .unwrap_or_default();
        let live_competition_count = column::<i64>(row, 29).map_err(db_error)?.max(0) as u64;
        let live_competition_attempts = column::<Option<String>>(row, 30)
            .map_err(db_error)?
            .map(|raw| {
                super::read_projections::decode::<Vec<PlanTaskCompetitionAttemptSummary>>(
                    plan_id, &raw,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let qa_queue_position = column::<i64>(row, 31).map_err(db_error)?.max(0) as u64;
        let activity_at = column::<i64>(row, 32).map_err(db_error)?.max(0) as u64;
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
            &column::<String>(row, 0).map_err(db_error)?,
            column::<Option<String>>(row, 7)
                .map_err(db_error)?
                .as_deref(),
        );
        Ok(TaskBoardRow {
            task_id: column(row, 0).map_err(db_error)?,
            position: column(row, 1).map_err(db_error)?,
            created_at,
            name: column(row, 3).map_err(db_error)?,
            status: column(row, 4).map_err(db_error)?,
            updated_at,
            status_changed_at,
            activity_at,
            handle: column(row, 7).map_err(db_error)?,
            key: column(row, 8).map_err(db_error)?,
            assigned_tab_id: column(row, 9).map_err(db_error)?,
            lane_tab_id: column(row, 10).map_err(db_error)?,
            lane_branch: column(row, 11).map_err(db_error)?,
            taken_by_role: column(row, 12).map_err(db_error)?,
            pause_reason: column(row, 13).map_err(db_error)?,
            model: column(row, 14).map_err(db_error)?,
            peer: column(row, 15).map_err(db_error)?,
            review_stage: column(row, 16).map_err(db_error)?,
            qa_state: column(row, 17).map_err(db_error)?,
            status_label: column(row, 18).map_err(db_error)?,
            qa_queue_position,
            group_key: column(row, 19).map_err(db_error)?,
            group_label: column(row, 20).map_err(db_error)?,
            group_color: column(row, 21).map_err(db_error)?,
            section_title: column(row, 22).map_err(db_error)?,
            latest_note: column(row, 23).map_err(db_error)?,
            historical_executor,
            competition_badge,
            loop_badge,
        })
    };
    let mut rows = match after {
        Some((created_at, position, task_id)) => statement.query(params![
            plan_id,
            created_at as i64,
            position,
            task_id,
            (limit + 1) as i64
        ]),
        None => statement.query(params![plan_id, (limit + 1) as i64]),
    }
    .map_err(db_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(db_error)? {
        result.push(map_row(row)?);
    }
    Ok(result)
}

fn page_value(
    status: &str,
    mut rows: Vec<TaskBoardRow>,
    page_size: usize,
) -> Result<PlanTaskBoardColumnPage, PlanError> {
    let has_more = rows.len() > page_size;
    rows.truncate(page_size);
    let next_cursor = has_more
        .then(|| {
            rows.last().map(|last| PlanTaskBoardCursor {
                created_at: last.created_at,
                position: last.position,
                task_id: last.task_id.clone(),
            })
        })
        .flatten();
    Ok(PlanTaskBoardColumnPage {
        status: status.into(),
        items: rows
            .into_iter()
            .map(TaskBoardRow::into_summary)
            .collect::<Result<_, _>>()?,
        next_cursor,
    })
}
