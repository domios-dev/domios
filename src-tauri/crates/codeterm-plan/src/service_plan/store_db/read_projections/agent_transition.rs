use super::{column, record_query};
use crate::service_plan::{PlanError, PlanStore};

impl PlanStore {
    pub fn transition_worker_is_task_bound_projection(&self, tab: &str) -> Result<bool, PlanError> {
        self.database
            .transition_worker_is_task_bound_projection(tab)
    }

    pub fn transition_worker_task_bindings_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<(String, String)>, PlanError> {
        self.database
            .transition_worker_task_bindings_projection(tab)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn transition_worker_is_task_bound_projection(
        &self,
        tab: &str,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE status NOT IN ('done','cancelled')
                AND (assigned_tab_id=?1 OR json_extract(row_json,'$.lane.tab_id')=?1))
                OR EXISTS(SELECT 1 FROM plan_task_attempts WHERE executor_tab_id=?1
                    AND json_extract(row_json,'$.closed_at') IS NULL)",
                [tab],
                |row| column(row, 0),
            )
            .map_err(super::super::db_error)
    }

    pub(in crate::service_plan) fn transition_worker_task_bindings_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<(String, String)>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare_cached(
                "SELECT t.plan_id,t.task_id FROM plan_tasks t JOIN plans p ON p.id=t.plan_id
                WHERE t.assigned_tab_id=?1 AND t.status NOT IN ('done','cancelled')
                ORDER BY p.updated_at DESC,p.id,t.position",
            )
            .map_err(super::super::db_error)?;
        let rows = statement
            .query_map([tab], |row| Ok((column(row, 0)?, column(row, 1)?)))
            .map_err(super::super::db_error)?;
        rows.collect::<Result<_, _>>()
            .map_err(super::super::db_error)
    }
}
