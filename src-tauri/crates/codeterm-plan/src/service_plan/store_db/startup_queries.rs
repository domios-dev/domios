//! Startup sweeps select their candidate plans in SQL, so restore never walks
//! or locks plans the sweep leaves alone.

use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;

/// Active plan header row: id and Team binding.
pub(in crate::service_plan) struct ActivePlanRow {
    pub id: String,
    pub team_id: Option<String>,
}

// Looser than the Rust lane predicate (any whitespace trims to empty there);
// callers re-check each candidate under its plan lock.
const UNSETTLED_LANE: &str = "
    json_extract(task, '$.status') IN ('done', 'cancelled')
    AND trim(coalesce(json_extract(task, '$.lane.cleanup_status'), ''), ' ' || char(9, 10, 11, 12, 13)) = ''
    AND (json_extract(task, '$.lane.worktree_path') IS NOT NULL
         OR json_extract(task, '$.lane.branch') IS NOT NULL)";

impl PlanDatabase {
    pub(in crate::service_plan) fn active_plan_rows(
        &self,
    ) -> Result<Vec<ActivePlanRow>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached("SELECT id, team_id FROM plans WHERE status = 'active'")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(ActivePlanRow {
                    id: row.get(0)?,
                    team_id: row.get(1)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// Plans holding a finished task whose lane may still need settling.
    pub(in crate::service_plan) fn plans_with_unsettled_lanes(
        &self,
    ) -> Result<Vec<String>, PlanError> {
        let sql = format!(
            "SELECT DISTINCT plan_id FROM (SELECT plan_id, row_json AS task FROM plan_tasks) WHERE {UNSETTLED_LANE}"
        );
        let connection = self.connection.lock();
        let mut statement = connection.prepare(&sql).map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }
}
