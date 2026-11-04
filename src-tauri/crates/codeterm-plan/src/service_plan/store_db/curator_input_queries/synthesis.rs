use super::super::db_error;
use super::super::document_queries::regression::unsigned_timestamp;
use super::super::read_projections::{column, decode, record_query};
use crate::service_plan::curator::synthesis::{self, SynthesisGoalInput};
use crate::service_plan::{CuratorDigestRead, PlanError};
use rusqlite::params;

#[derive(serde::Deserialize)]
struct GoalRead {
    vision: String,
    vector: String,
    open_criteria: usize,
}

pub(in crate::service_plan) fn context(
    read: &CuratorDigestRead,
    base: &str,
) -> Result<String, PlanError> {
    record_query();
    let raw: String=read.connection.query_row(
        "SELECT json_object('vision',CASE WHEN json_type(p.header_json,'$.goal') IS NULL OR json_type(p.header_json,'$.goal')='null' THEN ''
            WHEN json_type(p.header_json,'$.goal')='object' THEN CASE WHEN json_type(p.header_json,'$.goal.vision') IS NULL THEN '' ELSE domios_curator_bounded(json_extract(p.header_json,'$.goal.vision'),512) END ELSE json('invalid goal') END,
            'vector',CASE WHEN json_type(p.header_json,'$.goal') IS NULL OR json_type(p.header_json,'$.goal')='null' THEN ''
            WHEN json_type(p.header_json,'$.goal')='object' THEN domios_curator_bounded(json_extract(p.header_json,'$.goal.vector'),512) ELSE json('invalid goal') END,
            'open_criteria',(SELECT coalesce(sum(CASE WHEN json_type(row_json,'$.acceptance_criteria') IS NULL THEN 0
                WHEN json_type(row_json,'$.acceptance_criteria')='array' THEN json_array_length(row_json,'$.acceptance_criteria') ELSE json('invalid acceptance criteria') END),0)
                FROM plan_tasks WHERE plan_id=p.id AND status NOT IN ('done','cancelled')))
         FROM plans p WHERE p.id=?1",[&read.scope.id],|row|column(row,0)).map_err(db_error)?;
    let goal: GoalRead = decode(&read.scope.id, &raw)?;
    let mut body = synthesis::context_prefix(
        SynthesisGoalInput {
            id: &read.scope.id,
            title: &read.scope.title,
            vision: &goal.vision,
            vector: &goal.vector,
            open_criteria: goal.open_criteria,
        },
        base,
    );
    let budget = synthesis::history_budget(&body);
    let stride_at = unsigned_timestamp("s.row_json", "ts", false);
    let learning_at = unsigned_timestamp("l.value", "ts", false);
    let sql=format!("WITH selected AS MATERIALIZED (
        SELECT seq,row_json FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='curator_round'
            AND domios_curator_round(json_extract(row_json,'$.text')) ORDER BY seq DESC LIMIT 64
     ), rounds AS MATERIALIZED (
        SELECT s.seq,json_extract(s.row_json,'$.id') AS id,{stride_at} AS at,
            domios_curator_round_lines(json_extract(s.row_json,'$.text')) AS document FROM selected s
     ), learnings AS MATERIALIZED (
        SELECT CAST(l.key AS INTEGER) AS position,{learning_at} AS at,
            json_extract(l.value,'$.text') AS text FROM plan_heartbeat h,json_each(h.row_json,'$.learnings') l
        WHERE h.plan_id=?1 AND json_extract(l.value,'$.tag')='curator'
            AND EXISTS(SELECT 1 FROM rounds WHERE json_extract(document,'$.fallback'))
     ), evidence AS (
        SELECT r.seq,r.id,r.at,CAST(l.key AS INTEGER) AS position,
            json_extract(l.value,'$[0]') AS kind,json_extract(l.value,'$[1]') AS text
        FROM rounds r,json_each(r.document,'$.lines') l
        UNION ALL SELECT r.seq,r.id,r.at,l.position,
            domios_curator_history_kind(l.text),l.text FROM rounds r JOIN learnings l
                ON l.at=r.document->'$.ended_at' WHERE json_extract(r.document,'$.fallback')
     ), lines AS MATERIALIZED (
        SELECT domios_curator_history_line(id,kind,text) AS line,
            row_number() OVER(ORDER BY printf('%20s',at) DESC,id DESC,seq DESC,
                CASE kind WHEN 'MOVE' THEN 0 WHEN 'RETRO' THEN 1 ELSE 2 END,position) AS ordinal FROM evidence
     ), budgeted AS (
        SELECT line,ordinal,sum(length(CAST(line AS BLOB))) OVER(ORDER BY ordinal) AS bytes FROM lines
     ) SELECT coalesce(group_concat(line,''),'') FROM (SELECT line FROM budgeted WHERE bytes<=?2 ORDER BY ordinal)");
    record_query();
    let history: String = read
        .connection
        .query_row(&sql, params![read.scope.id, budget], |row| column(row, 0))
        .map_err(db_error)?;
    body.push_str(&history);
    Ok(body)
}
