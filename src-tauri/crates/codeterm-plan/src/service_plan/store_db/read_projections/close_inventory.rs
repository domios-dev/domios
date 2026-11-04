use super::{column, decode, record_query};
use crate::plan::{OrchestratorRef, PlanStatus, PlannerRef};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanCloseRestoreRead {
    pub id: String,
    pub project_path: String,
    pub team_id: Option<String>,
    pub owner: OrchestratorRef,
    pub planner: Option<PlannerRef>,
    pub status: PlanStatus,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn close_restore_projection(
        &self,
        team_ids: &[String],
    ) -> Result<Vec<PlanCloseRestoreRead>, PlanError> {
        if team_ids.is_empty() {
            return Ok(Vec::new());
        }
        let selected = serde_json::to_string(team_ids).map_err(super::super::json_error)?;
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare_cached(
                "SELECT id,json_object('id',id,'project_path',project_path,'team_id',team_id,
                'owner',json_extract(header_json,'$.owner'),
                'planner',json_extract(header_json,'$.planner'),'status',status)
             FROM plans WHERE team_id IN (SELECT value FROM json_each(?1))
             ORDER BY updated_at DESC,id",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([selected], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (id, document) = row.map_err(db_error)?;
            decode(&id, &document)
        })
        .collect()
    }
}

impl PlanStore {
    pub fn close_restore_projection(
        &self,
        team_ids: &[String],
    ) -> Result<Vec<PlanCloseRestoreRead>, PlanError> {
        self.database.close_restore_projection(team_ids)
    }
}
