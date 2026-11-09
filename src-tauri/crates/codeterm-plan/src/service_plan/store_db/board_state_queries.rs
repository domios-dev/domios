use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::ui_projections::board::{
    PlanBoardControlLoop, PlanBoardMissionControl, PlanBoardState,
};
use crate::service_plan::{board_state::BoardStateRead, state::TaskCounts, PlanError};
use rusqlite::OptionalExtension;
use serde_json::json;

pub(super) const COUNTS_SQL: &str = "SELECT (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1 AND json_extract(row_json,'$.status')='pending'),
    (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1 AND json_extract(row_json,'$.status')='in_progress'),
    (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1 AND json_extract(row_json,'$.status')='in_review'),
    (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1 AND json_extract(row_json,'$.status')='blocked'),
    (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1 AND json_extract(row_json,'$.status')='paused'),
    (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1 AND json_extract(row_json,'$.status')='done'),
    (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1 AND json_extract(row_json,'$.status')='cancelled'),
    (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1)";

pub(super) const BOUNCES_SQL: &str = "SELECT (SELECT count(*) FROM plan_task_attempts WHERE plan_id=?1),
    (SELECT count(*) FROM (SELECT task_id FROM plan_task_attempts WHERE plan_id=?1 GROUP BY task_id HAVING count(*)>=2)),
    (SELECT count(*) FROM plan_tasks t INDEXED BY plan_tasks_status_summary WHERE t.plan_id=?1
        AND json_extract(t.row_json,'$.status')='done'
        AND (SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id)=1),
    (SELECT count(*) FROM plan_tasks INDEXED BY plan_tasks_status_summary WHERE plan_id=?1
        AND json_extract(row_json,'$.status') IN ('pending','in_progress','in_review','blocked','paused','done','cancelled')
        AND json_extract(row_json,'$.status_label')='review bounced')";

impl PlanDatabase {
    pub(in crate::service_plan) fn board_state_projection(
        &self,
        id: &str,
        now_ms: u64,
        event_offset: usize,
        include_events: bool,
    ) -> Result<BoardStateRead, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        record_query();
        let (mode, status, team_id): (String, String, Option<String>) = transaction.query_row(
            "SELECT coalesce(json_extract(header_json,'$.mode'),'planning'),status,team_id FROM plans WHERE id=?1",
            [id], |row| Ok((column(row,0)?,column(row,1)?,column(row,2)?)),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let mode = serde_json::from_value(json!(mode)).map_err(super::json_error)?;
        let status = serde_json::from_value(json!(status)).map_err(super::json_error)?;
        let counts = task_counts(&transaction, id)?;
        let complete = counts.total > 0 && counts.done + counts.cancelled == counts.total;
        record_query();
        let bounces = transaction
            .query_row(BOUNCES_SQL, [id], |row| {
                Ok(crate::service_plan::state::mission_control::BounceSummary {
                    total_attempts: row.get(0)?,
                    tasks_on_second_attempt: row.get(1)?,
                    first_try_done: row.get(2)?,
                    labelled_bounced: row.get(3)?,
                })
            })
            .map_err(db_error)?;
        record_query();
        let raw: String = transaction.query_row("SELECT json_object(
            'mode',coalesce(json_extract(header_json,'$.control_loop.mode'),'curator'),
            'heartbeat_interval_secs',json_extract(header_json,'$.control_loop.heartbeat_interval_secs'),
            'next_at',json_extract(header_json,'$.control_loop.next_at'),
            'last_round',json_extract(header_json,'$.control_loop.last_round'),
            'fallback_reason',json_extract(header_json,'$.control_loop.fallback_reason'),
            'manual_run_requested',json(CASE WHEN json_extract(header_json,'$.control_loop.manual_run_requested') THEN 'true' ELSE 'false' END),
            'running',CASE WHEN json_type(header_json,'$.control_loop.running')='object' THEN json_object('number',json_extract(header_json,'$.control_loop.running.number')) END)
            FROM plans WHERE id=?1", [id], |row| column(row,0)).map_err(db_error)?;
        let mut control_loop: PlanBoardControlLoop = decode(id, &raw)?;
        control_loop.counts_7d = super::status_queries::control_counts(&transaction, id, now_ms)?;
        let events = if include_events {
            super::state_event_queries::event_window(&transaction, id, 50, event_offset)?
        } else {
            let recorded=transaction.query_row("SELECT (SELECT count(*) FROM plan_tasks WHERE plan_id=?1)+(SELECT count(*) FROM plan_task_milestones WHERE plan_id=?1)+(SELECT count(*) FROM plan_strides WHERE plan_id=?1)+coalesce(json_array_length(header_json,'$.planning_log'),0)+coalesce(json_array_length(header_json,'$.gates'),0) FROM plans WHERE id=?1",[id],|row|row.get(0)).map_err(db_error)?;
            crate::service_plan::state::mission_control::EventLog {
                recorded_events: recorded,
                truncated: false,
                entries: Vec::new(),
            }
        };
        let next_event_cursor = events
            .truncated
            .then_some(event_offset.saturating_add(events.entries.len()));
        let value = PlanBoardState {
            plan_id: id.into(),
            revision: super::change_feed::revision(&transaction, id)?.to_string(),
            task_counts: counts,
            control_loop,
            mission_control: PlanBoardMissionControl {
                bounces,
                event_log: events,
                spend: None,
            },
            next_event_cursor,
            event_window_loaded: include_events,
        };
        transaction.commit().map_err(db_error)?;
        Ok(BoardStateRead {
            mode,
            status,
            team_id,
            value,
            complete,
        })
    }
}

pub(super) fn task_counts(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<TaskCounts, PlanError> {
    record_query();
    let counts = connection
        .query_row(COUNTS_SQL, [id], |row| {
            Ok(TaskCounts {
                pending: row.get(0)?,
                in_progress: row.get(1)?,
                in_review: row.get(2)?,
                blocked: row.get(3)?,
                paused: row.get(4)?,
                done: row.get(5)?,
                cancelled: row.get(6)?,
                total: row.get(7)?,
            })
        })
        .map_err(db_error)?;
    Ok(counts)
}

#[cfg(test)]
mod tests;
