use super::read_projections::{column, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;
use rusqlite::params;

impl PlanDatabase {
    pub(in crate::service_plan) fn assigned_task_bindings(
        &self,
        tab: &str,
    ) -> Result<Vec<(String, String, String)>, PlanError> {
        if tab.trim().is_empty() {
            return Ok(Vec::new());
        }
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare_cached(
            "SELECT p.id,json_extract(t.row_json,'$.id'),json_extract(t.row_json,'$.name')
             FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
             WHERE p.status='active' AND t.status IN ('pending','in_progress','in_review','blocked','paused')
               AND t.assigned_tab_id=?1 ORDER BY p.updated_at DESC,p.id,t.position"
        ).map_err(db_error)?;
        let rows = statement
            .query_map([tab], |row| {
                Ok((column(row, 0)?, column(row, 1)?, column(row, 2)?))
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn unclosed_attempt_bindings(
        &self,
        tab: &str,
    ) -> Result<Vec<(String, String)>, PlanError> {
        if tab.trim().is_empty() {
            return Ok(Vec::new());
        }
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare_cached(
            "SELECT p.id,json_extract(t.row_json,'$.id')
             FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
             WHERE p.status='active' AND t.status IN ('pending','in_progress','in_review','blocked','paused')
               AND EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=p.id AND a.task_id=t.task_id
                   AND a.executor_tab_id=?1 AND json_extract(a.row_json,'$.closed_at') IS NULL)
             ORDER BY p.updated_at DESC,p.id,t.position"
        ).map_err(db_error)?;
        let rows = statement
            .query_map([tab], |row| Ok((column(row, 0)?, column(row, 1)?)))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

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
