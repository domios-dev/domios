use super::{column, decode, record_query};
use crate::plan::{TaskLaneMetadata, TaskStatus};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::{params, OptionalExtension};

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LaneCleanupRead {
    pub id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    #[serde(default)]
    pub tasks: Vec<LaneCleanupTaskRead>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LaneCleanupTaskRead {
    pub id: String,
    pub status: TaskStatus,
    pub assigned_tab_id: Option<String>,
    pub lane: TaskLaneMetadata,
    pub remote: bool,
    pub execution_attempts: Vec<LaneCleanupAttemptRead>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LaneCleanupAttemptRead {
    pub executor_tab_id: String,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub destination: Option<String>,
}

impl PlanStore {
    pub fn lane_cleanup_projection(&self, plan: &str) -> Result<LaneCleanupRead, PlanError> {
        self.database.lane_cleanup_projection(plan)
    }
}

impl PlanDatabase {
    fn lane_cleanup_projection(&self, plan: &str) -> Result<LaneCleanupRead, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let raw:String=tx.query_row(
            "SELECT json_object('id',id,'project_path',project_path,'canonical_project_path',coalesce(json_extract(header_json,'$.canonical_project_path'),'')) FROM plans WHERE id=?1",
            [plan],|row|column(row,0)
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let mut read: LaneCleanupRead = decode(plan, &raw)?;
        record_query();
        let mut statement=tx.prepare(
            "SELECT json_object('id',t.task_id,'status',t.status,'assigned_tab_id',json_extract(t.row_json,'$.assigned_tab_id'),
                'lane',json(coalesce(json_extract(t.row_json,'$.lane'),'{}')),
                'remote',json(CASE WHEN EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                    AND json_extract(a.row_json,'$.remote_worktree_path') IS NOT NULL) THEN 'true' ELSE 'false' END),
                'execution_attempts',json((SELECT json_group_array(json(raw)) FROM (
                    SELECT json_object('executor_tab_id',json_extract(a.row_json,'$.executor_tab_id'),
                        'worktree_path',json_extract(a.row_json,'$.worktree_path'),'branch',json_extract(a.row_json,'$.branch'),
                        'base',json_extract(a.row_json,'$.base'),'destination',json_extract(a.row_json,'$.destination')) AS raw
                    FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND (
                        json_extract(a.row_json,'$.worktree_path') IS NOT NULL OR json_extract(a.row_json,'$.branch') IS NOT NULL
                        OR json_extract(a.row_json,'$.base') IS NOT NULL OR json_extract(a.row_json,'$.destination') IS NOT NULL)
                    GROUP BY raw ORDER BY min(a.position)))))
             FROM plan_tasks t WHERE t.plan_id=?1 ORDER BY t.position"
        ).map_err(db_error)?;
        let rows = statement
            .query_map(params![plan], |row| column::<String>(row, 0))
            .map_err(db_error)?;
        read.tasks = rows
            .map(|row| decode(plan, &row.map_err(db_error)?))
            .collect::<Result<_, _>>()?;
        drop(statement);
        tx.commit().map_err(db_error)?;
        Ok(read)
    }
}

#[cfg(test)]
mod tests;
