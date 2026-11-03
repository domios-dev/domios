use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{Milestone, Plan, Task};
use crate::service_plan::PlanError;
use rusqlite::OptionalExtension;

impl PlanDatabase {
    pub(in crate::service_plan) fn curator_schedule_projection(
        &self,
        plan_id: &str,
    ) -> Result<Plan, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        record_query();
        // The complete control loop is the scheduler's compare-and-set value.
        let header: String = transaction
            .query_row(
                "SELECT json_object('id',id,'title',title,'project_path',project_path,
                'created_at',0,'updated_at',updated_at,'status',status,
                'team_id',team_id,'mode',json_extract(header_json,'$.mode'),
                'owner',json_extract(header_json,'$.owner'),
                'task_groups',json_extract(header_json,'$.task_groups'),
                'control_loop',json_extract(header_json,'$.control_loop'))
             FROM plans WHERE id=?1",
                [plan_id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        let mut plan: Plan = decode(plan_id, &header)?;
        {
            record_query();
            let mut statement = transaction
                .prepare_cached(
                    "SELECT json_object('id',task_id,'name','','type','',
                    'status',status,'group',json_extract(row_json,'$.group'),
                    'status_changed_at',coalesce(json_extract(row_json,'$.status_changed_at'),0))
                 FROM plan_tasks WHERE plan_id=?1 ORDER BY position",
                )
                .map_err(db_error)?;
            for row in statement
                .query_map([plan_id], |row| column::<String>(row, 0))
                .map_err(db_error)?
            {
                plan.tasks
                    .push(decode::<Task>(plan_id, &row.map_err(db_error)?)?);
            }
        }
        record_query();
        let closed: Option<String> = transaction
            .query_row(
                "SELECT json_remove(row_json,'$.text','$.attachments','$.canvas')
             FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='group_close'
             ORDER BY json_extract(row_json,'$.ts') DESC,seq DESC LIMIT 1",
                [plan_id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(closed) = closed {
            plan.strides.push(decode::<Milestone>(plan_id, &closed)?);
        }
        transaction.commit().map_err(db_error)?;
        Ok(plan)
    }
}
