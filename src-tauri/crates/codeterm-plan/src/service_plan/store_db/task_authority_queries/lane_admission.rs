use super::*;
use crate::service_plan::TaskLaneAdmissionRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_lane_admission_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<TaskLaneAdmissionRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw): (bool,Option<String>) = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),(SELECT json_object('plan_id',p.id,'task_id',t.task_id,
                'execution_intent',CASE WHEN json_type(t.row_json,'$.execution_intent') IS NULL THEN 'repository_mutation' ELSE json_extract(t.row_json,'$.execution_intent') END,
                'canonical_path',CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?3)=''
                    THEN json_extract(p.header_json,'$.project_path') ELSE json_extract(p.header_json,'$.canonical_project_path') END,
                'foreign_lanes',json(CASE WHEN CASE WHEN json_type(t.row_json,'$.execution_intent') IS NULL THEN 'repository_mutation' ELSE json_extract(t.row_json,'$.execution_intent') END='repository_mutation'
                    THEN (SELECT json_group_array(json_object('task_id',l.task_id,'worktree_path',json_extract(l.row_json,'$.lane.worktree_path')))
                        FROM (SELECT other.task_id,other.row_json FROM plan_tasks other WHERE other.plan_id=p.id
                            AND other.task_id<>t.task_id AND coalesce(json_extract(other.row_json,'$.handle'),'')<>t.task_id
                            AND json_extract(other.row_json,'$.lane.worktree_path') IS NOT NULL ORDER BY other.position) l)
                    ELSE '[]' END))
             FROM plans p JOIN plan_tasks t ON t.plan_id=p.id WHERE p.id=?1 AND t.task_id=?2)",
            params![plan,task,super::super::status_queries::WHITESPACE],|row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
}
