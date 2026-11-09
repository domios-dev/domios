use super::{column, decode, record_query};
use crate::plan::{Plan, Task, TaskStatus};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LaneCleanupPlanRead {
    pub id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub tasks: Vec<LaneCleanupTaskRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LaneCleanupTaskRead {
    pub id: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_tab_id: Option<String>,
    pub lane: LaneCleanupFieldsRead,
    pub remote: bool,
    pub execution_attempts: Vec<LaneCleanupAttemptRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LaneCleanupFieldsRead {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LaneCleanupAttemptRead {
    pub executor_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

impl From<&Task> for LaneCleanupTaskRead {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id.clone(),
            status: task.status.clone(),
            assigned_tab_id: task.assigned_tab_id.clone(),
            lane: LaneCleanupFieldsRead {
                tab_id: task.lane.tab_id.clone(),
                worktree_path: task.lane.worktree_path.clone(),
                branch: task.lane.branch.clone(),
                base: task.lane.base.clone(),
                destination: task.lane.destination.clone(),
            },
            remote: task
                .execution_attempts
                .iter()
                .any(|a| a.remote_worktree_path.is_some()),
            execution_attempts: task
                .execution_attempts
                .iter()
                .filter(|a| {
                    a.worktree_path.is_some()
                        || a.branch.is_some()
                        || a.base.is_some()
                        || a.destination.is_some()
                })
                .map(|a| LaneCleanupAttemptRead {
                    executor_tab_id: a.executor_tab_id.clone(),
                    worktree_path: a.worktree_path.clone(),
                    branch: a.branch.clone(),
                    base: a.base.clone(),
                    destination: a.destination.clone(),
                })
                .collect(),
        }
    }
}

impl From<&Plan> for LaneCleanupPlanRead {
    fn from(plan: &Plan) -> Self {
        Self {
            id: plan.id.clone(),
            project_path: plan.project_path.clone(),
            canonical_project_path: plan.canonical_project_path.clone(),
            tasks: plan.tasks.iter().map(Into::into).collect(),
        }
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn lane_cleanup_projection(
        &self,
        id: &str,
    ) -> Result<LaneCleanupPlanRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw:String=connection.query_row(
            "SELECT json_object('id',p.id,'project_path',p.project_path,
                'canonical_project_path',coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),
                'tasks',json((SELECT json_group_array(json(task)) FROM (SELECT json_object(
                    'id',t.task_id,'status',t.status,'assigned_tab_id',t.assigned_tab_id,
                    'lane',json_object('tab_id',json_extract(t.row_json,'$.lane.tab_id'),
                        'worktree_path',json_extract(t.row_json,'$.lane.worktree_path'),'branch',json_extract(t.row_json,'$.lane.branch'),
                        'base',json_extract(t.row_json,'$.lane.base'),'destination',json_extract(t.row_json,'$.lane.destination')),
                    'remote',json(CASE WHEN EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                        AND json_extract(a.row_json,'$.remote_worktree_path') IS NOT NULL) THEN 'true' ELSE 'false' END),
                    'execution_attempts',json((SELECT json_group_array(json(attempt)) FROM (SELECT json_object(
                        'executor_tab_id',a.executor_tab_id,'worktree_path',json_extract(a.row_json,'$.worktree_path'),
                        'branch',json_extract(a.row_json,'$.branch'),'base',json_extract(a.row_json,'$.base'),'destination',json_extract(a.row_json,'$.destination')) AS attempt
                        FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND (
                            json_extract(a.row_json,'$.worktree_path') IS NOT NULL OR json_extract(a.row_json,'$.branch') IS NOT NULL
                            OR json_extract(a.row_json,'$.base') IS NOT NULL OR json_extract(a.row_json,'$.destination') IS NOT NULL) ORDER BY a.position)))
                    ) AS task FROM plan_tasks t WHERE t.plan_id=p.id ORDER BY t.position)))) FROM plans p WHERE p.id=?1",
            [id],|row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        decode(id, &raw)
    }
}

impl PlanStore {
    pub fn lane_cleanup_projection(&self, id: &str) -> Result<LaneCleanupPlanRead, PlanError> {
        self.database.lane_cleanup_projection(id)
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ClosedTaskLaneCleanupRead {
    pub id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub task: Option<ClosedTaskLaneRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ClosedTaskLaneRead {
    pub id: String,
    pub status: TaskStatus,
    pub assigned_tab_id: Option<String>,
    pub lane: LaneCleanupFieldsRead,
    pub workspace_id: Option<String>,
    pub workspace_name: Option<String>,
    pub cleanup_status: Option<String>,
    pub executor_tabs: Vec<String>,
}

fn canonical_root<'a>(project: &'a str, canonical: &'a str) -> &'a str {
    if canonical.trim().is_empty() {
        project
    } else {
        canonical
    }
}

impl LaneCleanupPlanRead {
    pub fn canonical_path(&self) -> &str {
        canonical_root(&self.project_path, &self.canonical_project_path)
    }
}

impl ClosedTaskLaneCleanupRead {
    pub fn canonical_path(&self) -> &str {
        canonical_root(&self.project_path, &self.canonical_project_path)
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn closed_task_lane_cleanup_projection(
        &self,
        id: &str,
        task: &str,
    ) -> Result<Option<ClosedTaskLaneCleanupRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw:Option<String>=connection.query_row(
            "SELECT json_object('id',p.id,'project_path',p.project_path,'canonical_project_path',coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),
                'task',json((SELECT json_object('id',t.task_id,'status',t.status,'assigned_tab_id',t.assigned_tab_id,
                    'lane',json_object('tab_id',json_extract(t.row_json,'$.lane.tab_id'),'worktree_path',json_extract(t.row_json,'$.lane.worktree_path'),
                        'branch',json_extract(t.row_json,'$.lane.branch'),'base',json_extract(t.row_json,'$.lane.base'),'destination',json_extract(t.row_json,'$.lane.destination')),
                    'workspace_id',json_extract(t.row_json,'$.lane.workspace_id'),'workspace_name',json_extract(t.row_json,'$.lane.workspace_name'),
                    'cleanup_status',json_extract(t.row_json,'$.lane.cleanup_status'),
                    'executor_tabs',json((SELECT json_group_array(executor) FROM (SELECT json_extract(a.row_json,'$.executor_tab_id') AS executor
                        FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id GROUP BY executor ORDER BY min(a.position)))))
                 FROM plan_tasks t WHERE t.plan_id=p.id AND t.task_id=?2))) FROM plans p WHERE p.id=?1",
            rusqlite::params![id,task],|row|column(row,0),
        ).optional().map_err(db_error)?;
        raw.map(|raw| decode(id, &raw)).transpose()
    }
}

impl PlanStore {
    pub fn closed_task_lane_cleanup_projection(
        &self,
        id: &str,
        task: &str,
    ) -> Result<Option<ClosedTaskLaneCleanupRead>, PlanError> {
        self.database.closed_task_lane_cleanup_projection(id, task)
    }
}
