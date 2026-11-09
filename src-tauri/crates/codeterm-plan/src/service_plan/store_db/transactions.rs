use super::*;

impl PlanDatabase {
    pub(super) fn commit_entity_rows<T>(
        &self,
        entity_id: &str,
        kind: &str,
        now_ms: u64,
        change: impl FnOnce(&rusqlite::Transaction<'_>, i64) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        let mut connection = self.connection.lock();
        let operation_id = queue_operation(&connection, entity_id, kind, now_ms)?;
        let result: Result<T, PlanError> = (|| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            transaction
                .execute(
                    "UPDATE plan_operations SET state='running' WHERE id=?1",
                    [operation_id],
                )
                .map_err(db_error)?;
            let output = change(&transaction, operation_id)?;
            transaction
                .execute(
                    "UPDATE plan_operations SET state='committed',finished_at=?2 WHERE id=?1",
                    params![operation_id, now_ms],
                )
                .map_err(db_error)?;
            transaction.commit().map_err(db_error)?;
            Ok(output)
        })();
        if let Err(error) = &result {
            let _ = connection.execute(
                "UPDATE plan_operations SET state='failed',finished_at=?2,error=?3 WHERE id=?1",
                params![operation_id, now_ms, error.to_string()],
            );
        }
        result
    }
}
