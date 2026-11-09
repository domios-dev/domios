use super::db_error;
use super::read_projections::{column, record_query};
use crate::service_plan::curator::digest::{self, DigestBuilder, Row};
use crate::service_plan::{CuratorDigestRead, CuratorRuntimeNotice, PlanError};
use rusqlite::params;
use std::collections::BTreeMap;

const ROWS: &str = "WITH history AS (
    SELECT row_json FROM plan_strides WHERE plan_id=?1
    UNION ALL SELECT row_json FROM plan_task_milestones WHERE plan_id=?1
    ), completed_tasks AS MATERIALIZED (
    SELECT plan_id,task_id FROM plan_tasks WHERE plan_id=?1 AND status='done'
    ), episodes AS (
    SELECT DISTINCT json_extract(s.value,'$.slug') AS slug,t.task_id
    FROM completed_tasks t JOIN plan_task_attempts a USING(plan_id,task_id),json_each(a.row_json,'$.report.loaded_plan_skills') s
    ), carried AS MATERIALIZED (
    SELECT json_extract(l.value,'$.id') AS id,json_extract(l.value,'$.text') AS text
    FROM plan_heartbeat h,json_each(h.row_json,'$.learnings') l
    WHERE h.plan_id=?1 AND json_extract(l.value,'$.tag')='curator:next'
    ), open_tasks AS MATERIALIZED (
    SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND status NOT IN ('done','cancelled')
    ), durable(section,id,kind,state,summary) AS (
    SELECT 'reports',task_id,'report',coalesce(json_extract(row_json,'$.report.status'),'unknown'),
        'sha='||coalesce(json_extract(row_json,'$.report.sha'),'none')||' tests_covering='||CASE
        WHEN substr(json_extract(row_json,'$.report.message'),1,23)='TESTS COVERING CHANGES:'
            OR instr(json_extract(row_json,'$.report.message'),char(10)||'TESTS COVERING CHANGES:')>0 THEN 'yes' ELSE 'no' END
    FROM plan_task_attempts WHERE plan_id=?1 AND json_type(row_json,'$.report')='object' AND domios_curator_after(row_json->'$.reported_at',?2)
    UNION ALL SELECT 'incidents',attempt_id,'attempt',CASE WHEN json_extract(row_json,'$.acknowledged_at') IS NULL THEN 'awaiting_ack' ELSE 'interrupted' END,task_id
    FROM plan_task_attempts WHERE plan_id=?1 AND domios_curator_after(row_json->'$.assigned_at',?2)
        AND (json_extract(row_json,'$.interruption.reason') IS NOT NULL OR json_extract(row_json,'$.acknowledged_at') IS NULL)
    UNION ALL SELECT CASE json_extract(row_json,'$.kind') WHEN 'report' THEN 'reports' WHEN 'worker_report' THEN 'reports'
        WHEN 'skill' THEN 'memory' WHEN 'skill_ledger' THEN 'memory' ELSE 'incidents' END,
        json_extract(row_json,'$.id'),json_extract(row_json,'$.kind'),'recorded',coalesce(json_extract(row_json,'$.text'),'')
    FROM history WHERE domios_curator_after(row_json->'$.ts',?2) AND json_extract(row_json,'$.kind') IN ('report','worker_report','incident','worker_lost','missing_ack','skill','skill_ledger')
    UNION ALL SELECT 'review verdicts',t.task_id,'review',json_extract(r.value,'$.verdict'),coalesce(json_extract(r.value,'$.note'),'')
    FROM plan_tasks t,json_each(t.row_json,'$.review_evidence') r WHERE t.plan_id=?1 AND domios_curator_after(r.value->'$.at',?2)
    UNION ALL SELECT 'decisions',json_extract(row_json,'$.id'),'decision','recorded',row_json->'$.ts'
    FROM plan_strides WHERE plan_id=?1 AND domios_curator_after(row_json->'$.ts',?2) AND json_extract(row_json,'$.kind') IN ('decision','plan_decision','decision_answer')
    UNION ALL SELECT 'carried',c.id,'next-check','open',c.text FROM carried c
        WHERE EXISTS(SELECT 1 FROM open_tasks t WHERE instr(c.text,t.task_id)>0)
    UNION ALL SELECT 'candidates','plan-skill:'||slug,'plan_skill','repeated',group_concat(task_id,' ')
    FROM (SELECT slug,task_id FROM episodes ORDER BY slug,task_id) GROUP BY slug HAVING count(*)>=2
    ), all_rows AS (
    SELECT * FROM durable
    UNION ALL SELECT * FROM input_rows
    UNION ALL SELECT s.key,json_extract(r.value,'$.id'),json_extract(r.value,'$.kind'),json_extract(r.value,'$.state'),json_extract(r.value,'$.summary')
    FROM json_each(?3) s,json_each(s.value) r
    ), unique_rows AS (SELECT DISTINCT section,id,kind,state,summary FROM all_rows), ranked AS (
    SELECT *,count(*) OVER(PARTITION BY section) AS total,
        row_number() OVER(PARTITION BY section ORDER BY CASE WHEN section='time in state' THEN summary END DESC,id,kind,state,summary) AS rank
    FROM unique_rows
    ), limits AS (SELECT CAST(key AS INTEGER) AS ordering,json_extract(value,'$[0]') AS section,json_extract(value,'$[1]') AS cap FROM json_each(?4))
    SELECT r.section,domios_curator_clean(id,64),domios_curator_clean(kind,32),domios_curator_clean(state,32),domios_curator_clean(summary,120),total,
        EXISTS(SELECT 1 FROM unique_rows u JOIN limits USING(section) WHERE domios_curator_activity(u.section))
    FROM ranked r JOIN limits USING(section)
    WHERE rank<=cap ORDER BY limits.ordering,rank";

impl CuratorDigestRead {
    pub fn build_digest(
        &self,
        header: &str,
        rows: &BTreeMap<String, Vec<Row>>,
        limit: usize,
    ) -> Result<digest::Digest, PlanError> {
        self.build_digest_with_runtime(header, rows, limit, None)
    }

    pub fn build_digest_with_runtime(
        &self,
        header: &str,
        extra: &BTreeMap<String, Vec<Row>>,
        limit: usize,
        notices: Option<&[CuratorRuntimeNotice]>,
    ) -> Result<digest::Digest, PlanError> {
        let extra = serde_json::to_string(extra).map_err(super::json_error)?;
        let limits = serde_json::to_string(&digest::SECTIONS).map_err(super::json_error)?;
        let notices = notices
            .map(serde_json::to_string)
            .transpose()
            .map_err(super::json_error)?;
        let sql = format!(
            "{}, {}",
            super::curator_input_queries::row_prefix(),
            ROWS.strip_prefix("WITH ")
                .ok_or_else(|| PlanError::Io("curator row query has no WITH prefix".into()))?
        );
        record_query();
        let mut statement = self.connection.prepare_cached(&sql).map_err(db_error)?;
        let mut rows = statement
            .query(params![
                self.scope.id,
                self.since.to_string(),
                extra,
                limits,
                notices,
                self.now.to_string(),
                super::status_queries::WHITESPACE
            ])
            .map_err(db_error)?;
        let mut builder = None;
        let mut section = String::new();
        let mut accepting = false;
        while let Some(row) = rows.next().map_err(db_error)? {
            let name: String = column(row, 0).map_err(db_error)?;
            if builder.is_none() {
                builder = Some(DigestBuilder::new(
                    header,
                    limit,
                    row.get(6).map_err(db_error)?,
                ));
            }
            let build = builder
                .as_mut()
                .ok_or_else(|| PlanError::Io("curator builder missing".into()))?;
            if section != name {
                section = name;
                if !build.start_section(&section, row.get(5).map_err(db_error)?) {
                    break;
                }
                accepting = true;
            }
            if accepting {
                let selected = Row {
                    id: column(row, 1).map_err(db_error)?,
                    kind: column(row, 2).map_err(db_error)?,
                    state: column(row, 3).map_err(db_error)?,
                    summary: column(row, 4).map_err(db_error)?,
                };
                accepting = build.append(&selected);
            }
        }
        Ok(builder
            .unwrap_or_else(|| DigestBuilder::new(header, limit, false))
            .finish())
    }
}
