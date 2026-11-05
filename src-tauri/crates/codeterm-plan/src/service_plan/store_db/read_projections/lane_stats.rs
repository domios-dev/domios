use super::{column, record_query};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct LaneWatchRead {
    pub worktree_path: String,
    pub canonical_path: String,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct LaneAttemptRead {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub worktree_path: String,
    pub canonical_path: String,
    pub turn: u32,
}

pub(super) fn canonical_path(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<String> {
    let project: String = column(row, index)?;
    let canonical: String = column(row, index + 1)?;
    Ok(if canonical.trim().is_empty() {
        project
    } else {
        canonical
    })
}

impl PlanDatabase {
    pub(in crate::service_plan) fn lane_watch_projection(
        &self,
    ) -> Result<Vec<LaneWatchRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare_cached(
                "SELECT json_extract(t.row_json,'$.lane.worktree_path'),p.project_path,
                coalesce(json_extract(p.header_json,'$.canonical_project_path'),'')
             FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
             WHERE t.status IN ('pending','paused','in_progress','in_review','blocked')
                AND json_extract(t.row_json,'$.lane.worktree_path') IS NOT NULL
                AND json_extract(t.row_json,'$.lane.cleanup_status') IS NULL
             ORDER BY p.updated_at DESC,p.id,t.position",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(LaneWatchRead {
                    worktree_path: column(row, 0)?,
                    canonical_path: canonical_path(row, 1)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn lane_attempt_projection(
        &self,
        tab: &str,
    ) -> Result<Option<LaneAttemptRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(
            "WITH chosen AS MATERIALIZED (
                SELECT p.id AS plan_id,t.task_id,a.position FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
                    JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                WHERE a.executor_tab_id=?1 AND json_extract(a.row_json,'$.closed_at') IS NULL
                    AND coalesce(json_extract(a.row_json,'$.placement'),'isolated_lane')='isolated_lane'
                    AND json_extract(a.row_json,'$.worktree_path') IS NOT NULL
                ORDER BY p.updated_at DESC,p.id,t.position,a.position DESC LIMIT 1
             ) SELECT p.id,c.task_id,a.attempt_id,json_extract(a.row_json,'$.worktree_path'),
                p.project_path,coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),
                coalesce((SELECT max(json_extract(s.value,'$.turn')) FROM json_each(a.row_json,'$.lane_turn_stats') s),0)
             FROM chosen c JOIN plans p ON p.id=c.plan_id JOIN plan_task_attempts a
                ON a.plan_id=c.plan_id AND a.task_id=c.task_id AND a.position=c.position", [tab],
            |row| Ok(LaneAttemptRead {
                plan_id: column(row, 0)?,task_id: column(row, 1)?,attempt_id: column(row, 2)?,
                worktree_path: column(row, 3)?,canonical_path: canonical_path(row, 4)?,turn: column(row, 6)?,
            }),
        ).optional().map_err(db_error)
    }
}

impl PlanStore {
    pub fn lane_watch_projection(&self) -> Result<Vec<LaneWatchRead>, PlanError> {
        self.database.lane_watch_projection()
    }

    pub fn lane_attempt_projection(&self, tab: &str) -> Result<Option<LaneAttemptRead>, PlanError> {
        self.database.lane_attempt_projection(tab)
    }
}
