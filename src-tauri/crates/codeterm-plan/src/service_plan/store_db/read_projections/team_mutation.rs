use super::{column, decode, record_query};
use crate::plan::{OrchestratorRef, PlanOrphan, PlanStatus, PlannerRef, TransferRecord};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamPlanMutationRead {
    pub id: String,
    pub title: String,
    pub owner: OrchestratorRef,
    pub status: PlanStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner: Option<PlannerRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orphaned: Option<PlanOrphan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_owner_transfer: Option<TransferRecord>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn team_plan_mutation_projection(
        &self,
        id: &str,
    ) -> Result<TeamPlanMutationRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: String = connection.query_row(
            "SELECT json_object('id',id,'title',title,'owner',json_extract(header_json,'$.owner'),
                'status',status,'team_id',team_id,'planner',json_extract(header_json,'$.planner'),
                'orphaned',json_extract(header_json,'$.orphaned'),
                'latest_owner_transfer',CASE
                    WHEN json_type(header_json,'$.owner_history') IS NULL THEN NULL
                    WHEN json_type(header_json,'$.owner_history')='array' AND json_array_length(header_json,'$.owner_history')=0 THEN NULL
                    WHEN json_type(header_json,'$.owner_history')='array'
                        AND json_type(header_json,'$.owner_history[#-1]')='object'
                        THEN json_extract(header_json,'$.owner_history[#-1]')
                    ELSE json('false') END)
             FROM plans WHERE id=?1",
            [id], |row| column(row, 0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        decode(id, &raw)
    }
}

impl PlanStore {
    pub fn team_plan_mutation_projection(
        &self,
        id: &str,
    ) -> Result<TeamPlanMutationRead, PlanError> {
        self.database.team_plan_mutation_projection(id)
    }
}
