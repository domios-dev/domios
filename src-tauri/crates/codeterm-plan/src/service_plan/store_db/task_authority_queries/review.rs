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
    pub(in crate::service_plan) fn closed_review_continuation_projection(
        &self,
        plan_id: &str,
        task_ref: &str,
    ) -> Result<Option<ReviewContinuation>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (status,raw): (Option<String>,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN}, matching AS (
                SELECT a.plan_id,a.task_id,a.position,
                    json_extract(a.row_json,'$.closed_at') IS NULL AS unclosed
                FROM plan_task_attempts a JOIN selected_task t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
                WHERE json_extract(a.row_json,'$.acknowledged_at') IS NOT NULL
                    AND json_extract(a.row_json,'$.reported_at') IS NOT NULL
                    AND trim(coalesce(json_extract(a.row_json,'$.session_id'),''),?3)<>''
                    AND CASE WHEN json_extract(a.row_json,'$.competition_group_id') IS NOT NULL
                        THEN json_type(a.row_json,'$.winner')='true'
                        ELSE t.assigned_tab_id=a.executor_tab_id OR (
                            json_extract(t.row_json,'$.status')='in_review'
                            AND json_extract(t.row_json,'$.status_label')=?4 AND t.assigned_tab_id IS NULL
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
