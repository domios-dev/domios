use super::{column, decode, record_query};
use crate::plan::OrchestratorRef;
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RuntimePlanIdentityRead {
    pub id: String,
    pub project_path: String,
    pub owner: OrchestratorRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
}

impl PlanStore {
    pub fn runtime_plan_identity_projection(
        &self,
        plan: &str,
    ) -> Result<RuntimePlanIdentityRead, PlanError> {
        self.database.runtime_plan_identity_projection(plan)
    }

    pub fn runtime_bound_plan_projection(
        &self,
        tab: &str,
        team: Option<&str>,
    ) -> Result<Option<String>, PlanError> {
        self.database.runtime_bound_plan_projection(tab, team)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn runtime_plan_identity_projection(
        &self,
        plan: &str,
    ) -> Result<RuntimePlanIdentityRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection.query_row(
            "SELECT json_object('id',id,'project_path',project_path,'team_id',team_id,'owner',json_extract(header_json,'$.owner')) FROM plans WHERE id=?1",
            [plan], |row| column(row,0),
        ).optional().map_err(super::super::db_error)?.ok_or(PlanError::NotFound)?;
        decode(plan, &encoded)
    }

    pub(in crate::service_plan) fn runtime_bound_plan_projection(
        &self,
        tab: &str,
        team: Option<&str>,
    ) -> Result<Option<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(
            "SELECT id FROM plans WHERE owner_tab_id=?1 OR (json_extract(header_json,'$.planner.kind')='tab'
                AND json_extract(header_json,'$.planner.tab_id')=?1) OR (?2 IS NOT NULL AND team_id=?2)
                ORDER BY updated_at DESC,id LIMIT 1",
            rusqlite::params![tab,team], |row| column(row,0),
        ).optional().map_err(super::super::db_error)
    }
}
