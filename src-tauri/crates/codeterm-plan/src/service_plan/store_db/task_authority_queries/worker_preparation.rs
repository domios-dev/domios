use super::*;
use crate::service_plan::task_queries::TaskSection;
use crate::service_plan::{TaskBriefStrideRead, WorkerTaskPreparationRead};

impl PlanDatabase {
    pub(in crate::service_plan) fn worker_task_preparation_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<WorkerTaskPreparationRead, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let (exists,raw):(bool,Option<String>)=tx.query_row(&format!(
            "{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('task_id',t.task_id,'personality',json_extract(t.row_json,'$.personality'),
                    'speed',json_extract(t.row_json,'$.speed'),'lane_base',json_extract(t.row_json,'$.lane.base'),
                    'brief',json_object('name',json_extract(t.row_json,'$.name'),'parent_task_id',json_extract(t.row_json,'$.parent_task_id'),
                        'body',CASE WHEN json_type(t.row_json,'$.body') IS NULL THEN '' ELSE json_extract(t.row_json,'$.body') END,
                        'intent',json_extract(t.row_json,'$.intent'),'correctness',json_extract(t.row_json,'$.correctness'),
                        'test_plan',json_extract(t.row_json,'$.test_plan'),'role_guidance',json_extract(t.row_json,'$.role_guidance'),
                        'executor_role',json_extract(t.row_json,'$.executor_role'),
                        'acceptance_criteria',json(CASE WHEN json_type(t.row_json,'$.acceptance_criteria') IS NULL THEN '[]' ELSE t.row_json->'$.acceptance_criteria' END)))
                 FROM selected_task t JOIN plans p ON p.id=t.plan_id WHERE p.status='active'
                    AND CASE WHEN json_type(t.row_json,'$.section_ids') IS NULL THEN 1
                        WHEN json_type(t.row_json,'$.section_ids')='array' AND NOT EXISTS(SELECT 1 FROM json_each(t.row_json,'$.section_ids') WHERE type<>'text') THEN 1
                        ELSE json('invalid task section references') END)"),
            params![plan,task],|row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        let mut read: WorkerTaskPreparationRead =
            decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)?;
        record_query();
        let mut statement=tx.prepare(
            "WITH RECURSIVE sections(path,node) AS (
                SELECT printf('%020d',position),row_json FROM plan_sections WHERE plan_id=?1
                UNION ALL SELECT sections.path||'.'||printf('%020d',child.key),child.value FROM sections,json_each(sections.node,'$.subsections') child)
             SELECT json_extract(node,'$.id'),json_extract(node,'$.title') FROM sections
             WHERE CASE WHEN json_type(node)='object' THEN json_extract(node,'$.id') IN
                (SELECT value FROM plan_tasks t,json_each(t.row_json,'$.section_ids') WHERE t.plan_id=?1 AND t.task_id=?2)
                ELSE json('invalid section metadata') END ORDER BY path"
        ).map_err(db_error)?;
        let rows = statement
            .query_map(params![plan, read.task_id], |row| {
                Ok(TaskSection {
                    id: column(row, 0)?,
                    title: column(row, 1)?,
                })
            })
            .map_err(db_error)?;
        read.brief.sections = rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
        drop(statement);
        record_query();
        let mut statement=tx.prepare("SELECT json_object('kind',json_extract(row_json,'$.kind'),'text',CASE WHEN json_type(row_json,'$.text') IS NULL THEN '' ELSE json_extract(row_json,'$.text') END)
            FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 ORDER BY seq").map_err(db_error)?;
        let rows = statement
            .query_map(params![plan, read.task_id], |row| column::<String>(row, 0))
            .map_err(db_error)?;
        read.brief.strides = rows
            .map(|row| decode::<TaskBriefStrideRead>(plan, &row.map_err(db_error)?))
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        tx.commit().map_err(db_error)?;
        Ok(read)
    }
}
