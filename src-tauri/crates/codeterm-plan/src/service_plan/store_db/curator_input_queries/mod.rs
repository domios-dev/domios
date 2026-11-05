use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::TaskStatus;
use crate::service_plan::state::TaskCounts;
use crate::service_plan::{CuratorDigestRead, CuratorDigestScope, PlanError};
use rusqlite::{Connection, OpenFlags, OptionalExtension};

mod functions;
mod rows;
mod synthesis;

pub(super) fn open(
    database: &PlanDatabase,
    id: &str,
    now: u64,
) -> Result<CuratorDigestRead, PlanError> {
    let connection = Connection::open_with_flags(
        database.path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(db_error)?;
    super::curator_snapshot::register_functions(&connection)?;
    super::status_queries::register_functions(&connection)?;
    super::curator_notice_queries::register_functions(&connection)?;
    super::curator_queries::register_timestamp(&connection)?;
    functions::register(&connection)?;
    connection
        .execute_batch("BEGIN DEFERRED TRANSACTION")
        .map_err(db_error)?;
    record_query();
    let raw:Option<String>=connection.query_row(
        "SELECT json_object('id',id,'title',domios_curator_bounded(title,256),'status',status,
            'mode',CASE WHEN json_type(header_json,'$.mode') IS NULL THEN 'planning' ELSE json_extract(header_json,'$.mode') END,
            'team_id',team_id,'owner',json_extract(header_json,'$.owner'),'created_at',json(header_json->'$.created_at'),
            'control_mode',CASE WHEN json_type(header_json,'$.control_loop') IS NULL THEN 'curator'
                WHEN json_type(header_json,'$.control_loop')<>'object' THEN json('invalid control loop')
                WHEN json_type(header_json,'$.control_loop.mode') IS NULL THEN 'curator'
                ELSE json_extract(header_json,'$.control_loop.mode') END,
            'running',json(CASE WHEN json_type(header_json,'$.control_loop.running') IS NULL OR json_type(header_json,'$.control_loop.running')='null' THEN 'false'
                WHEN json_type(header_json,'$.control_loop.running')='object' THEN 'true' ELSE json('invalid running curator') END),
            'last_round',CASE WHEN json_type(header_json,'$.control_loop.last_round') IS NULL OR json_type(header_json,'$.control_loop.last_round')='null' THEN NULL
                WHEN json_type(header_json,'$.control_loop.last_round')='object' THEN json_object(
                    'number',json(coalesce(header_json->'$.control_loop.last_round.number','0')),
                    'ended_at',json(coalesce(header_json->'$.control_loop.last_round.ended_at','0')))
                ELSE json('invalid curator round cursor') END,
            'manual_run_requested',json(CASE WHEN json_type(header_json,'$.control_loop.manual_run_requested') IS NULL THEN 'false'
                WHEN json_type(header_json,'$.control_loop.manual_run_requested') IN ('true','false') THEN header_json->'$.control_loop.manual_run_requested'
                ELSE json('invalid manual curator request') END),
            'last_real_round_at',json(header_json->'$.control_loop.last_real_round_at'),
            'last_synthesis_group_close_at',json(header_json->'$.control_loop.last_synthesis_group_close_at'),
            'owner_turn_cursor',json_extract(header_json,'$.control_loop.owner_turn_cursor'),
            'owner_session_key',json_extract(header_json,'$.control_loop.owner_session_key'),
            'real_rounds_started',json(coalesce(header_json->'$.control_loop.real_rounds_started','0')))
         FROM plans WHERE id=?1",[id],|row|column(row,0)).optional().map_err(db_error)?;
    let scope: CuratorDigestScope = decode(id, &raw.ok_or(PlanError::NotFound)?)?;
    let since = scope
        .last_round
        .as_ref()
        .map(|round| round.ended_at)
        .unwrap_or(now.saturating_sub(codeterm_types::plan::CURATOR_INITIAL_WINDOW_MS));
    let counts = counts(&connection, id)?;
    record_query();
    let lanes=connection.query_row("SELECT count(*) FROM plan_tasks WHERE plan_id=?1 AND CASE
        WHEN json_type(row_json,'$.lane.worktree_path') IS NULL OR json_type(row_json,'$.lane.worktree_path')='null' THEN 0
        WHEN json_type(row_json,'$.lane.worktree_path')<>'text' THEN json('invalid lane worktree')
        WHEN json_type(row_json,'$.lane.cleanup_status') IS NULL OR json_type(row_json,'$.lane.cleanup_status')='null' THEN 1
        WHEN json_type(row_json,'$.lane.cleanup_status')='text' THEN json_extract(row_json,'$.lane.cleanup_status')<>'done'
        ELSE json('invalid lane cleanup status') END",[id],|row|row.get(0)).map_err(db_error)?;
    record_query();
    let recorded:u64=connection.query_row("SELECT count(*) FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='curator_round' AND domios_curator_round(json_extract(row_json,'$.text'))",[id],|row|row.get(0)).map_err(db_error)?;
    let real_rounds = scope.real_rounds_started.max(recorded);
    let group_close = super::curator_queries::schedule_group_close(&connection, id)?;
    record_query();
    let latest:Option<String>=connection.query_row("SELECT max(domios_curator_timestamp(row_json->'$.content.ts')) FROM plan_elements WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='handoff'",[id],|row|column(row,0)).map_err(db_error)?;
    let latest_handoff = latest
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|error| PlanError::Io(error.to_string()))?;
    Ok(CuratorDigestRead {
        connection,
        scope,
        since,
        now,
        counts,
        lanes,
        group_close,
        real_rounds,
        latest_handoff,
    })
}

fn counts(connection: &Connection, id: &str) -> Result<TaskCounts, PlanError> {
    record_query();
    let mut statement = connection
        .prepare_cached(
            "SELECT json_quote(status),count(*) FROM plan_tasks WHERE plan_id=?1 GROUP BY status",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok((column::<String>(row, 0)?, row.get::<_, usize>(1)?))
        })
        .map_err(db_error)?;
    let mut result = TaskCounts::default();
    for row in rows {
        let (raw, count) = row.map_err(db_error)?;
        let status: TaskStatus = decode(id, &raw)?;
        result.total += count;
        match status {
            TaskStatus::Pending => result.pending = count,
            TaskStatus::InProgress => result.in_progress = count,
            TaskStatus::InReview => result.in_review = count,
            TaskStatus::Blocked => result.blocked = count,
            TaskStatus::Paused => result.paused = count,
            TaskStatus::Done => result.done = count,
            TaskStatus::Cancelled => result.cancelled = count,
        }
    }
    Ok(result)
}

impl PlanDatabase {
    pub(in crate::service_plan) fn curator_digest_projection(
        &self,
        id: &str,
        now: u64,
    ) -> Result<CuratorDigestRead, PlanError> {
        open(self, id, now)
    }
}

pub(in crate::service_plan) fn synthesis_context(
    read: &CuratorDigestRead,
    base: &str,
) -> Result<String, PlanError> {
    synthesis::context(read, base)
}

pub(super) fn row_prefix() -> String {
    rows::prefix()
}

pub(in crate::service_plan) fn has_actionable(read: &CuratorDigestRead) -> Result<bool, PlanError> {
    let facts = super::curator_notice_queries::reminder_task_facts();
    record_query();
    read.connection.query_row(&format!("{facts} SELECT EXISTS(SELECT 1 FROM facts WHERE json_extract(decision,'$.orchestrator_actionable'))"),rusqlite::params![read.scope.id,super::status_queries::WHITESPACE],|row|row.get(0)).map_err(db_error)
}

pub(in crate::service_plan) fn heartbeat_body(
    read: &CuratorDigestRead,
) -> Result<String, PlanError> {
    Ok(super::curator_notice_queries::bounded_notice(
        &read.connection,
        &read.scope.id,
        read.now,
        None,
        &[],
    )?
    .body)
}
