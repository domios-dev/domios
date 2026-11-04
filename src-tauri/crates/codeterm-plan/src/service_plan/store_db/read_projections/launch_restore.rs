use super::{column, decode, record_query};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TabLaunchTaskRead {
    pub attempt_id: Option<String>,
    pub base: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct LaunchLaneRead {
    pub lane_path: Option<String>,
    pub project_root: String,
}

impl PlanStore {
    pub fn tab_launch_task_projection(
        &self,
        plan: &str,
        task: &str,
        tab: &str,
    ) -> Result<Option<TabLaunchTaskRead>, PlanError> {
        self.database.tab_launch_task_projection(plan, task, tab)
    }
    pub fn launch_lane_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<LaunchLaneRead>, PlanError> {
        self.database.launch_lane_projection(plan, task)
    }
    pub fn continuing_restore_tab_projection(&self, tab: &str) -> Result<bool, PlanError> {
        self.database.continuing_restore_tab_projection(tab)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn tab_launch_task_projection(
        &self,
        plan: &str,
        task: &str,
        tab: &str,
    ) -> Result<Option<TabLaunchTaskRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: Option<String>=connection.query_row(
            "WITH selected_attempt AS (SELECT row_json FROM plan_task_attempts a WHERE a.plan_id=?1 AND a.task_id=?2 AND a.executor_tab_id=?3
                AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL ORDER BY a.position DESC LIMIT 1)
             SELECT json_object('attempt_id',(SELECT json_extract(row_json,'$.id') FROM selected_attempt),
                'base',coalesce((SELECT json_extract(row_json,'$.base') FROM selected_attempt),json_extract(t.row_json,'$.lane.base')))
             FROM plan_tasks t WHERE t.plan_id=?1 AND t.task_id=?2 ORDER BY t.position LIMIT 1",
            rusqlite::params![plan,task,tab],|row|column(row,0),
        ).optional().map_err(super::super::db_error)?;
        encoded.map(|raw| decode(plan, &raw)).transpose()
    }
    pub(in crate::service_plan) fn launch_lane_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<LaunchLaneRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(
            "SELECT json_extract(t.row_json,'$.lane.worktree_path'),CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?3)='' THEN p.project_path ELSE json_extract(p.header_json,'$.canonical_project_path') END FROM plans p JOIN plan_tasks t ON t.plan_id=p.id WHERE p.id=?1 AND t.task_id=?2 ORDER BY t.position LIMIT 1",
            rusqlite::params![plan,task,super::super::status_queries::WHITESPACE],|row|Ok(LaunchLaneRead{lane_path:column(row,0)?,project_root:column(row,1)?}),
        ).optional().map_err(super::super::db_error)
    }
    pub(in crate::service_plan) fn continuing_restore_tab_projection(
        &self,
        tab: &str,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM plans p WHERE p.status='active'
                AND coalesce(json_extract(p.header_json,'$.mode'),'planning') NOT IN ('paused','complete')
                AND EXISTS(SELECT 1 FROM plan_tasks t JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                    WHERE t.plan_id=p.id AND t.status='in_progress' AND a.open=1)
                AND (p.owner_tab_id=?1 OR (json_extract(p.header_json,'$.planner.kind')='tab' AND json_extract(p.header_json,'$.planner.tab_id')=?1)
                    OR EXISTS(SELECT 1 FROM plan_tasks t JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                        WHERE t.plan_id=p.id AND t.status='in_progress' AND a.open=1 AND a.executor_tab_id=?1 AND json_extract(a.row_json,'$.mesh_peer') IS NULL)))",
            [tab],|row|column(row,0),
        ).map_err(super::super::db_error)
    }
}
