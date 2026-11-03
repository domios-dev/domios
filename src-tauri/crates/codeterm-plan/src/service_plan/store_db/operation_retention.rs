use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection};

pub(in crate::service_plan) const OPERATION_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1000;
pub(in crate::service_plan) const MAX_TERMINAL_OPERATIONS: usize = 10_000;
const TERMINAL: &str = "state IN ('committed', 'failed') AND finished_at IS NOT NULL
    AND (error IS NULL OR error != 'daemon restarted before commit')";

impl PlanDatabase {
    pub(in crate::service_plan) fn prune_operations(
        &self,
        now_ms: u64,
    ) -> Result<usize, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let removed = prune(&transaction, now_ms, MAX_TERMINAL_OPERATIONS)?;
        transaction.commit().map_err(db_error)?;
        Ok(removed)
    }
}

fn prune(connection: &Connection, now_ms: u64, limit: usize) -> Result<usize, PlanError> {
    let cutoff = now_ms.saturating_sub(OPERATION_RETENTION_MS);
    connection
        .execute(
            &format!(
                "DELETE FROM plan_operations WHERE {TERMINAL} AND (
            finished_at < ?1 OR id IN (
                SELECT id FROM plan_operations WHERE {TERMINAL}
                ORDER BY finished_at DESC, id DESC LIMIT -1 OFFSET ?2
            ) OR (kind IN ('persist_plan', 'delete_plan') AND NOT EXISTS (
                SELECT 1 FROM plans WHERE plans.id = plan_operations.entity_id
            )) OR (kind IN ('persist_spec', 'delete_spec') AND NOT EXISTS (
                SELECT 1 FROM specs WHERE specs.id = plan_operations.entity_id
            ))
        )"
            ),
            params![cutoff, limit as i64],
        )
        .map_err(db_error)
}

pub(super) fn prune_deleted_plan(connection: &Connection, plan_id: &str) -> Result<(), PlanError> {
    connection
        .execute(
            &format!(
                "DELETE FROM plan_operations WHERE {TERMINAL} AND (
            (entity_id = ?1 AND kind IN ('persist_plan', 'delete_plan')) OR
            (kind IN ('persist_spec', 'delete_spec') AND entity_id IN (
                SELECT id FROM specs WHERE plan_id = ?1
            ))
        )"
            ),
            [plan_id],
        )
        .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
