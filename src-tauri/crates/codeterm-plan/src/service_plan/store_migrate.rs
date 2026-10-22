use super::store_rows;
use super::store_sync;
use rusqlite::{params, Connection, TransactionBehavior};

/// Moves tasks out of whole-document plan rows, one plan per transaction; unsplittable rows stay untouched.
pub(super) fn split_whole_documents(connection: &mut Connection) -> rusqlite::Result<usize> {
    let candidates = {
        let mut statement = connection
            .prepare("SELECT id FROM plans WHERE instr(document_json, '\"tasks\"') > 0")?;
        let ids = statement.query_map([], |row| row.get::<_, String>(0))?;
        ids.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut migrated = 0;
    for plan_id in candidates {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(document) = store_sync::plan_document(&transaction, &plan_id)? else {
            continue;
        };
        let rows = match split_document(&document) {
            Ok(Some(rows)) => rows,
            Ok(None) => continue,
            Err(error) => {
                log::error!("[plan.store] kept whole-document plan record {plan_id}: {error}");
                continue;
            }
        };
        store_sync::sync_task_rows(&transaction, &plan_id, &rows.tasks)?;
        transaction.execute(
            "UPDATE plans SET document_json = ?2 WHERE id = ?1",
            params![plan_id, rows.plan],
        )?;
        transaction.commit()?;
        migrated += 1;
        log::info!(
            "[plan.store] split plan record {plan_id} ({} bytes) into {} task rows, {} milestones, {} attempts",
            document.len(),
            rows.tasks.len(),
            rows.tasks.iter().map(|task| task.milestones.len()).sum::<usize>(),
            rows.tasks.iter().map(|task| task.attempts.len()).sum::<usize>()
        );
    }
    Ok(migrated)
}

fn split_document(document: &str) -> Result<Option<store_rows::PlanRows>, String> {
    let value: serde_json::Value =
        serde_json::from_str(document).map_err(|error| error.to_string())?;
    if value.get("tasks").is_none() {
        return Ok(None);
    }
    store_rows::split_plan(value).map(Some)
}
