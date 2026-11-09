use super::{column, record_query, SECTION_ROWS};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::params;

impl PlanStore {
    pub fn section_removal_ids_projection(
        &self,
        plan: &str,
        section: &str,
    ) -> Result<Vec<String>, PlanError> {
        self.database.section_removal_ids_projection(plan, section)
    }
}

impl PlanDatabase {
    fn section_removal_ids_projection(
        &self,
        plan: &str,
        section: &str,
    ) -> Result<Vec<String>, PlanError> {
        self.ensure_plan_exists(plan)?;
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection.prepare(&format!(
            "WITH sections AS ({SECTION_ROWS}), selected AS (
                SELECT position,path FROM sections WHERE id=?2 OR handle=?2 ORDER BY position,traversal LIMIT 1)
             SELECT r.id FROM sections r JOIN selected s ON r.position=s.position
                AND (r.path=s.path OR instr(r.path,s.path||'.subsections[')=1)
             ORDER BY r.traversal"
        )).map_err(db_error)?;
        let rows = statement
            .query_map(params![plan, section], |row| column(row, 0))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }
}

#[cfg(test)]
mod tests;
