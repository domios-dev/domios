use super::super::super::super::db_error;
use super::super::{column, decode, gate_rows, record_query, ReminderGateRead};
use super::{ReminderAdmissionTaskRead, ReminderOwnerRowRead};
use crate::{
    plan::{PlanMode, PlanStatus},
    service_plan::PlanError,
};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

pub(super) const HEADER:&str="SELECT json_object('id',id,'status',status,'mode',coalesce(json_extract(header_json,'$.mode'),'planning'),
 'planning_state',coalesce(json_extract(header_json,'$.planning_state'),'drafting'),'project_path',project_path,
 'team_id',team_id,'owner_tab_id',owner_tab_id,
 'control_loop',json_object('mode',coalesce(json_extract(header_json,'$.control_loop.mode'),'curator'),'heartbeat_interval_secs',json_extract(header_json,'$.control_loop.heartbeat_interval_secs'),
 'has_fallback',json(CASE WHEN json_type(header_json,'$.control_loop.fallback_reason') IS NULL OR json_type(header_json,'$.control_loop.fallback_reason')='null' THEN 'false' ELSE 'true' END)),
 'complete',json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=plans.id) AND NOT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=plans.id AND status NOT IN ('done','cancelled')) THEN 'true' ELSE 'false' END),
 'owner_open_work',json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=plans.id AND status IN ('pending','in_progress','in_review','blocked')) THEN 'true' ELSE 'false' END)) FROM plans WHERE id=?1";

pub(super) fn projection(
    connection: &Connection,
    id: &str,
    mode: &PlanMode,
    status: &PlanStatus,
    task: Option<&str>,
) -> Result<BTreeMap<String, String>, PlanError> {
    // Fold task-local maps in task order before plan strides, preserving duplicate-ID overwrite order.
    let tasks =
        gate_rows::TASK_EVIDENCE.replacen("SELECT t.task_id,", "SELECT t.position,t.task_id,", 1);
    let attempts = gate_rows::ATTEMPT_EVIDENCE
        .replacen(
            "SELECT a.task_id,",
            "SELECT t.position,a.position,a.task_id,",
            1,
        )
        .replace("?2 IS NULL OR a.task_id=?2", "?3 IS NULL OR a.task_id=?3");
    let milestones = gate_rows::TASK_MILESTONE_HASHES
        .replacen("SELECT m.task_id,", "SELECT t.position,m.seq,m.task_id,", 1)
        .replace("?2 IS NULL OR m.task_id=?2", "?3 IS NULL OR m.task_id=?3");
    let sql=format!("WITH task_rows(position,task_id,task_hash,review_hash,extra) AS ({tasks}),attempt_rows(position,ordinal,task_id,body,attempt_hash,report_hash) AS ({attempts}),milestone_rows(position,ordinal,task_id,stride_id,hash) AS ({milestones})
        SELECT position,0,task_id,0,task_hash FROM task_rows
        UNION ALL SELECT position,1,task_id,0,review_hash FROM task_rows
        UNION ALL SELECT position,2,json_extract(body,'$.id'),ordinal,attempt_hash FROM attempt_rows
        UNION ALL SELECT position,3,json_extract(body,'$.report_id'),ordinal,report_hash FROM attempt_rows WHERE report_hash IS NOT NULL
        UNION ALL SELECT position,4,stride_id,ordinal,hash FROM milestone_rows
        ORDER BY 1,2,4");
    record_query();
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let rows = statement
        .query_map(
            params![
                id,
                super::super::super::super::status_queries::WHITESPACE,
                task
            ],
            |row| {
                Ok((
                    row.get::<_, usize>(1)?,
                    column::<String>(row, 2)?,
                    column::<String>(row, 4)?,
                ))
            },
        )
        .map_err(db_error)?;
    let mut out = BTreeMap::from([("mode".into(), format!("{mode:?}:{status:?}"))]);
    for row in rows {
        let (kind, id, hash) = row.map_err(db_error)?;
        let prefix = ["task", "review", "attempt", "report", "stride"][kind];
        out.insert(format!("{prefix}:{id}"), hash);
    }
    if task.is_none() {
        record_query();
        let mut statement = connection
            .prepare_cached(gate_rows::PLAN_STRIDE_HASHES)
            .map_err(db_error)?;
        let rows = statement
            .query_map([id], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        for row in rows {
            let (id, hash) = row.map_err(db_error)?;
            out.insert(format!("stride:{id}"), hash);
        }
        record_query();
        let mut statement = connection
            .prepare_cached(gate_rows::GATE_HASHES)
            .map_err(db_error)?;
        let rows = statement
            .query_map([id], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        for row in rows {
            let (body, hash) = row.map_err(db_error)?;
            let gate: ReminderGateRead = decode(id, &body)?;
            out.insert(format!("gate:{}", gate.id), hash);
        }
    }
    Ok(out)
}
pub(super) fn owner_catalog(
    connection: &Connection,
    id: &str,
    gates: &mut Vec<ReminderGateRead>,
    learnings: &mut BTreeMap<String, String>,
) -> Result<(), PlanError> {
    record_query();
    let mut statement = connection
        .prepare_cached(gate_rows::GATE_HASHES)
        .map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| column::<String>(row, 0))
        .map_err(db_error)?;
    for row in rows {
        gates.push(decode(id, &row.map_err(db_error)?)?);
    }
    record_query();
    let mut statement = connection
        .prepare_cached(gate_rows::LEARNING_CATALOG)
        .map_err(db_error)?;
    let rows = statement
        .query_map(
            params![id, super::super::super::super::status_queries::WHITESPACE],
            |row| Ok((column::<String>(row, 0)?, column::<String>(row, 1)?)),
        )
        .map_err(db_error)?;
    for row in rows {
        let (key, value) = row.map_err(db_error)?;
        learnings.insert(key, value);
    }
    Ok(())
}

fn task_body() -> String {
    "json_object('id',t.task_id,'status',t.status,'assigned_tab_id',t.assigned_tab_id,
 'actionable',json(CASE WHEN json_extract(f.decision,'$.category')>=0 THEN 'true' ELSE 'false' END),
 'held',json(CASE WHEN json_extract(f.decision,'$.held') THEN 'true' ELSE 'false' END),
 'status_changed_at',json(coalesce(t.row_json->'$.status_changed_at','0')),'lane_path',json_extract(t.row_json,'$.lane.worktree_path'),'lane_branch',json_extract(t.row_json,'$.lane.branch'),
 'attempts',json((SELECT json_group_array(json(body)) FROM open_attempts a WHERE a.task_id=t.task_id)))".into()
}
fn open_attempts() -> String {
    format!(",attempt_rows(task_id,body,hash,report_hash) AS ({}),open_attempts AS MATERIALIZED (SELECT task_id,body FROM attempt_rows WHERE json_extract(body,'$.closed')=0)",gate_rows::ATTEMPT_EVIDENCE)
}
pub(super) fn worker(
    connection: &Connection,
    id: &str,
    task: &str,
) -> Result<Option<ReminderAdmissionTaskRead>, PlanError> {
    let facts = super::super::super::super::curator_notice_queries::reminder_task_facts();
    let body = task_body();
    let attempts = open_attempts();
    let sql=format!("{facts}{attempts} SELECT {body} FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id WHERE t.plan_id=?1 AND t.task_id=?3");
    // Open attempts use ?2 as task filter in their shared query, while presentation uses ?2 as whitespace.
    let sql = sql.replace("?2 IS NULL OR a.task_id=?2", "?3 IS NULL OR a.task_id=?3");
    record_query();
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let mut rows = statement
        .query(params![
            id,
            super::super::super::super::status_queries::WHITESPACE,
            task
        ])
        .map_err(db_error)?;
    rows.next()
        .map_err(db_error)?
        .map(|row| decode(id, &column::<String>(row, 0).map_err(db_error)?))
        .transpose()
}
pub(super) fn actionable(connection: &Connection, id: &str) -> Result<Vec<String>, PlanError> {
    record_query();
    let facts = super::super::super::super::curator_notice_queries::reminder_task_facts();
    let sql=format!("{facts} SELECT task_id FROM facts WHERE json_extract(decision,'$.category')>=0 ORDER BY position");
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let rows = statement
        .query_map(
            params![id, super::super::super::super::status_queries::WHITESPACE],
            |row| column(row, 0),
        )
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub(super) fn owner_rows(
    connection: &Connection,
    id: &str,
    delta: &BTreeMap<String, String>,
) -> Result<Vec<ReminderOwnerRowRead>, PlanError> {
    let facts = super::super::super::super::curator_notice_queries::reminder_task_facts();
    let attempts = open_attempts().replace("(?2 IS NULL OR a.task_id=?2)", "1");
    let body = task_body();
    let sql=format!("{facts}{attempts},delta AS MATERIALIZED (SELECT key FROM json_each(?3)),events(position,phase,ordinal,body) AS (
        SELECT t.position,0,0,json_object('kind','wake','event',CASE t.status WHEN 'done' THEN 'task_closed' WHEN 'cancelled' THEN 'task_closed' WHEN 'blocked' THEN 'task_blocked' ELSE 'task_state' END,'id',t.task_id) FROM plan_tasks t WHERE t.plan_id=?1 AND 'task:'||t.task_id IN(SELECT key FROM delta)
        UNION ALL SELECT t.position,1,0,json_object('kind','wake','event','failed_gate','id',json_extract(t.row_json,'$.review_evidence[#-1].id')) FROM plan_tasks t WHERE t.plan_id=?1 AND json_extract(t.row_json,'$.review_evidence[#-1].verdict')='bounce' AND 'review:'||t.task_id IN(SELECT key FROM delta)
        UNION ALL SELECT t.position,2,a.position*2,json_object('kind','wake','event','incident','id','dispatch_or_worker_failure:'||a.attempt_id) FROM plan_task_attempts a JOIN plan_tasks t ON t.plan_id=a.plan_id AND t.task_id=a.task_id WHERE a.plan_id=?1 AND json_type(a.row_json,'$.interruption.reason') IS NOT NULL AND json_type(a.row_json,'$.interruption.reason')<>'null' AND (json_extract(a.row_json,'$.closed_at') IS NULL OR 'attempt:'||a.attempt_id IN(SELECT key FROM delta))
        UNION ALL SELECT t.position,2,a.position*2+1,json_object('kind','wake','event','new_report','id',json_extract(a.row_json,'$.report.id')) FROM plan_task_attempts a JOIN plan_tasks t ON t.plan_id=a.plan_id AND t.task_id=a.task_id WHERE a.plan_id=?1 AND 'report:'||json_extract(a.row_json,'$.report.id') IN(SELECT key FROM delta)
        UNION ALL SELECT t.position,3,0,json_object('kind','wake','event','incident','id','dispatch_required:'||t.task_id) FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id WHERE t.plan_id=?1 AND t.status='pending' AND t.assigned_tab_id IS NULL AND json_extract(f.decision,'$.category')>=0
        UNION ALL SELECT t.position,4,0,json_object('kind','worker','task',json({body})) FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id WHERE t.plan_id=?1 AND t.status IN ('pending','in_progress','in_review','blocked') AND EXISTS(SELECT 1 FROM open_attempts a WHERE a.task_id=t.task_id)
        UNION ALL SELECT t.position,5,0,json_object('kind','wake','event','incident','id','dispatch_missing:'||t.task_id) FROM plan_tasks t WHERE t.plan_id=?1 AND t.status='in_progress' AND NOT EXISTS(SELECT 1 FROM open_attempts a WHERE a.task_id=t.task_id)
        UNION ALL SELECT t.position,6,0,json_object('kind','wake','event','incident','id','task_interruption:'||t.task_id) FROM plan_tasks t WHERE t.plan_id=?1 AND t.status IN ('pending','in_progress','in_review','blocked') AND json_extract(t.row_json,'$.interruption.handoff_required')=1
    ) SELECT body FROM events ORDER BY position,phase,ordinal");
    record_query();
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let delta = serde_json::to_string(delta).map_err(|error| PlanError::Io(error.to_string()))?;
    let rows = statement
        .query_map(
            params![
                id,
                super::super::super::super::status_queries::WHITESPACE,
                delta
            ],
            |row| column::<String>(row, 0),
        )
        .map_err(db_error)?;
    rows.map(|row| decode(id, &row.map_err(db_error)?))
        .collect()
}

pub(super) fn context(
    connection: &Connection,
    id: &str,
    delta: &BTreeMap<String, String>,
    limit: usize,
) -> Result<(BTreeMap<String, serde_json::Value>, bool), PlanError> {
    let facts = super::super::super::super::curator_notice_queries::reminder_task_facts();
    let project="domios_reminder_projection(t.status,coalesce(json_extract(t.row_json,'$.review_stage'),'review'),json_extract(t.row_json,'$.qa_state'),
        (SELECT json_group_array(json_object('task_id',dep.value,'name','', 'status',target.status,'missing',json(CASE WHEN target.task_id IS NULL THEN 'true' ELSE 'false' END))) FROM json_each(coalesce(t.row_json->'$.depends_on','[]')) dep LEFT JOIN dependency_targets target ON target.task_id=dep.value WHERE target.task_id IS NULL OR target.status NOT IN ('done','cancelled')),
        coalesce(json_extract(t.row_json,'$.interruption.handoff_required'),0),parent.parent_id IS NOT NULL,json_extract(f.decision,'$.parked'))";
    let rows = gate_rows::DELTA_CONTEXT
        .strip_prefix("WITH ")
        .expect("shared context CTE");
    let rows=rows.replace("FROM plan_tasks t JOIN json_each(?3) j ON json_extract(j.value,'$.id')=t.task_id WHERE t.plan_id=?1","FROM plan_tasks t JOIN projected j ON j.task_id=t.task_id WHERE t.plan_id=?1")
        .replace("json_extract(j.value,'$.projection.presentation')","json_extract(j.projection,'$.presentation')")
        .replace("j.value->'$.projection.dependency_waits'","j.projection->'$.dependency_waits'")
        .replace("json_each(?2) d","json_each(?3) d")
        .replace("json(j.value->'$.held')","json(CASE WHEN j.held THEN 'true' ELSE 'false' END)");
    let sql=format!("{facts},projected AS MATERIALIZED (SELECT t.task_id,{project} AS projection,json_extract(f.decision,'$.held') AS held FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id LEFT JOIN open_parents parent ON parent.parent_id=t.task_id WHERE t.plan_id=?1 AND 'task:'||t.task_id IN(SELECT key FROM json_each(?3))),{rows}");
    let delta = serde_json::to_string(delta).map_err(|error| PlanError::Io(error.to_string()))?;
    record_query();
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let rows = statement
        .query_map(
            params![
                id,
                super::super::super::super::status_queries::WHITESPACE,
                delta,
                limit.saturating_add(1)
            ],
            |row| Ok((column::<String>(row, 0)?, column::<String>(row, 1)?)),
        )
        .map_err(db_error)?;
    let mut out = BTreeMap::new();
    let mut truncated = false;
    for row in rows {
        let (key, raw) = row.map_err(db_error)?;
        if out.len() == limit {
            truncated = true;
            break;
        }
        out.insert(key, decode(id, &raw)?);
    }
    Ok((out, truncated))
}
