use super::read_projections::{column, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;
use rusqlite::params;

impl PlanDatabase {
    pub(in crate::service_plan) fn open_attempt_binding(
        &self,
        tab: &str,
        session: &str,
        nonce: &str,
        unknown_model: bool,
    ) -> Result<Option<(String, String, String)>, PlanError> {
        if tab.trim().is_empty() || session.trim().is_empty() || nonce.trim().is_empty() {
            return Ok(None);
        }
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare_cached(
            "SELECT p.id,t.task_id,json_extract(a.row_json,'$.id')
             FROM plans p JOIN plan_tasks t ON t.plan_id=p.id JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
             WHERE p.status='active' AND t.status IN ('pending','in_progress','in_review','blocked','paused')
               AND a.executor_tab_id=?1 AND a.open=1
               AND a.session_id=?2 AND a.session_binding_nonce=?3
               AND (?4=0 OR json_extract(a.row_json,'$.session_model') IS NULL)
             LIMIT 2"
        ).map_err(db_error)?;
        let rows = statement
            .query_map(params![tab, session, nonce, unknown_model], |row| {
                Ok((column(row, 0)?, column(row, 1)?, column(row, 2)?))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        Ok(if rows.len() == 1 {
            rows.into_iter().next()
        } else {
            None
        })
    }
}
