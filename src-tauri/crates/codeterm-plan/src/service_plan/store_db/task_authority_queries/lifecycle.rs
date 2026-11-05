use super::super::document_queries::regression::unsigned_timestamp;
use super::*;
use crate::service_plan::LiveTabAttempt;

impl PlanDatabase {
    pub(in crate::service_plan) fn live_task_attempts_projection(
        &self,
        executor: &str,
    ) -> Result<Vec<LiveTabAttempt>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let epoch = unsigned_timestamp("a.row_json", "interruption.owner_epoch", true);
        let ack = unsigned_timestamp("a.row_json", "acknowledged_at", false);
        let retired = unsigned_timestamp("a.row_json", "retired_at", false);
        let mut statement = connection.prepare(&format!(
            "WITH selected AS MATERIALIZED (
                SELECT plan_id,task_id,max(position) AS position FROM plan_task_attempts
                WHERE executor_tab_id=?1 AND json_extract(row_json,'$.closed_at') IS NULL
                GROUP BY plan_id,task_id)
             SELECT p.id,json_object('plan_id',p.id,'task_id',t.task_id,'task_handle',json_extract(t.row_json,'$.handle'),
                'plan_active',json(CASE WHEN p.status='active' THEN 'true' ELSE 'false' END),'task_status',t.status,
                'attempt',json_object('id',json_extract(a.row_json,'$.id'),'executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'),
                    'acknowledged_at',json(CASE WHEN json_extract(a.row_json,'$.acknowledged_at') IS NOT NULL THEN {ack} END),
                    'retired_at',json(CASE WHEN json_extract(a.row_json,'$.retired_at') IS NOT NULL THEN {retired} END),
                    'orchestrator_tab_id',json_extract(a.row_json,'$.orchestrator_tab_id'),
                    'parent_tab_id',json_extract(a.row_json,'$.parent_tab_id'),'owner_epoch',json({epoch})))
             FROM selected c JOIN plans p ON p.id=c.plan_id
                JOIN plan_tasks t ON t.plan_id=c.plan_id AND t.task_id=c.task_id
                JOIN plan_task_attempts a ON a.plan_id=c.plan_id AND a.task_id=c.task_id AND a.position=c.position
             ORDER BY p.updated_at DESC,p.id,t.position"
        )).map_err(db_error)?;
        let rows = statement
            .query_map([executor], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (plan, raw) = row.map_err(db_error)?;
            decode(&plan, &raw)
        })
        .collect()
    }

    pub(in crate::service_plan) fn superseded_attempt_projection(
        &self,
        tab: &str,
    ) -> Result<Option<(String, String, String, String, String)>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String> = connection
            .query_row(
                "WITH owned_last AS MATERIALIZED (
                    SELECT plan_id,task_id,max(position) AS position FROM plan_task_attempts
                    WHERE executor_tab_id=?1 AND json_extract(row_json,'$.executor_tab_id')=?1 GROUP BY plan_id,task_id
                 ), owned AS MATERIALIZED (
                    SELECT a.plan_id,a.task_id,a.position,json_extract(a.row_json,'$.id') AS attempt_id
                    FROM owned_last latest JOIN plan_task_attempts a USING(plan_id,task_id,position)
                    WHERE json_extract(a.row_json,'$.closed_at') IS NOT NULL
                 ), unclosed AS MATERIALIZED (
                    SELECT plan_id,task_id,max(position) AS position FROM plan_task_attempts
                    WHERE (plan_id,task_id) IN (SELECT plan_id,task_id FROM owned)
                        AND json_extract(row_json,'$.closed_at') IS NULL AND json_extract(row_json,'$.retired_at') IS NULL
                    GROUP BY plan_id,task_id
                 ) SELECT json_array(p.id,t.task_id,o.attempt_id,
                    json_extract(a.row_json,'$.id'),json_extract(a.row_json,'$.executor_tab_id'))
                 FROM owned o JOIN plans p ON p.id=o.plan_id
                    JOIN plan_tasks t ON t.plan_id=o.plan_id AND t.task_id=o.task_id
                    JOIN unclosed latest ON latest.plan_id=o.plan_id AND latest.task_id=o.task_id AND latest.position>o.position
                    JOIN plan_task_attempts a ON a.plan_id=latest.plan_id AND a.task_id=latest.task_id AND a.position=latest.position
                 ORDER BY p.updated_at DESC,p.id,t.position LIMIT 1",
                [tab],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?;
        raw.map(|raw| decode("superseded attempt", &raw))
            .transpose()
    }

    pub(in crate::service_plan) fn runtime_ready_subject_projection(
        &self,
        tab: &str,
    ) -> Result<(u64, Option<(String, String, String)>), PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: String = connection.query_row(
            "WITH latest AS MATERIALIZED (
                SELECT plan_id,task_id,max(position) AS position FROM plan_task_attempts
                WHERE executor_tab_id=?1 AND json_extract(row_json,'$.executor_tab_id')=?1 AND json_extract(row_json,'$.closed_at') IS NULL
                GROUP BY plan_id,task_id
             ), candidates AS MATERIALIZED (
                SELECT p.id AS plan_id,t.task_id,a.position,json_extract(a.row_json,'$.id') AS attempt_id
                FROM latest chosen JOIN plans p ON p.id=chosen.plan_id
                    JOIN plan_tasks t ON t.plan_id=chosen.plan_id AND t.task_id=chosen.task_id
                    JOIN plan_task_attempts a ON a.plan_id=chosen.plan_id AND a.task_id=chosen.task_id AND a.position=chosen.position
                WHERE p.status='active' AND t.status IN ('pending','in_progress'))
             SELECT json_array((SELECT count(*) FROM candidates),
                json((SELECT json_array(plan_id,task_id,attempt_id) FROM candidates
                    WHERE (SELECT count(*) FROM candidates)=1 LIMIT 1)))",
            [tab], |row|column(row,0),
        ).map_err(db_error)?;
        decode("runtime-ready subject", &raw)
    }
}
