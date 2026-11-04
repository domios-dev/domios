use super::*;
use crate::plan::TaskStatus;
use crate::service_plan::review_gate::ReviewContinuation;

#[derive(serde::Deserialize)]
struct ClosedReviewRead {
    attempt_id: String,
    executor_tab_id: String,
    session_id: String,
    mesh_peer: Option<String>,
    epoch: u32,
    review_round: u32,
    state: String,
    #[serde(rename = "acknowledged_at")]
    _acknowledged_at: u64,
    #[serde(rename = "reported_at")]
    _reported_at: u64,
    #[serde(rename = "closed_at")]
    _closed_at: u64,
    #[serde(rename = "retired_at")]
    _retired_at: Option<u64>,
    #[serde(rename = "assigned_tab_id")]
    _assigned_tab_id: Option<String>,
    #[serde(rename = "competition_group_id")]
    _competition_group_id: Option<String>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn task_review_worker_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let worker: Option<String> = connection
            .query_row(
                "SELECT CASE WHEN json_extract(a.row_json,'$.mesh_peer') IS NULL
                THEN json_extract(a.row_json,'$.executor_tab_id') END
             FROM plan_task_attempts a JOIN plans p ON p.id=a.plan_id AND p.status='active'
             WHERE a.plan_id=?1 AND a.task_id=?2 ORDER BY a.position DESC LIMIT 1",
                params![plan, task],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        Ok(worker)
    }

    pub(in crate::service_plan) fn task_lane_integration_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<crate::service_plan::LaneIntegrationRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String> = connection.query_row(
            "WITH holder AS (SELECT t.task_id,a.position FROM plan_tasks t JOIN plan_task_attempts a
                ON a.plan_id=t.plan_id AND a.task_id=t.task_id WHERE t.plan_id=?1
                    AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL
                    AND CASE WHEN json_type(a.row_json,'$.placement') IS NULL THEN 0
                        WHEN json_type(a.row_json,'$.placement')='text' THEN json_extract(a.row_json,'$.placement')<>'isolated_lane'
                        ELSE json('invalid worker placement') END
                ORDER BY t.position,a.position LIMIT 1)
             SELECT json_object('plan_id',p.id,'task_id',t.task_id,'worktree_path',json_extract(t.row_json,'$.lane.worktree_path'),
                'canonical_path',CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?3)=''
                    THEN json_extract(p.header_json,'$.project_path') ELSE json_extract(p.header_json,'$.canonical_project_path') END,
                'holder',json_object('task_id',h.task_id,'attempt_id',json_extract(a.row_json,'$.id'),
                    'placement',json_extract(a.row_json,'$.placement')))
             FROM plans p JOIN plan_tasks t ON t.plan_id=p.id JOIN holder h
                JOIN plan_task_attempts a ON a.plan_id=p.id AND a.task_id=h.task_id AND a.position=h.position
             WHERE p.id=?1 AND t.task_id=?2 AND h.task_id<>t.task_id
                AND trim(coalesce(json_extract(t.row_json,'$.lane.worktree_path'),''),?3)<>''",
            params![plan,task,super::super::status_queries::WHITESPACE],|row|column(row,0),
        ).optional().map_err(db_error)?;
        raw.map(|raw| decode(plan, &raw)).transpose()
    }

    pub(in crate::service_plan) fn closed_review_continuation_projection(
        &self,
        plan_id: &str,
        task_ref: &str,
    ) -> Result<Option<ReviewContinuation>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (status,raw): (Option<String>,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN}, selected_facts AS MATERIALIZED (
                SELECT plan_id,task_id,assigned_tab_id,json_extract(row_json,'$.status') AS status,
                    json_extract(row_json,'$.status_label') AS status_label FROM selected_task
             ), matching AS (
                SELECT a.plan_id,a.task_id,a.position,
                    json_extract(a.row_json,'$.closed_at') IS NULL AS unclosed
                FROM plan_task_attempts a JOIN selected_facts t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
                WHERE json_extract(a.row_json,'$.acknowledged_at') IS NOT NULL
                    AND json_extract(a.row_json,'$.reported_at') IS NOT NULL
                    AND trim(coalesce(json_extract(a.row_json,'$.session_id'),''),?3)<>''
                    AND CASE WHEN json_extract(a.row_json,'$.competition_group_id') IS NOT NULL
                        THEN json_type(a.row_json,'$.winner')='true'
                        ELSE t.assigned_tab_id=a.executor_tab_id OR (
                            t.status='in_review'
                            AND t.status_label=?4 AND t.assigned_tab_id IS NULL
                            AND json_extract(a.row_json,'$.retired_at') IS NOT NULL) END)
             SELECT (SELECT json_quote(json_extract(row_json,'$.status')) FROM selected_task),
                (SELECT json_object('attempt_id',json_extract(a.row_json,'$.id'),'executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'),
                    'session_id',json_extract(a.row_json,'$.session_id'),
                    'mesh_peer',coalesce(json_extract(a.row_json,'$.mesh_peer'),json_extract(t.row_json,'$.peer')),
                    'epoch',coalesce(json_extract(a.row_json,'$.continuation.epoch'),0),
                    'review_round',coalesce(json_extract(t.row_json,'$.review_round'),0),
                    'state',coalesce(json_extract(a.row_json,'$.continuation.state'),'executor_gone'),
                    'acknowledged_at',json_extract(a.row_json,'$.acknowledged_at'),
                    'reported_at',json_extract(a.row_json,'$.reported_at'),'closed_at',json_extract(a.row_json,'$.closed_at'),
                    'retired_at',json_extract(a.row_json,'$.retired_at'),'assigned_tab_id',json_extract(t.row_json,'$.assigned_tab_id'),
                    'competition_group_id',json_extract(a.row_json,'$.competition_group_id'))
                 FROM selected_task t JOIN plans p ON p.id=t.plan_id
                    JOIN matching m ON m.plan_id=t.plan_id AND m.task_id=t.task_id
                    JOIN plan_task_attempts a ON a.plan_id=m.plan_id AND a.task_id=m.task_id AND a.position=m.position
                 WHERE (json_extract(t.row_json,'$.status')='in_review' OR (
                    json_extract(t.row_json,'$.status')='in_progress' AND json_extract(t.row_json,'$.status_label')
                        IN ('awaiting_continuation','bounce_continuation_failed','review bounced')))
                    AND m.unclosed=0 AND NOT EXISTS(SELECT 1 FROM matching WHERE unclosed=1)
                    AND (SELECT count(*) FROM matching WHERE unclosed=0)=1 LIMIT 1)"),
            params![plan_id,task_ref,super::super::status_queries::WHITESPACE,crate::plan::reconcile::TAB_GONE_LABEL],
            |row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        let Some(status) = status else {
            return Ok(None);
        };
        let _status: TaskStatus = decode(plan_id, &status)?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        let read: ClosedReviewRead = decode(plan_id, &raw)?;
        Ok(Some(ReviewContinuation {
            attempt_id: read.attempt_id,
            executor_tab_id: read.executor_tab_id,
            session_id: read.session_id,
            mesh_peer: read.mesh_peer,
            epoch: read.epoch,
            review_round: read.review_round,
            state: read.state,
        }))
    }
}
