use super::{column, record_query};
use crate::service_plan::{PlanError, PlanStore};

impl PlanStore {
    pub fn linked_tab_task_name_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<String, PlanError> {
        self.database.linked_tab_task_name_projection(plan, task)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn linked_tab_task_name_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<String, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists, name): (bool, Option<String>) = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_extract(row_json,'$.name') FROM plan_tasks WHERE plan_id=?1 AND task_id=?2 ORDER BY position LIMIT 1)",
            [plan, task], |row| Ok((column(row,0)?,column(row,1)?)),
        ).map_err(super::super::db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        name.ok_or(PlanError::TaskNotFound)
    }
}
