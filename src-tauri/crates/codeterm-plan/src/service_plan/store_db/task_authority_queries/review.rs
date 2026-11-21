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

#[derive(serde::Deserialize)]
struct LaneIntegrationCandidate {
    target_task_id: String,
    worktree_path: String,
    target_plan_canonical: String,
    holder_plan_id: String,
    holder_task_id: String,
    holder_attempt_id: String,
    holder_placement: crate::plan::WorkerPlacement,
    holder_checkout: String,
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
        let sql = format!(
            "WITH target AS (
                SELECT p.id AS plan_id,
                    t.task_id,
                    CASE
                        WHEN trim(coalesce(json_extract(t.row_json,'$.lane.worktree_path'),''),?3)<>''
                            THEN json_extract(t.row_json,'$.lane.worktree_path')
                        ELSE (
                            SELECT json_extract(la.row_json,'$.worktree_path')
                            FROM plan_task_attempts la
                            WHERE la.plan_id=t.plan_id AND la.task_id=t.task_id
                                AND coalesce(json_extract(la.row_json,'$.placement'),'isolated_lane')='isolated_lane'
                                AND trim(coalesce(json_extract(la.row_json,'$.worktree_path'),''),?3)<>''
                            ORDER BY la.position DESC LIMIT 1
                        )
                    END AS worktree_path,
                    CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?3)=''
                        THEN json_extract(p.header_json,'$.project_path')
                        ELSE json_extract(p.header_json,'$.canonical_project_path')
                    END AS plan_canonical
                FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
                WHERE p.id=?1 AND t.task_id=?2
            ),
            candidates AS (
                SELECT
                    target.task_id AS target_task_id,
                    target.worktree_path,
                    target.plan_canonical AS target_plan_canonical,
                    hp.id AS holder_plan_id,
                    t.task_id AS holder_task_id,
                    json_extract(a.row_json,'$.id') AS holder_attempt_id,
                    json_extract(a.row_json,'$.placement') AS holder_placement,
                    CASE
                        WHEN trim(coalesce(json_extract(a.row_json,'$.checkout_path'),''),?3)<>''
                            THEN json_extract(a.row_json,'$.checkout_path')
                        WHEN trim(coalesce(json_extract(a.row_json,'$.worktree_path'),''),?3)<>''
                            THEN json_extract(a.row_json,'$.worktree_path')
                        WHEN trim(coalesce(json_extract(hp.header_json,'$.canonical_project_path'),''),?3)<>''
                            THEN json_extract(hp.header_json,'$.canonical_project_path')
                        ELSE json_extract(hp.header_json,'$.project_path')
                    END AS holder_checkout,
                    (a.plan_id=?1) AS same_plan,
                    hp.updated_at AS holder_plan_updated_at,
                    t.position AS holder_task_pos,
                    a.position AS holder_attempt_pos
                FROM target
                JOIN plan_task_attempts a ON a.open
                JOIN plan_tasks t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
                JOIN plans hp ON hp.id=a.plan_id
                WHERE trim(coalesce(target.worktree_path,''),?3)<>''
                    AND NOT (a.plan_id=target.plan_id AND a.task_id=target.task_id)
                    AND {writer}
            )
            SELECT json_object(
                'target_task_id',target_task_id,
                'worktree_path',worktree_path,
                'target_plan_canonical',target_plan_canonical,
                'holder_plan_id',holder_plan_id,
                'holder_task_id',holder_task_id,
                'holder_attempt_id',holder_attempt_id,
                'holder_placement',holder_placement,
                'holder_checkout',holder_checkout
            )
            FROM candidates
            WHERE trim(coalesce(holder_checkout,''),?3)<>''
            ORDER BY same_plan DESC, holder_plan_updated_at DESC, holder_plan_id, holder_task_pos, holder_attempt_pos",
            writer = super::super::read_projections::checkout::LIVE_CHECKOUT_WRITER_PREDICATE,
        );
        let rows = {
            let connection = self.connection.lock();
            record_query();
            let mut statement = connection.prepare(&sql).map_err(db_error)?;
            let mapped = statement
                .query_map(
                    params![plan, task, super::super::status_queries::WHITESPACE],
                    |row| column::<String>(row, 0),
                )
                .map_err(db_error)?;
            mapped.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let mut target_primary: Option<Option<String>> = None;
        let mut target_resolved: Option<String> = None;
        for raw in rows {
            let candidate: LaneIntegrationCandidate = decode(plan, &raw)?;
            let primary = target_primary.get_or_insert_with(|| {
                crate::service_plan::shared_checkout::resolved_git_primary_checkout_path(
                    &candidate.worktree_path,
                )
            });
            let expected = target_resolved.get_or_insert_with(|| {
                primary
                    .as_deref()
                    .map(crate::service_plan::shared_checkout::canonical_path)
                    .unwrap_or_else(|| {
                        crate::service_plan::shared_checkout::resolved_checkout_target(
                            &candidate.target_plan_canonical,
                        )
                    })
            });
            let holder_resolved = crate::service_plan::shared_checkout::resolved_checkout_target(
                &candidate.holder_checkout,
            );
            if holder_resolved != *expected {
                continue;
            }
            let canonical_path = primary.clone().unwrap_or(candidate.holder_checkout);
            return Ok(Some(crate::service_plan::LaneIntegrationRead {
                plan_id: candidate.holder_plan_id,
                task_id: candidate.target_task_id,
                worktree_path: candidate.worktree_path,
                canonical_path,
                holder: crate::service_plan::CanonicalWriterHoldRead {
                    task_id: candidate.holder_task_id,
                    attempt_id: candidate.holder_attempt_id,
                    placement: candidate.holder_placement,
                },
            }));
        }
        Ok(None)
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
