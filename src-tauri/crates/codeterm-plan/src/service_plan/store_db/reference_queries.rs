use super::read_projections::{column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::plan::{Plan, Section, Task};
use crate::service_plan::{ui_projections::PlanViewProjection, PlanError};
use rusqlite::{params, OptionalExtension};

impl PlanDatabase {
    pub(in crate::service_plan) fn reference_projection(
        &self,
        plan_id: &str,
        task_ref: Option<&str>,
        section_ref: Option<&str>,
    ) -> Result<Plan, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        let value =
            super::ui_projections::projection(&transaction, plan_id, PlanViewProjection::Summary)?;
        let mut plan: Plan =
            serde_json::from_value(value).map_err(|error| PlanError::Io(error.to_string()))?;
        if let Some(reference) = task_ref {
            match super::task_page_queries::resolve_detail_task(&transaction, plan_id, reference) {
                Ok(id) => {
                    record_query();
                    let raw: String = transaction.query_row(
                        "SELECT json_object('id',task_id,'name',json_extract(row_json,'$.name'),'key',json_extract(row_json,'$.key'),'handle',json_extract(row_json,'$.handle')) FROM plan_tasks WHERE plan_id=?1 AND task_id=?2",
                        params![plan_id,id], |row| column(row,0)).map_err(db_error)?;
                    plan.tasks.push(decode::<Task>(plan_id, &raw)?);
                }
                Err(PlanError::TaskNotFound) => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(reference) = section_ref {
            record_query();
            let raw: Option<String> = transaction.query_row(
                &format!("SELECT json_object('id',id,'title',title,'handle',handle) FROM ({SECTION_ROWS}) WHERE id=?2 OR handle=?2 ORDER BY position,traversal LIMIT 1"),
                params![plan_id,reference], |row| column(row,0)).optional().map_err(db_error)?;
            if let Some(raw) = raw {
                plan.sections.push(decode::<Section>(plan_id, &raw)?);
            }
        }
        transaction.commit().map_err(db_error)?;
        Ok(plan)
    }
}
