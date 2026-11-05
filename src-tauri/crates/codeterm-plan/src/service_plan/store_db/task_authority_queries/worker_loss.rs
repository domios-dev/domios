use super::*;
use crate::service_plan::{WorkerLossRead, WorkerLossRecoveryRead};

impl PlanDatabase {
    pub(in crate::service_plan) fn worker_loss_projection(
        &self,
        plan: Option<&str>,
        task: Option<&str>,
        tab: &str,
        in_progress_only: bool,
    ) -> Result<Vec<WorkerLossRead>, PlanError> {
        self.worker_loss_rows(plan, task, tab, in_progress_only, true)
    }

    pub(in crate::service_plan) fn worker_loss_recovery_projection(
        &self,
        plan: &str,
        task: &str,
        tab: &str,
    ) -> Result<Option<WorkerLossRecoveryRead>, PlanError> {
        self.worker_loss_rows(Some(plan), Some(task), tab, false, false)
            .map(|rows: Vec<WorkerLossRecoveryRead>| rows.into_iter().next())
    }

    fn worker_loss_rows<T: serde::de::DeserializeOwned>(
        &self,
        plan: Option<&str>,
        task: Option<&str>,
        tab: &str,
        in_progress_only: bool,
        include_metadata: bool,
    ) -> Result<Vec<T>, PlanError> {
        let metadata = if include_metadata {
            "'plan_id',p.id,'task_id',t.task_id,'task_name',json_extract(t.row_json,'$.name'),
                'orchestrator_tab_id',CASE WHEN json_extract(p.header_json,'$.owner.tab_id')=?3 THEN NULL ELSE json_extract(p.header_json,'$.owner.tab_id') END,
                'attempt_id',json_extract(own.row_json,'$.id'),
                'worktree_path',coalesce(json_extract(own.row_json,'$.worktree_path'),json_extract(t.row_json,'$.lane.worktree_path')),
                'branch',coalesce(json_extract(own.row_json,'$.branch'),json_extract(t.row_json,'$.lane.branch')),"
        } else {
            ""
        };
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare(&format!("WITH requested AS (
            SELECT p.id AS plan_id,t.task_id FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
            WHERE (?1 IS NULL OR p.id=?1) AND (?2 IS NULL OR t.task_id=?2)
                AND (NOT ?4 OR (p.status='active' AND t.status='in_progress' AND (
                    json_extract(t.row_json,'$.assigned_tab_id')=?3 OR json_extract(t.row_json,'$.lane.tab_id')=?3 OR EXISTS(
                        SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND json_extract(a.row_json,'$.executor_tab_id')=?3 AND json_extract(a.row_json,'$.closed_at') IS NULL))))),
            source AS MATERIALIZED (SELECT r.plan_id,r.task_id,(SELECT a.position FROM plan_task_attempts a
                WHERE a.plan_id=r.plan_id AND a.task_id=r.task_id AND json_extract(a.row_json,'$.executor_tab_id')=?3
                ORDER BY a.position DESC LIMIT 1) AS position FROM requested r),
            owned_ids AS MATERIALIZED (
                SELECT s.plan_id,s.task_id,json_extract(own.row_json,'$.id') AS attempt_id
                FROM source s LEFT JOIN plan_task_attempts own ON own.plan_id=s.plan_id AND own.task_id=s.task_id AND own.position=s.position
            ), boundary AS (SELECT s.plan_id,s.task_id,(SELECT a.position FROM plan_task_attempts a
                WHERE a.plan_id=s.plan_id AND a.task_id=s.task_id AND json_extract(a.row_json,'$.id')=s.attempt_id
                ORDER BY a.position DESC LIMIT 1) AS position
                FROM owned_ids s)
            SELECT p.id,json_object({metadata}
                'latest_attempt_id',(SELECT json_extract(a.row_json,'$.id') FROM plan_task_attempts a
                    WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id ORDER BY a.position DESC LIMIT 1),
                'replacement',json((SELECT json_object('attempt_id',json_extract(a.row_json,'$.id'),'executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'))
                    FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                        AND a.position>b.position AND json_extract(a.row_json,'$.closed_at') IS NULL ORDER BY a.position DESC LIMIT 1)))
            FROM source s JOIN plans p ON p.id=s.plan_id JOIN plan_tasks t ON t.plan_id=s.plan_id AND t.task_id=s.task_id
                JOIN boundary b ON b.plan_id=s.plan_id AND b.task_id=s.task_id
                LEFT JOIN plan_task_attempts own ON own.plan_id=s.plan_id AND own.task_id=s.task_id AND own.position=s.position
            ORDER BY p.updated_at DESC,p.id,t.position")).map_err(db_error)?;
        let rows = statement
            .query_map(params![plan, task, tab, in_progress_only], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (plan, raw) = row.map_err(db_error)?;
            decode(&plan, &raw)
        })
        .collect()
    }
}
