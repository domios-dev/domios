use super::super::document_queries::regression::unsigned_timestamp;
use super::*;
use crate::service_plan::{ProviderRecoveryRead, TaskInterruptionRead};

const ACTIVE_INTERRUPTION: &str = "(json_extract(a.row_json,'$.reported_at') IS NULL OR (
    t.task_status='in_progress'
    AND coalesce(json_extract(a.row_json,'$.continuation.epoch'),0)>0
    AND json_extract(a.row_json,'$.continuation.state')='continued'
    AND json_extract(a.row_json,'$.continuation.delivered_at') IS NOT NULL))";

impl PlanDatabase {
    pub(in crate::service_plan) fn provider_exhaustion_tabs_projection(
        &self,
        provider: &str,
    ) -> Result<Vec<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection.prepare(&format!("WITH relevant_attempts AS MATERIALIZED (
                SELECT plan_id,task_id,row_json FROM plan_task_attempts
                WHERE json_extract(row_json,'$.closed_at') IS NULL AND json_extract(row_json,'$.provider')=?1
             ), task_states AS MATERIALIZED (
                SELECT plan_id,task_id,json_extract(row_json,'$.status') AS task_status FROM plan_tasks
                WHERE (plan_id,task_id) IN (SELECT plan_id,task_id FROM relevant_attempts)
             ) SELECT DISTINCT json_extract(a.row_json,'$.executor_tab_id')
            FROM relevant_attempts a JOIN task_states t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
            WHERE json_extract(a.row_json,'$.closed_at') IS NULL
                AND json_extract(a.row_json,'$.provider')=?1 AND {ACTIVE_INTERRUPTION}
            ORDER BY json_extract(a.row_json,'$.executor_tab_id')")).map_err(db_error)?;
        let rows = statement
            .query_map([provider], |row| column(row, 0))
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn provider_recovery_projection(
        &self,
        provider: &str,
    ) -> Result<Vec<ProviderRecoveryRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let owner = unsigned_timestamp("t.epoch_json", "interruption.owner_epoch", true);
        let mut statement = connection.prepare(&format!("WITH recovery_tasks AS MATERIALIZED (
                SELECT t.plan_id,t.task_id,t.position,json_extract(t.row_json,'$.interruption.owner_tab_id') AS owner_tab,
                    json_extract(t.row_json,'$.interruption.owner_session_id') AS owner_session,
                    CASE WHEN json_type(t.row_json,'$.interruption.owner_epoch') IS NULL THEN '{{}}'
                        ELSE json_object('interruption',json_object('owner_epoch',json(t.row_json->'$.interruption.owner_epoch'))) END AS epoch_json
                FROM plan_tasks t WHERE json_extract(t.row_json,'$.status_label')=?2
                    AND json_extract(t.row_json,'$.interruption.owner_session_id') IS NOT NULL
             ) SELECT p.id,json_object('plan_id',p.id,'task_id',t.task_id,
                'owner_tab_id',t.owner_tab,
                'owner_session_id',t.owner_session,
                'owner_epoch',json({owner}))
            FROM plans p JOIN recovery_tasks t ON t.plan_id=p.id
            WHERE (SELECT json_extract(a.row_json,'$.provider') FROM plan_task_attempts a
                    WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                        AND json_extract(a.row_json,'$.closed_at') IS NULL
                        AND json_extract(a.row_json,'$.executor_tab_id')=t.owner_tab
                    ORDER BY a.position LIMIT 1)=?1
            ORDER BY p.updated_at DESC,p.id,t.position")).map_err(db_error)?;
        let rows = statement
            .query_map(
                params![
                    provider,
                    crate::service_plan::interruption::PROVIDER_EXHAUSTED_LABEL
                ],
                |row| Ok((column::<String>(row, 0)?, column::<String>(row, 1)?)),
            )
            .map_err(db_error)?;
        rows.map(|row| {
            let (plan, raw) = row.map_err(db_error)?;
            decode(&plan, &raw)
        })
        .collect()
    }

    pub(in crate::service_plan) fn task_interruption_projection(
        &self,
        tab_id: &str,
    ) -> Result<Option<TaskInterruptionRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let owner = unsigned_timestamp("t.row_json", "interruption.owner_epoch", true);
        let attempt_epoch = unsigned_timestamp("a.row_json", "interruption.owner_epoch", true);
        let selected: Option<(String,String)> = connection.query_row(&format!("WITH interruption_tasks AS MATERIALIZED (
            SELECT t.plan_id,t.task_id,t.position,{owner} AS owner_epoch,json_extract(t.row_json,'$.status') AS task_status
            FROM plan_tasks t WHERE json_extract(t.row_json,'$.interruption.execution_intent')='mutating'
                AND json_extract(t.row_json,'$.interruption.owner_tab_id')=?1
                AND coalesce(json_extract(t.row_json,'$.interruption.handoff_required'),0)=0
                AND json_extract(t.row_json,'$.interruption.owner_session_id') IS NOT NULL
                AND json_extract(t.row_json,'$.lane.worktree_path') IS NOT NULL
                AND EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                    AND json_extract(a.row_json,'$.executor_tab_id')=?1 AND json_extract(a.row_json,'$.closed_at') IS NULL)
            ), selected AS (
            SELECT p.id AS plan_id,t.task_id,a.position,t.owner_epoch FROM plans p
            JOIN interruption_tasks t ON t.plan_id=p.id JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
            WHERE json_extract(a.row_json,'$.executor_tab_id')=?1 AND json_extract(a.row_json,'$.closed_at') IS NULL
                AND {attempt_epoch}=t.owner_epoch
                AND {ACTIVE_INTERRUPTION}
            ORDER BY p.updated_at DESC,p.id,t.position,a.position LIMIT 1)
            SELECT p.id,json_object('plan_id',p.id,'task_id',t.task_id,'attempt_id',json_extract(a.row_json,'$.id'),
                'owner_session_id',json_extract(t.row_json,'$.interruption.owner_session_id'),
                'owner_epoch',json(s.owner_epoch),
                'repo_path',CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?2)=''
                    THEN json_extract(p.header_json,'$.project_path') ELSE json_extract(p.header_json,'$.canonical_project_path') END,
                'worktree_path',json_extract(t.row_json,'$.lane.worktree_path'),
                'base_head',json_extract(t.row_json,'$.lane.base'),'test_plan',json_extract(t.row_json,'$.test_plan'))
            FROM selected s JOIN plans p ON p.id=s.plan_id JOIN plan_tasks t ON t.plan_id=s.plan_id AND t.task_id=s.task_id
                JOIN plan_task_attempts a ON a.plan_id=s.plan_id AND a.task_id=s.task_id AND a.position=s.position"),
            params![tab_id,super::super::status_queries::WHITESPACE],|row|Ok((column(row,0)?,column(row,1)?)))
            .optional().map_err(db_error)?;
        selected.map(|(plan, raw)| decode(&plan, &raw)).transpose()
    }
}
