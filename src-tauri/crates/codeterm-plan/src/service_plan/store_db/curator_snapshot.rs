use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{CuratorRound, Milestone, Plan};
use crate::service_plan::{CuratorDigestRead, PlanError};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

const HEADER: &str = "SELECT json_remove(header_json,
    '$.planning_log','$.owner_history','$.lifecycle_events','$.pause_operations','$.planner_requests','$.planner_inbox',
    '$.control_loop.rounds','$.control_loop.save_candidates','$.control_loop.save_candidate_shapes') FROM plans WHERE id=?1";

impl PlanDatabase {
    pub(in crate::service_plan) fn curator_digest_projection(
        &self,
        id: &str,
        now: u64,
    ) -> Result<CuratorDigestRead, PlanError> {
        let connection = Connection::open_with_flags(
            self.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        super::status_queries::register_functions(&connection)?;
        register_functions(&connection)?;
        // A separate WAL reader keeps the snapshot while live-source callbacks use the writer.
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION;")
            .map_err(db_error)?;
        record_query();
        let header: String = connection
            .query_row(HEADER, [id], |row| column(row, 0))
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        let mut plan: Plan = decode(id, &header)?;
        super::status_queries::load_tasks(&connection, &mut plan)?;
        super::status_queries::load_sections(&connection, &mut plan)?;
        super::status_queries::load_attempts(&connection, &mut plan)?;
        super::status_queries::load_history(&connection, &mut plan)?;
        record_query();
        let recorded: u64=connection.query_row(
            "SELECT count(*) FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='curator_round' AND domios_curator_round(json_extract(row_json,'$.text'))",
            [id],|row|row.get(0),
        ).map_err(db_error)?;
        let real_rounds = plan.control_loop.real_rounds_started.max(recorded);
        load_rounds(&connection, &mut plan)?;
        load_markers(&connection, &mut plan)?;
        let learning_ids = fallback_learning_ids(&connection, &plan)?;
        super::status_queries::load_heartbeat(&connection, &mut plan, &learning_ids)?;
        let since = plan
            .control_loop
            .last_round
            .as_ref()
            .map(|round| round.ended_at)
            .unwrap_or(now.saturating_sub(codeterm_types::plan::CURATOR_INITIAL_WINDOW_MS));
        record_query();
        let latest_handoff: Option<String> = connection.query_row(
            "SELECT max(domios_curator_timestamp(row_json->'$.content.ts')) FROM plan_elements WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='handoff'",
            [id], |row| column(row,0),
        ).map_err(db_error)?;
        let latest_handoff = latest_handoff
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|error| PlanError::Io(error.to_string()))?;
        Ok(CuratorDigestRead {
            connection,
            plan,
            since,
            real_rounds,
            latest_handoff,
        })
    }
}

fn load_rounds(connection: &Connection, plan: &mut Plan) -> Result<(), PlanError> {
    plan.strides.retain(|stride| stride.kind != "curator_round");
    record_query();
    let mut statement=connection.prepare_cached(
        "SELECT json_remove(row_json,'$.attachments','$.canvas') FROM (
            SELECT seq,row_json FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='curator_round'
              AND domios_curator_round(json_extract(row_json,'$.text')) ORDER BY seq DESC LIMIT 64) ORDER BY seq"
    ).map_err(db_error)?;
    for row in statement
        .query_map([&plan.id], |row| column::<String>(row, 0))
        .map_err(db_error)?
    {
        plan.strides
            .push(decode(&plan.id, &row.map_err(db_error)?)?);
    }
    Ok(())
}

fn marker(id: String, kind: &str, ts: u64, links: Vec<String>) -> Milestone {
    Milestone {
        id,
        kind: kind.into(),
        from: Default::default(),
        text: String::new(),
        ts,
        links,
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn load_markers(connection: &Connection, plan: &mut Plan) -> Result<(), PlanError> {
    record_query();
    let closed: Option<u64>=connection.query_row(
        "SELECT max(json_extract(row_json,'$.ts')) FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='group_close'",
        [&plan.id],|row|row.get(0),
    ).map_err(db_error)?;
    if let Some(ts) = closed {
        plan.strides.push(marker(
            "sql-curator-close".into(),
            "group_close",
            ts,
            vec![],
        ));
    }
    record_query();
    let mut statement=connection.prepare_cached(
        "SELECT t.task_id,max(json_extract(s.row_json,'$.ts')) FROM plan_tasks t
         JOIN plan_strides s ON s.plan_id=t.plan_id,json_each(s.row_json,'$.links') link
         WHERE t.plan_id=?1 AND t.status='done' AND link.value=t.task_id AND json_extract(s.row_json,'$.kind') IN ('owner_notice','shown')
         GROUP BY t.task_id HAVING max(json_extract(s.row_json,'$.ts'))>=coalesce(json_extract(t.row_json,'$.status_changed_at'),0)"
    ).map_err(db_error)?;
    for row in statement
        .query_map([&plan.id], |row| {
            Ok((column::<String>(row, 0)?, row.get::<_, u64>(1)?))
        })
        .map_err(db_error)?
    {
        let (id, ts) = row.map_err(db_error)?;
        plan.strides.push(marker(
            format!("sql-curator-shown-{id}"),
            "shown",
            ts,
            vec![id],
        ));
    }
    Ok(())
}

fn fallback_learning_ids(connection: &Connection, plan: &Plan) -> Result<Vec<String>, PlanError> {
    let mut rounds = Vec::new();
    for (order, stride) in plan
        .strides
        .iter()
        .filter(|stride| stride.kind == "curator_round")
        .enumerate()
    {
        let round: CuratorRound =
            serde_json::from_str(&stride.text).map_err(|error| PlanError::Quarantined {
                plan_id: plan.id.clone(),
                reason: error.to_string(),
            })?;
        if round.retro.is_empty() && round.learn.is_empty() {
            rounds.push(serde_json::json!({"id":stride.id,"ts":stride.ts,"ended_at":round.ended_at,"order":order}));
        }
    }
    let rounds =
        serde_json::to_string(&rounds).map_err(|error| PlanError::Io(error.to_string()))?;
    record_query();
    let mut statement=connection.prepare_cached(
        "WITH matches AS (SELECT json_extract(l.value,'$.id') AS id,l.key,
            json_extract(r.value,'$.ts') AS ts,json_extract(r.value,'$.id') AS round_id,json_extract(r.value,'$.order') AS round_order,
            row_number() OVER(PARTITION BY l.key ORDER BY json_extract(r.value,'$.ts') DESC,json_extract(r.value,'$.id') DESC,json_extract(r.value,'$.order') DESC) AS rank
         FROM plan_heartbeat h,json_each(h.row_json,'$.learnings') l
         JOIN json_each(?2) r ON json_extract(r.value,'$.ended_at')=json_extract(l.value,'$.ts')
         WHERE h.plan_id=?1 AND json_extract(l.value,'$.tag')='curator')
         SELECT id FROM matches WHERE rank=1 ORDER BY ts DESC,round_id DESC,round_order DESC,key LIMIT 4097"
    ).map_err(db_error)?;
    let rows = statement
        .query_map(params![plan.id, rounds], |row| column::<String>(row, 0))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    use rusqlite::functions::FunctionFlags;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_curator_round", 1, flags, |context| {
            let rusqlite::types::ValueRef::Text(bytes) = context.get_raw(0) else {
                return Ok(false);
            };
            if bytes.len() > 16 * 1024 {
                return Ok(false);
            }
            Ok(serde_json::from_slice::<CuratorRound>(bytes)
                .ok()
                .is_some_and(|round| !round.digest_hash.is_empty()))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_prefix", 1, flags, |context| {
            let text = context
                .get_raw(0)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            Ok(text.chars().take(120).collect::<String>())
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_timestamp", 1, flags, |context| {
            let value = match context.get_raw(0) {
                rusqlite::types::ValueRef::Text(bytes) => serde_json::from_slice::<u64>(bytes)
                    .ok()
                    .map(|value| format!("{value:020}")),
                _ => None,
            };
            Ok(value)
        })
        .map_err(db_error)
}
