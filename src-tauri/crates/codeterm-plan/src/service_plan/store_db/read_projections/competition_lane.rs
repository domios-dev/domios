use super::{column, decode, record_query};
use crate::plan::ExecutionIntent;
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore, TaskLaneOwnerRead};
use rusqlite::params;

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CompetitionLaneRead {
    pub id: String,
    pub task_id: String,
    pub canonical_path: String,
    pub dispatch_intent: ExecutionIntent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_tab_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub foreign_lanes: Vec<TaskLaneOwnerRead>,
}

impl PlanStore {
    pub fn competition_lane_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<CompetitionLaneRead, PlanError> {
        self.database.competition_lane_projection(plan, task)
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn competition_lane_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<CompetitionLaneRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw):(bool,Option<String>)=connection.query_row(&format!(
            "{selected} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),(SELECT json_object('id',p.id,'task_id',t.task_id,
                'canonical_path',CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?3)=''
                    THEN p.project_path ELSE json_extract(p.header_json,'$.canonical_project_path') END,
                'dispatch_intent',CASE WHEN json_type(t.row_json,'$.interruption.execution_intent') IS NULL THEN 'unspecified' ELSE json_extract(t.row_json,'$.interruption.execution_intent') END,
                'owner_tab_id',json_extract(p.header_json,'$.owner.tab_id'),
                'foreign_lanes',json((SELECT json_group_array(json_object('task_id',task_id,'worktree_path',path)) FROM (
                    SELECT other.task_id,json_extract(other.row_json,'$.lane.worktree_path') AS path FROM plan_tasks other
                    WHERE other.plan_id=p.id AND other.task_id<>t.task_id AND coalesce(json_extract(other.row_json,'$.handle'),'')<>t.task_id
                        AND json_extract(other.row_json,'$.lane.worktree_path') IS NOT NULL ORDER BY other.position))))
                FROM selected_task t JOIN plans p ON p.id=t.plan_id)",
            selected=super::super::task_authority_queries::SELECTED_TASK_IN_PLAN
        ),params![plan,task,super::super::status_queries::WHITESPACE],|row|Ok((column(row,0)?,column(row,1)?))).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
}

#[cfg(test)]
mod tests;
