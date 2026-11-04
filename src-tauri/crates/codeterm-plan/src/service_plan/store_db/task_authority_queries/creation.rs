use super::*;
use crate::service_plan::{TaskCreationRead, TaskDescriptionRead};

impl PlanDatabase {
    pub(in crate::service_plan) fn task_description_projection(
        &self,
        task_ref: &str,
    ) -> Result<TaskDescriptionRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: String = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT json_object('name',json_extract(row_json,'$.name'),'assigned_tab_id',assigned_tab_id) FROM selected_task"),
            [task_ref], |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        decode("task tab description", &raw)
    }

    pub(in crate::service_plan) fn task_creation_projection(
        &self,
        plan_id: &str,
        task_ref: &str,
    ) -> Result<Option<TaskCreationRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String> = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT json_object(
                'key',coalesce(json_extract(row_json,'$.key'),upper(CASE WHEN substr(task_id,1,5)='task_' THEN substr(task_id,6) ELSE task_id END)),
                'section_ids',json(coalesce(json_extract(row_json,'$.section_ids'),'[]')))
             FROM selected_task t JOIN plans p ON p.id=t.plan_id"),
            params![plan_id,task_ref], |row| column(row,0),
        ).optional().map_err(db_error)?;
        raw.map(|raw| decode(plan_id, &raw)).transpose()
    }
}
