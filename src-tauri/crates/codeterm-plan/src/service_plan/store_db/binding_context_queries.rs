use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::{KpiPlanContextRead, PlanError, TaskBindingRead};
use rusqlite::{params, OptionalExtension};

const ASSIGNED: &str = "SELECT p.id AS plan_id,json_extract(t.row_json,'$.id') AS task_id
    FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
    WHERE p.status='active' AND t.assigned_tab_id=?1
        AND t.status IN ('pending','in_progress','in_review','blocked','paused')
        AND (?2=0 OR (json_extract(p.header_json,'$.mode')='execution'
            AND t.status IN ('pending','in_progress','blocked')))
    ORDER BY p.updated_at DESC,p.id,t.position LIMIT 1";

const TASKLESS: &str = "SELECT id FROM plans WHERE status='active' AND (
    (json_extract(header_json,'$.mode')='execution' AND owner_tab_id=?1)
    OR (json_extract(header_json,'$.planner.kind')='tab'
        AND json_extract(header_json,'$.planner.tab_id')=?1)) ORDER BY id";

impl PlanDatabase {
    pub(in crate::service_plan) fn assigned_task_binding_projection(
        &self,
        tab: &str,
        report_pending: bool,
    ) -> Result<Option<TaskBindingRead>, PlanError> {
        if tab.trim().is_empty() {
            return Ok(None);
        }
        let connection = self.connection.lock();
        record_query();
        let encoded: Option<(String,String)> = connection.query_row(
            &format!("SELECT plan_id,json_object('plan_id',plan_id,'task_id',task_id) FROM ({ASSIGNED})"),
            params![tab,report_pending], |row| Ok((column(row,0)?,column(row, 1)?)),
        ).optional().map_err(db_error)?;
        encoded.map(|(id, value)| decode(&id, &value)).transpose()
    }

    pub(in crate::service_plan) fn planner_binding_projection(
        &self,
        tab: &str,
    ) -> Result<Option<String>, PlanError> {
        if tab.trim().is_empty() {
            return Ok(None);
        }
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                "SELECT id FROM plans WHERE status='active'
                AND json_extract(header_json,'$.planner.kind')='tab'
                AND json_extract(header_json,'$.planner.tab_id')=?1
             ORDER BY updated_at DESC,id LIMIT 1",
                [tab],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)
    }

    pub(in crate::service_plan) fn taskless_plan_bindings_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<String>, PlanError> {
        if tab.trim().is_empty() {
            return Ok(Vec::new());
        }
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection.prepare_cached(TASKLESS).map_err(db_error)?;
        let rows = statement
            .query_map([tab], |row| column(row, 0))
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn kpi_plan_context_projection(
        &self,
        tab: &str,
    ) -> Result<KpiPlanContextRead, PlanError> {
        if tab.trim().is_empty() {
            return Ok(KpiPlanContextRead::default());
        }
        let connection = self.connection.lock();
        record_query();
        let (id,encoded): (Option<String>,String) = connection.query_row(
            &format!("WITH assigned AS ({ASSIGNED}),taskless AS ({TASKLESS} LIMIT 2),
                context AS (SELECT CASE WHEN EXISTS(SELECT 1 FROM assigned)
                    THEN (SELECT plan_id FROM assigned)
                    WHEN (SELECT count(*) FROM taskless)=1 THEN (SELECT id FROM taskless) END AS plan_id,
                    (SELECT task_id FROM assigned) AS task_id)
                SELECT plan_id,json_object('plan_id',plan_id,'task_id',task_id) FROM context"),
            params![tab,false], |row| Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        decode(id.as_deref().unwrap_or("unbound KPI context"), &encoded)
    }
}
