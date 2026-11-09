use super::super::document_queries::regression::unsigned_timestamp;
use super::*;
use crate::service_plan::{BatchTaskRefusalRead, TaskSelectorRead};

fn selection() -> String {
    let updated = unsigned_timestamp("t.row_json", "updated_at", true);
    let created = unsigned_timestamp("t.row_json", "created_at", true);
    format!("requested AS MATERIALIZED (SELECT key AS request_index,value AS selector FROM json_each(?2)),
        task_metadata AS MATERIALIZED (
            SELECT t.task_id,t.position,json_extract(t.row_json,'$.handle') AS handle,
                coalesce(json_extract(t.row_json,'$.key'),CASE WHEN substr(t.task_id,1,5)='task_' THEN substr(t.task_id,6) ELSE t.task_id END) AS key,
                CASE WHEN EXISTS(SELECT 1 FROM requested WHERE selector='@latest') THEN {updated} ELSE '0' END AS updated,
                CASE WHEN EXISTS(SELECT 1 FROM requested WHERE selector='@latest') THEN {created} ELSE '0' END AS created
            FROM plan_tasks t WHERE t.plan_id=?1 AND EXISTS(SELECT 1 FROM requested)
        ), candidates AS (
        SELECT r.request_index,r.selector,t.task_id,
            sum(t.task_id=r.selector) OVER (PARTITION BY r.request_index) AS exact_count,
            count(*) OVER (PARTITION BY r.request_index) AS matches,
            row_number() OVER (PARTITION BY r.request_index ORDER BY
                CASE WHEN r.selector='@latest' THEN printf('%20s',t.updated) END DESC,
                CASE WHEN r.selector='@latest' THEN printf('%20s',t.created) END DESC,
                CASE WHEN r.selector<>'@latest' THEN t.task_id=r.selector END DESC,t.position DESC) AS ordinal
        FROM requested r CROSS JOIN task_metadata t WHERE r.selector='@latest' OR t.task_id=r.selector OR (
            r.selector<>'' AND (t.handle COLLATE NOCASE=r.selector OR t.key COLLATE NOCASE=r.selector))),
        selected AS (SELECT request_index,task_id FROM candidates WHERE ordinal=1
            AND (selector='@latest' OR exact_count>0 OR matches=1))")
}

fn encode_selectors(selectors: &[String]) -> Result<String, PlanError> {
    serde_json::to_string(selectors).map_err(|error| PlanError::Io(error.to_string()))
}

impl PlanDatabase {
    pub(in crate::service_plan) fn batch_task_resolution_projection(
        &self,
        plan: &str,
        selectors: &[String],
    ) -> Result<Vec<TaskSelectorRead>, PlanError> {
        let selectors = encode_selectors(selectors)?;
        let connection = self.connection.lock();
        record_query();
        let (exists,raw): (bool,String) = connection.query_row(
            &format!("WITH {} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_group_array(json_object('index',request_index,'task_id',task_id)) FROM (SELECT * FROM selected ORDER BY request_index))",selection()),
            params![plan,selectors],|row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw)
    }

    pub(in crate::service_plan) fn batch_task_refusal_projection(
        &self,
        plan: &str,
        selectors: &[String],
        caller: Option<&str>,
    ) -> Result<Option<BatchTaskRefusalRead>, PlanError> {
        let selectors = encode_selectors(selectors)?;
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String> = connection.query_row(
            &format!("WITH {}, owner_context AS MATERIALIZED (SELECT p.id,CASE WHEN ?3 IS NULL THEN NULL ELSE CASE WHEN json_type(p.header_json,'$.owner.tab_id') IS NULL OR json_type(p.header_json,'$.owner.tab_id')='null' THEN NULL WHEN json_type(p.header_json,'$.owner.tab_id')='text' THEN trim(json_extract(p.header_json,'$.owner.tab_id'),?4) ELSE json('invalid owner tab') END END AS owner_tab FROM plans p WHERE p.id=?1) SELECT json_object('task_id',t.task_id,'name',json_extract(t.row_json,'$.name'),
                    'assigned_tab_id',json_extract(t.row_json,'$.assigned_tab_id'))
                FROM owner_context p CROSS JOIN selected s JOIN plan_tasks t ON t.plan_id=p.id AND t.task_id=s.task_id
                WHERE NOT (?3 IS NOT NULL AND coalesce(p.owner_tab=?3,0))
                    AND (trim(coalesce(json_extract(t.row_json,'$.assigned_tab_id'),''),?4)<>'' OR EXISTS(
                        SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL
                            AND json_extract(a.row_json,'$.competition_group_id') IS NOT NULL))
                    AND NOT (?3 IS NOT NULL AND (coalesce(json_extract(t.row_json,'$.assigned_tab_id')=?3,0) OR EXISTS(
                        SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND json_extract(a.row_json,'$.executor_tab_id')=?3 AND json_extract(a.row_json,'$.closed_at') IS NULL
                            AND json_extract(a.row_json,'$.retired_at') IS NULL AND json_extract(a.row_json,'$.competition_group_id') IS NOT NULL)))
                ORDER BY s.request_index LIMIT 1",selection()),
            params![plan,selectors,caller,super::super::status_queries::WHITESPACE],|row|column(row,0),
        ).optional().map_err(db_error)?;
        raw.map(|raw| decode(plan, &raw)).transpose()
    }
}
