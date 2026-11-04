use super::{column, decode, record_query};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TeamExecutionRead {
    pub id: String,
    pub team_id: String,
    pub mode: crate::plan::PlanMode,
    pub has_running_tasks: bool,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn team_execution_projection(
        &self,
        teams: &[String],
    ) -> Result<Vec<TeamExecutionRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection.prepare_cached(
            "SELECT p.id,json_object('id',p.id,'team_id',p.team_id,
                'mode',CASE WHEN json_type(p.header_json,'$.mode') IS NULL THEN 'planning' ELSE json_extract(p.header_json,'$.mode') END,
                'has_running_tasks',json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=p.id AND t.status IN ('in_progress','in_review')) THEN 'true' ELSE 'false' END))
             FROM plans p WHERE p.status='active' AND p.team_id IN (SELECT value FROM json_each(?1))
             ORDER BY p.updated_at DESC,p.id"
        ).map_err(db_error)?;
        let teams =
            serde_json::to_string(teams).map_err(|error| PlanError::Io(error.to_string()))?;
        let rows = statement
            .query_map([teams], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (id, raw) = row.map_err(db_error)?;
            decode(&id, &raw)
        })
        .collect()
    }

    pub(in crate::service_plan) fn team_owner_projection(
        &self,
        team: &str,
    ) -> Result<Option<(String, Option<String>)>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row("SELECT id,json_extract(header_json,'$.owner.tab_id') FROM plans WHERE status='active' AND team_id=?1 ORDER BY updated_at DESC,id LIMIT 1", [team],
            |row| Ok((column(row, 0)?,column(row, 1)?))).optional().map_err(db_error)
    }
}

impl PlanStore {
    pub fn team_execution_projection(
        &self,
        teams: &[String],
    ) -> Result<Vec<TeamExecutionRead>, PlanError> {
        self.database.team_execution_projection(teams)
    }

    pub fn team_owner_projection(
        &self,
        team: &str,
    ) -> Result<Option<(String, Option<String>)>, PlanError> {
        if team.trim().is_empty() {
            return Ok(None);
        }
        self.database.team_owner_projection(team.trim())
    }
}
