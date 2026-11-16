use super::{column, record_query};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::{params, OptionalExtension};

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct TaskLaneReferenceRead {
    pub plan_id: String,
    pub task_id: String,
    pub canonical_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ownership: Option<crate::plan::LaneOwnership>,
}

const COLUMNS: &str = "SELECT p.id,t.task_id,p.project_path,
    coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),
    json_extract(t.row_json,'$.lane.worktree_path'),
    json_extract(t.row_json,'$.lane.branch'),
    json_extract(t.row_json,'$.lane.ownership')
    FROM plans p JOIN plan_tasks t ON t.plan_id=p.id";

pub(super) fn ownership(raw: Option<String>) -> Option<crate::plan::LaneOwnership> {
    // An unknown value is not a recorded Domios lane, so it decodes to unrecorded.
    raw.and_then(|raw| serde_json::from_value(serde_json::Value::String(raw)).ok())
}

fn lane_reference(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskLaneReferenceRead> {
    let project: String = column(row, 2)?;
    let canonical: String = column(row, 3)?;
    Ok(TaskLaneReferenceRead {
        plan_id: column(row, 0)?,
        task_id: column(row, 1)?,
        canonical_path: if canonical.trim().is_empty() {
            project
        } else {
            canonical
        },
        worktree_path: column(row, 4)?,
        branch: column(row, 5)?,
        ownership: ownership(column(row, 6)?),
    })
}

impl PlanDatabase {
    pub(in crate::service_plan) fn recorded_task_lanes_projection(
        &self,
    ) -> Result<Vec<TaskLaneReferenceRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare_cached(&format!(
                "{COLUMNS} WHERE json_extract(t.row_json,'$.lane.worktree_path') IS NOT NULL
             ORDER BY p.updated_at DESC,p.id,t.position"
            ))
            .map_err(db_error)?;
        let rows = statement.query_map([], lane_reference).map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn task_lane_reference_projection(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Option<TaskLaneReferenceRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                &format!("{COLUMNS} WHERE p.id=?1 AND t.task_id=?2"),
                params![plan_id, task_id],
                lane_reference,
            )
            .optional()
            .map_err(db_error)
    }
}

impl PlanStore {
    pub fn recorded_task_lanes_projection(&self) -> Result<Vec<TaskLaneReferenceRead>, PlanError> {
        self.database.recorded_task_lanes_projection()
    }

    pub fn task_lane_reference_projection(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Option<TaskLaneReferenceRead>, PlanError> {
        self.database
            .task_lane_reference_projection(plan_id, task_id)
    }
}
