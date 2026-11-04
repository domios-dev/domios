use super::*;
use crate::plan::PlanStatus;

impl PlanDatabase {
    pub(in crate::service_plan) fn plan_exists_projection(
        &self,
        plan: &str,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                [plan],
                |row| column(row, 0),
            )
            .map_err(db_error)
    }

    pub(in crate::service_plan) fn plan_owner_tab_projection(
        &self,
        plan: &str,
    ) -> Result<Option<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                "SELECT json_extract(header_json,'$.owner.tab_id') FROM plans WHERE id=?1",
                [plan],
                |row| column(row, 0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(db_error)
    }

    pub(in crate::service_plan) fn plan_status_projection(
        &self,
        plan: &str,
    ) -> Result<PlanStatus, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: String = connection
            .query_row(
                "SELECT json_quote(status) FROM plans WHERE id=?1",
                [plan],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        decode(plan, &raw)
    }

    pub(in crate::service_plan) fn plan_project_ids_projection(
        &self,
        project: &str,
    ) -> Result<Vec<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare("SELECT id FROM plans WHERE project_path=?1 ORDER BY id")
            .map_err(db_error)?;
        let rows = statement
            .query_map([project], |row| column(row, 0))
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }
}
