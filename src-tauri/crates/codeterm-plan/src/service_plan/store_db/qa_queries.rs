use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::ReviewPolicy;
use crate::service_plan::review_gate::{review_gate_status_for, ReviewGateStatus};
use crate::service_plan::PlanError;
use crate::service_plan::{qa::QaQueueTaskRead, review_gate::QaWaveInput};
use rusqlite::{params, OptionalExtension};

#[derive(serde::Deserialize)]
struct ReviewLimits {
    wip_limit: u32,
    credit: i64,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn review_gate_projection(
        &self,
        id: &str,
    ) -> Result<ReviewGateStatus, PlanError> {
        let connection = self.connection.lock();
        let defaults = ReviewPolicy::default();
        record_query();
        let row=connection.query_row(
            "SELECT json_object('wip_limit',CASE WHEN json_type(header_json,'$.review_policy') IS NULL THEN ?2 ELSE json_extract(header_json,'$.review_policy.wip_limit') END,
                'credit',CASE WHEN json_type(header_json,'$.review_policy') IS NULL THEN ?3 ELSE json_extract(header_json,'$.review_policy.credit') END),
                (SELECT COUNT(*) FROM plan_tasks WHERE plan_id=p.id AND status='in_review') FROM plans p WHERE id=?1",
            params![id,defaults.wip_limit,defaults.credit],|row|Ok((column::<String>(row,0)?,row.get::<_,usize>(1)?)),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let limits: ReviewLimits = decode(id, &row.0)?;
        Ok(review_gate_status_for(
            row.1,
            limits.wip_limit,
            limits.credit,
        ))
    }

    pub(in crate::service_plan) fn qa_queue_tasks(
        &self,
        id: &str,
    ) -> Result<Vec<QaQueueTaskRead>, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                [id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        record_query();
        let mut statement=tx.prepare_cached(
            "SELECT json_object('id',json_extract(row_json,'$.id'),'name',json_extract(row_json,'$.name'),'qa_state',json_extract(row_json,'$.qa_state'),
                'status_changed_at',json(CASE WHEN json_type(row_json,'$.status_changed_at') IS NULL THEN '0' ELSE row_json->'$.status_changed_at' END),
                'assigned_tab_id',json_extract(row_json,'$.assigned_tab_id'),
                'qa_plan',CASE WHEN json_type(row_json,'$.qa_plan') IS NULL OR json_type(row_json,'$.qa_plan')='null'
                    THEN json_extract(row_json,'$.test_plan') ELSE json_extract(row_json,'$.qa_plan') END)
             FROM plan_tasks WHERE plan_id=?1 AND status='in_review' AND json_extract(row_json,'$.review_stage')='qa' ORDER BY position"
        ).map_err(db_error)?;
        let tasks = statement
            .query_map([id], |row| column::<String>(row, 0))
            .map_err(db_error)?
            .map(|row| decode(id, &row.map_err(db_error)?))
            .collect::<Result<_, _>>()?;
        drop(statement);
        tx.commit().map_err(db_error)?;
        Ok(tasks)
    }

    pub(in crate::service_plan) fn qa_wave_projection(
        &self,
        id: &str,
    ) -> Result<QaWaveInput, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let document: String=tx.query_row(
            "WITH qa_groups AS MATERIALIZED (
                SELECT DISTINCT json_extract(row_json,'$.group') AS group_key FROM plan_tasks
                WHERE plan_id=?1 AND status='in_review' AND json_extract(row_json,'$.review_stage')='qa'
             ) SELECT json_object('tasks',json('[]'),'task_groups',json((SELECT json_group_array(json(document)) FROM (
                    SELECT json_object('key',json_extract(g.value,'$.key'),'label',json_extract(g.value,'$.label')) AS document
                    FROM json_each(p.header_json,'$.task_groups') g
                    WHERE json_extract(g.value,'$.key') IN (SELECT group_key FROM qa_groups)
                    ORDER BY g.key)))) FROM plans p WHERE id=?1",
            [id],|row|column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let mut plan: QaWaveInput = decode(id, &document)?;
        record_query();
        let mut statement=tx.prepare_cached(
            "SELECT json_object('id',json_extract(row_json,'$.id'),'group',json_extract(row_json,'$.group')) FROM plan_tasks
             WHERE plan_id=?1 AND status='in_review' AND json_extract(row_json,'$.review_stage')='qa'
                AND json_extract(row_json,'$.group') IS NOT NULL ORDER BY position"
        ).map_err(db_error)?;
        for document in statement
            .query_map([id], |row| column::<String>(row, 0))
            .map_err(db_error)?
        {
            plan.tasks.push(decode(id, &document.map_err(db_error)?)?);
        }
        drop(statement);
        tx.commit().map_err(db_error)?;
        Ok(plan)
    }
}
