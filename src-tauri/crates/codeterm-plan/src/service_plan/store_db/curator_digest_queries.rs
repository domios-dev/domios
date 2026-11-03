use super::db_error;
use super::read_projections::{column, record_query};
use crate::service_plan::curator::digest::{self, PreparedRows, PreparedSection, Row};
use crate::service_plan::{CuratorDigestRead, PlanError};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

const ROWS: &str = "WITH history AS (
    SELECT row_json FROM plan_strides WHERE plan_id=?1
    UNION ALL SELECT row_json FROM plan_task_milestones WHERE plan_id=?1
    ), episodes AS (
    SELECT DISTINCT json_extract(s.value,'$.slug') AS slug,t.task_id
    FROM plan_tasks t JOIN plan_task_attempts a USING(plan_id,task_id),json_each(a.row_json,'$.report.loaded_plan_skills') s
    WHERE t.plan_id=?1 AND t.status='done'
    ), durable(section,id,kind,state,summary) AS (
    SELECT 'reports',task_id,'report',coalesce(json_extract(row_json,'$.report.status'),'unknown'),
        'sha='||coalesce(json_extract(row_json,'$.report.sha'),'none')||' tests_covering='||CASE
        WHEN substr(json_extract(row_json,'$.report.message'),1,23)='TESTS COVERING CHANGES:'
            OR instr(json_extract(row_json,'$.report.message'),char(10)||'TESTS COVERING CHANGES:')>0 THEN 'yes' ELSE 'no' END
    FROM plan_task_attempts WHERE plan_id=?1 AND json_type(row_json,'$.report')='object' AND json_extract(row_json,'$.reported_at')>?2
    UNION ALL SELECT 'incidents',attempt_id,'attempt',CASE WHEN json_extract(row_json,'$.acknowledged_at') IS NULL THEN 'awaiting_ack' ELSE 'interrupted' END,task_id
    FROM plan_task_attempts WHERE plan_id=?1 AND json_extract(row_json,'$.assigned_at')>?2
        AND (json_extract(row_json,'$.interruption.reason') IS NOT NULL OR json_extract(row_json,'$.acknowledged_at') IS NULL)
    UNION ALL SELECT CASE json_extract(row_json,'$.kind') WHEN 'report' THEN 'reports' WHEN 'worker_report' THEN 'reports'
        WHEN 'skill' THEN 'memory' WHEN 'skill_ledger' THEN 'memory' ELSE 'incidents' END,
        json_extract(row_json,'$.id'),json_extract(row_json,'$.kind'),'recorded',coalesce(json_extract(row_json,'$.text'),'')
    FROM history WHERE json_extract(row_json,'$.ts')>?2 AND json_extract(row_json,'$.kind') IN ('report','worker_report','incident','worker_lost','missing_ack','skill','skill_ledger')
    UNION ALL SELECT 'review verdicts',t.task_id,'review',json_extract(r.value,'$.verdict'),coalesce(json_extract(r.value,'$.note'),'')
    FROM plan_tasks t,json_each(t.row_json,'$.review_evidence') r WHERE t.plan_id=?1 AND json_extract(r.value,'$.at')>?2
    UNION ALL SELECT 'decisions',json_extract(row_json,'$.id'),'decision','recorded',CAST(json_extract(row_json,'$.ts') AS TEXT)
    FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.ts')>?2 AND json_extract(row_json,'$.kind') IN ('decision','plan_decision','decision_answer')
    UNION ALL SELECT 'carried',json_extract(l.value,'$.id'),'next-check','open',json_extract(l.value,'$.text')
    FROM plan_heartbeat h,json_each(h.row_json,'$.learnings') l WHERE h.plan_id=?1 AND json_extract(l.value,'$.tag')='curator:next'
        AND EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=?1 AND t.status NOT IN ('done','cancelled') AND instr(json_extract(l.value,'$.text'),t.task_id)>0)
    UNION ALL SELECT 'candidates','plan-skill:'||slug,'plan_skill','repeated',group_concat(task_id,' ')
    FROM (SELECT slug,task_id FROM episodes ORDER BY slug,task_id) GROUP BY slug HAVING count(*)>=2
    ), all_rows AS (
    SELECT * FROM durable
    UNION ALL SELECT s.key,json_extract(r.value,'$.id'),json_extract(r.value,'$.kind'),json_extract(r.value,'$.state'),json_extract(r.value,'$.summary')
    FROM json_each(?3) s,json_each(s.value) r
    ), unique_rows AS (SELECT DISTINCT section,id,kind,state,summary FROM all_rows), ranked AS (
    SELECT *,count(*) OVER(PARTITION BY section) AS total,
        row_number() OVER(PARTITION BY section ORDER BY CASE WHEN section='time in state' THEN summary END DESC,id,kind,state,summary) AS rank
    FROM unique_rows
    ), limits AS (SELECT json_extract(value,'$[0]') AS section,json_extract(value,'$[1]') AS cap FROM json_each(?4))
    SELECT r.section,id,kind,state,domios_curator_prefix(summary),total FROM ranked r JOIN limits USING(section)
    WHERE rank<=cap ORDER BY r.section,rank";

impl CuratorDigestRead {
    pub fn build_digest(
        &self,
        header: &str,
        rows: &BTreeMap<String, Vec<Row>>,
        limit: usize,
    ) -> Result<digest::Digest, PlanError> {
        let prepared = prepared_rows(&self.connection, &self.plan.id, self.since, rows)?;
        Ok(digest::build_prepared(header, &prepared, limit))
    }
}

pub(super) fn prepared_rows(
    connection: &Connection,
    plan_id: &str,
    since: u64,
    extra: &BTreeMap<String, Vec<Row>>,
) -> Result<PreparedRows, PlanError> {
    let extra = serde_json::to_string(extra).map_err(|error| PlanError::Io(error.to_string()))?;
    let limits = serde_json::to_string(&digest::SECTIONS)
        .map_err(|error| PlanError::Io(error.to_string()))?;
    record_query();
    let mut statement = connection.prepare_cached(ROWS).map_err(db_error)?;
    let mut sections = PreparedRows::new();
    for row in statement
        .query_map(params![plan_id, since, extra, limits], |row| {
            Ok((
                column::<String>(row, 0)?,
                Row {
                    id: column(row, 1)?,
                    kind: column(row, 2)?,
                    state: column(row, 3)?,
                    summary: column(row, 4)?,
                },
                row.get::<_, usize>(5)?,
            ))
        })
        .map_err(db_error)?
    {
        let (name, row, total) = row.map_err(db_error)?;
        sections
            .entry(name)
            .or_insert_with(|| PreparedSection {
                rows: Vec::new(),
                total,
            })
            .rows
            .push(row);
    }
    Ok(sections)
}
