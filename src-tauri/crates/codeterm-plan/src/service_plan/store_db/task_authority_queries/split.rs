use super::*;
use crate::service_plan::TaskSplitDefaultsRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_split_defaults_projection(
        &self,
        task_ref: &str,
        include_body: bool,
    ) -> Result<TaskSplitDefaultsRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: String = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT json_object(
                'parent_task_id',json_extract(row_json,'$.parent_task_id'),'status',json_extract(row_json,'$.status'),
                'kind',json_extract(row_json,'$.type'),
                'body',CASE WHEN ?2 AND json_extract(row_json,'$.parent_task_id') IS NULL
                    AND json_extract(row_json,'$.status') NOT IN ('done','cancelled') THEN coalesce(json_extract(row_json,'$.body'),'') END,
                'acceptance_criteria',json(coalesce(json_extract(row_json,'$.acceptance_criteria'),'[]')),
                'section_ids',json(coalesce(json_extract(row_json,'$.section_ids'),'[]')),
                'test_plan',json_extract(row_json,'$.test_plan'),'qa_plan',json_extract(row_json,'$.qa_plan'),
                'correctness',json_extract(row_json,'$.correctness'),'executor_role',json_extract(row_json,'$.executor_role'),
                'personality',json_extract(row_json,'$.personality'),'role_guidance',json_extract(row_json,'$.role_guidance'),
                'model',json_extract(row_json,'$.model'),'model_rationale',json_extract(row_json,'$.model_rationale'),
                'intent',json_extract(row_json,'$.intent')) FROM selected_task"),
            params![task_ref,include_body],|row|column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        decode("task split defaults", &raw)
    }
}
