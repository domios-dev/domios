use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::plan_match::{self, PlanMatch};
use crate::service_plan::task_queries::TaskListPlan;
use crate::service_plan::PlanError;

const LIST_TASK: &str = "json_object(
    'id',task_id,'name',json_extract(row_json,'$.name'),'type',json_extract(row_json,'$.type'),
    'status',status,'assigned_tab_id',assigned_tab_id,
    'key',json_extract(row_json,'$.key'),'handle',json_extract(row_json,'$.handle'),
    'parent_task_id',json_extract(row_json,'$.parent_task_id'),
    'section_ids',json(coalesce(json_extract(row_json,'$.section_ids'),'[]')),
    'external_refs',json(coalesce(json_extract(row_json,'$.external_refs'),'[]')))";

impl PlanDatabase {
    pub(in crate::service_plan) fn task_list_snapshot(
        &self,
        predicate: &PlanMatch,
    ) -> Result<Vec<TaskListPlan>, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let (clause, params) = plan_match::compile(predicate);
        record_query();
        let mut statement = transaction
            .prepare_cached(&format!(
                "SELECT p.id,json_object('id',p.id,'title',p.title,'status',p.status,
                'project_path',p.project_path,'owner',json_extract(p.header_json,'$.owner'))
             FROM plans p WHERE {clause} ORDER BY p.updated_at DESC,p.id"
            ))
            .map_err(db_error)?;
        let headers = statement
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        drop(statement);
        let mut plans = Vec::new();
        for (id, header) in headers {
            let mut plan: TaskListPlan = decode(&id, &header)?;
            record_query();
            let mut tasks = transaction
                .prepare_cached(&format!(
                    "SELECT {LIST_TASK} FROM plan_tasks WHERE plan_id=?1 ORDER BY position"
                ))
                .map_err(db_error)?;
            for document in tasks
                .query_map([&id], |row| column::<String>(row, 0))
                .map_err(db_error)?
            {
                plan.tasks.push(decode(&id, &document.map_err(db_error)?)?);
            }
            record_query();
            let mut sections = transaction.prepare_cached(
                "SELECT json_object('id',json_extract(j.value,'$.id'),'title',json_extract(j.value,'$.title'))
                 FROM plan_sections s,json_tree(s.row_json) j WHERE s.plan_id=?1
                   AND j.type='object' AND (j.fullkey='$' OR j.path LIKE '%.subsections')
                   AND json_type(j.value,'$.id')='text' ORDER BY s.position,j.id"
            ).map_err(db_error)?;
            for document in sections
                .query_map([&id], |row| column::<String>(row, 0))
                .map_err(db_error)?
            {
                plan.sections
                    .push(decode(&id, &document.map_err(db_error)?)?);
            }
            plans.push(plan);
        }
        transaction.commit().map_err(db_error)?;
        Ok(plans)
    }
}
