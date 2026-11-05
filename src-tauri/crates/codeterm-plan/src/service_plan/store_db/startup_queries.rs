//! Startup sweeps select their candidate plans in SQL, so restore never walks
//! or locks plans the sweep leaves alone.

use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;

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
    /// Plans holding a finished task whose lane may still need settling.
    pub(in crate::service_plan) fn plans_with_unsettled_lanes(
        &self,
    ) -> Result<Vec<String>, PlanError> {
        let sql = format!(
            "SELECT DISTINCT selected.plan_id FROM (SELECT plan_id, row_json AS task FROM plan_tasks) selected WHERE {UNSETTLED_LANE} AND EXISTS(SELECT 1 FROM plans p WHERE p.id=selected.plan_id)"
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
