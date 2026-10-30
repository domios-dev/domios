//! Startup sweeps select their candidate plans in SQL, so restore never walks
//! or locks plans the sweep leaves alone.

use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;

/// Active plan header row: id and Team binding.
pub(in crate::service_plan) struct ActivePlanRow {
    pub id: String,
    pub team_id: Option<String>,
}

/// Every char `str::trim` strips (Unicode White_Space), so `trim(value, ?1)`
/// in SQL blanks exactly the values the Rust lane rule treats as absent.
const RUST_TRIM_WHITESPACE: &str = "\u{9}\u{a}\u{b}\u{c}\u{d}\u{20}\u{85}\u{a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}";

// The lane settlement rule (`LaneRecord::needs_settlement`) with `?1` bound to
// RUST_TRIM_WHITESPACE; callers still re-check each candidate under its plan lock.
const UNSETTLED_LANE: &str = "
    json_extract(task, '$.status') IN ('done', 'cancelled')
    AND trim(coalesce(json_extract(task, '$.lane.cleanup_status'), ''), ?1) = ''
    AND (trim(coalesce(json_extract(task, '$.lane.worktree_path'), ''), ?1) <> ''
         OR trim(coalesce(json_extract(task, '$.lane.branch'), ''), ?1) <> '')";

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

    /// Plans holding a finished task whose lane may still need settling, from
    /// task rows and from legacy plans that keep their tasks inline.
    pub(in crate::service_plan) fn plans_with_unsettled_lanes(
        &self,
    ) -> Result<Vec<String>, PlanError> {
        let sql = format!(
            "SELECT plan_id FROM (SELECT plan_id, document_json AS task FROM plan_tasks) WHERE {UNSETTLED_LANE}
             UNION
             SELECT id FROM (
                 SELECT plans.id AS id, inline.value AS task
                 FROM plans, json_each(plans.document_json, '$.tasks') AS inline
                 WHERE json_type(plans.document_json, '$.tasks') = 'array'
             ) WHERE {UNSETTLED_LANE}"
        );
        let connection = self.connection.lock();
        let mut statement = connection.prepare(&sql).map_err(db_error)?;
        let rows = statement
            .query_map([RUST_TRIM_WHITESPACE], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }
}

#[cfg(test)]
mod tests;
