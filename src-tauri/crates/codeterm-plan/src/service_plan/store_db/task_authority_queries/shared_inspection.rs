use super::super::document_queries::regression::unsigned_timestamp;
use super::*;
use crate::service_plan::task_authority_projection::{SharedDispatchRead, SharedReportRead};

impl PlanDatabase {
    pub(in crate::service_plan) fn dispatch_checkout_projection(
        &self,
        plan: &str,
    ) -> Result<String, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(
            "SELECT CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''),?2)=''
                THEN json_extract(header_json,'$.project_path') ELSE json_extract(header_json,'$.canonical_project_path') END
             FROM plans WHERE id=?1",
            params![plan,super::super::status_queries::WHITESPACE],
            |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)
    }

    pub(in crate::service_plan) fn shared_dispatch_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<SharedDispatchRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw): (bool,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('task_id',t.task_id,'checkout',CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?3)=''
                    THEN json_extract(p.header_json,'$.project_path') ELSE json_extract(p.header_json,'$.canonical_project_path') END,
                    'files',json(coalesce(json_extract(t.row_json,'$.files'),'{{}}')))
                 FROM selected_task t JOIN plans p ON p.id=t.plan_id)"),
            params![plan,task,super::super::status_queries::WHITESPACE],|row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }

    pub(in crate::service_plan) fn shared_report_projection(
        &self,
        plan: &str,
        task: &str,
        reporter: &str,
    ) -> Result<Option<SharedReportRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let owner = unsigned_timestamp("t.row_json", "interruption.owner_epoch", true);
        let attempt_epoch = unsigned_timestamp("a.row_json", "interruption.owner_epoch", true);
        let (exists,task_exists,raw): (bool,bool,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN}, task_owners AS MATERIALIZED (
                SELECT t.plan_id,t.task_id,{owner} AS owner_epoch FROM selected_task t
                WHERE t.assigned_tab_id=?3 AND EXISTS(SELECT 1 FROM plan_task_attempts a
                    WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.executor_tab_id')=?3
                        AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL)
             ), chosen AS (
                SELECT a.plan_id,a.task_id,a.position FROM task_owners t JOIN plan_task_attempts a
                    ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                WHERE json_extract(a.row_json,'$.executor_tab_id')=?3
                    AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL
                    AND (t.owner_epoch='0' OR {attempt_epoch}=t.owner_epoch)
                ORDER BY a.position DESC LIMIT 1)
             SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),EXISTS(SELECT 1 FROM selected_task),
                (SELECT json_object('attempt_id',json_extract(a.row_json,'$.id'),'executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'),
                    'checkout',json_extract(a.row_json,'$.checkout_path'),'start_sha',json_extract(a.row_json,'$.shared_start_sha'),
                    'files',json(coalesce(json_extract(t.row_json,'$.files'),'{{}}')))
                 FROM chosen c JOIN plan_task_attempts a ON a.plan_id=c.plan_id AND a.task_id=c.task_id AND a.position=c.position
                    JOIN selected_task t ON t.plan_id=c.plan_id AND t.task_id=c.task_id
                 WHERE json_extract(t.row_json,'$.execution_intent') IS NOT 'non_repository'
                    AND CASE WHEN json_type(a.row_json,'$.placement') IS NULL THEN 0
                    WHEN json_extract(a.row_json,'$.placement') IN ('isolated_lane','shared','canonical_writer','read_only','non_repository')
                        THEN json_extract(a.row_json,'$.placement')='shared' ELSE json('invalid worker placement') END)"),
            params![plan,task,reporter],|row|Ok((column(row,0)?,column(row,1)?,column(row,2)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        if !task_exists {
            return Err(PlanError::TaskNotFound);
        }
        raw.map(|raw| decode(plan, &raw)).transpose()
    }
}
