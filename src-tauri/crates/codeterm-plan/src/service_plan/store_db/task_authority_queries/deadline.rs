use super::super::document_queries::regression::unsigned_timestamp;
use super::*;
use crate::plan::TaskLaneMetadata;

#[derive(serde::Deserialize)]
struct StartupEpochRead {
    plan_id: String,
    assigned_tab: Option<String>,
    owner_tab: Option<String>,
    owner_epoch: u64,
    attempt_epoch: Option<u64>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn startup_owner_epoch_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
        executor: &str,
    ) -> Result<Option<u64>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let owner = unsigned_timestamp("t.row_json", "interruption.owner_epoch", true);
        let attempt_epoch = unsigned_timestamp("a.row_json", "interruption.owner_epoch", true);
        let raw: Option<String> = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT (SELECT json_object('plan_id',t.plan_id,'assigned_tab',t.assigned_tab_id,
                'owner_tab',json_extract(t.row_json,'$.interruption.owner_tab_id'),'owner_epoch',json({owner}),
                'attempt_epoch',json((SELECT {attempt_epoch} FROM plan_task_attempts a
                    WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')=?2
                        AND json_extract(a.row_json,'$.executor_tab_id')=?3
                        AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL
                    ORDER BY a.position LIMIT 1))) FROM selected_task t)"),
            params![task,attempt,executor], |row|column(row,0),
        ).map_err(db_error)?;
        let Some(read) = raw
            .map(|raw| decode::<StartupEpochRead>(plan, &raw))
            .transpose()?
        else {
            return Ok(None);
        };
        Ok((read.plan_id == plan
            && read.assigned_tab.as_deref() == Some(executor)
            && read.owner_tab.as_deref() == Some(executor)
            && read.owner_epoch > 0
            && read.attempt_epoch == Some(read.owner_epoch))
        .then_some(read.owner_epoch))
    }

    pub(in crate::service_plan) fn lifecycle_provider_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String> = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT (SELECT json_quote(coalesce(json_extract(a.row_json,'$.agent_type'),json_extract(a.row_json,'$.provider')))
                FROM selected_task t JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                WHERE json_extract(a.row_json,'$.id')=?3 ORDER BY a.position LIMIT 1)"),
            params![plan,task,attempt], |row|column(row,0),
        ).map_err(db_error)?;
        raw.map(|raw| decode::<Option<String>>(plan, &raw))
            .transpose()
            .map(Option::flatten)
    }

    pub(in crate::service_plan) fn task_lane_metadata_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<TaskLaneMetadata, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw): (bool,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT CASE WHEN json_type(t.row_json,'$.lane') IS NULL THEN '{{}}' ELSE t.row_json->'$.lane' END FROM selected_task t)"),
            params![plan,task], |row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
}
