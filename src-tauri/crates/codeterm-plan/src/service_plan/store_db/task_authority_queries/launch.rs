use super::*;
use crate::service_plan::{TaskDispatchSeedRead, TaskLaunchRead};

impl PlanDatabase {
    pub(in crate::service_plan) fn task_launch_projection(
        &self,
        plan: &str,
        task: &str,
        source_attempt: Option<&str>,
    ) -> Result<TaskLaunchRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw): (bool,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('task_id',t.task_id,'name',json_extract(t.row_json,'$.name'),
                    'status',json_extract(t.row_json,'$.status'),
                    'lane',json(CASE WHEN json_type(t.row_json,'$.lane') IS NULL THEN '{{}}' ELSE t.row_json->'$.lane' END),
                    'execution_intent',CASE WHEN json_type(t.row_json,'$.execution_intent') IS NULL THEN 'repository_mutation' ELSE json_extract(t.row_json,'$.execution_intent') END,
                    'dispatch_intent',CASE WHEN json_type(t.row_json,'$.interruption.execution_intent') IS NULL THEN 'unspecified' ELSE json_extract(t.row_json,'$.interruption.execution_intent') END,
                    'source_attempt_exists',json(CASE WHEN EXISTS(SELECT 1 FROM plan_task_attempts a
                        WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')=?3)
                        THEN 'true' ELSE 'false' END)) FROM selected_task t)"),
            params![plan,task,source_attempt], |row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }

    pub(in crate::service_plan) fn task_dispatch_seed_projection(
        &self,
        plan: &str,
        task: &str,
        new_tab: &str,
    ) -> Result<TaskDispatchSeedRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw): (bool,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('acceptance_criteria',json(CASE WHEN json_type(t.row_json,'$.acceptance_criteria') IS NULL
                        THEN '[]' ELSE t.row_json->'$.acceptance_criteria' END),
                    'predecessor_tabs',json((SELECT json_group_array(tab) FROM (
                        SELECT CASE WHEN json_type(a.row_json,'$.executor_tab_id')='text' THEN trim(json_extract(a.row_json,'$.executor_tab_id'),?4) ELSE json('invalid predecessor executor') END AS tab
                        FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND (json_extract(a.row_json,'$.closed_at') IS NOT NULL OR json_extract(a.row_json,'$.retired_at') IS NOT NULL)
                            AND json_extract(a.row_json,'$.competition_group_id') IS NULL AND json_extract(a.row_json,'$.mesh_peer') IS NULL
                        GROUP BY tab HAVING tab<>'' AND tab<>?3 ORDER BY max(a.position) DESC))))
                 FROM selected_task t)"),
            params![plan,task,new_tab,super::super::status_queries::WHITESPACE],
            |row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
}
