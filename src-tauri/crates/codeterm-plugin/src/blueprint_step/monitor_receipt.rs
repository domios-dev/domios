use super::*;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobOriginReceipt {
    pub operation: String,
    pub scope: PluginJobScope,
    pub origin: PluginJobOrigin,
    pub admitted_at_ms: u64,
    pub deadline_ms: u64,
    pub job: Option<String>,
}

impl OperationLedger {
    pub fn admit_job_origin(
        &self,
        scope: PluginJobScope,
        origin: PluginJobOrigin,
        now: u64,
    ) -> Result<JobOriginReceipt, String> {
        let receipt = JobOriginReceipt {
            operation: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
            scope,
            origin,
            admitted_at_ms: now,
            deadline_ms: now.saturating_add(7 * 24 * 60 * 60 * 1000),
            job: None,
        };
        let probe = PluginJobRegistration {
            scope: receipt.scope.clone(),
            origin: receipt.origin.clone(),
            operation: receipt.operation.clone(),
            job: "identity-probe".into(),
            next_check_ms: now.saturating_add(1000),
            deadline_ms: receipt.deadline_ms,
        };
        if !probe.valid_identity()
            || !crate::media_delivery::valid_plugin_id(&probe.scope.plugin)
            || receipt.deadline_ms > MAX_COST_AMOUNT
        {
            return Err("invalid trusted plugin job origin".into());
        }
        let db = self.inner.db.lock();
        let count: u64 = db
            .query_row(
                "SELECT count(*) FROM blueprint_job_origins WHERE plugin=?1 AND deadline>?2",
                params![receipt.scope.plugin, now],
                |row| row.get(0),
            )
            .map_err(receipt_error)?;
        let total: u64 = db
            .query_row(
                "SELECT count(*) FROM blueprint_job_origins WHERE deadline>?1",
                [now],
                |row| row.get(0),
            )
            .map_err(receipt_error)?;
        if count >= 256 || total >= 1024 {
            return Err("plugin origin receipt capacity reached".into());
        }
        db.execute("INSERT INTO blueprint_job_origins(operation,plugin,deadline,record) VALUES(?1,?2,?3,?4)", params![receipt.operation, receipt.scope.plugin, receipt.deadline_ms, serde_json::to_string(&receipt).map_err(|_| "invalid plugin origin receipt")?]).map_err(receipt_error)?;
        Ok(receipt)
    }

    pub fn job_origin(&self, plugin: &str, operation: &str) -> Result<JobOriginReceipt, String> {
        let db = self.inner.db.lock();
        read(&db, plugin, operation)
    }

    pub fn attach_existing_job(
        &self,
        plugin: &str,
        operation: &str,
        job: &str,
    ) -> Result<JobOriginReceipt, String> {
        if !valid_job_identity(job) {
            return Err("invalid exact plugin job identity".into());
        }
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(receipt_error)?;
        let mut receipt = read(&tx, plugin, operation)?;
        if receipt.job.as_ref().is_some_and(|existing| existing != job) {
            return Err("plugin job identity cannot be replaced".into());
        }
        receipt.job = Some(job.into());
        tx.execute(
            "UPDATE blueprint_job_origins SET record=?2 WHERE operation=?1",
            params![
                operation,
                serde_json::to_string(&receipt).map_err(|_| "invalid plugin origin receipt")?
            ],
        )
        .map_err(receipt_error)?;
        tx.commit().map_err(receipt_error)?;
        Ok(receipt)
    }
}

fn read(
    db: &rusqlite::Connection,
    plugin: &str,
    operation: &str,
) -> Result<JobOriginReceipt, String> {
    let value: Option<String> = db
        .query_row(
            "SELECT record FROM blueprint_job_origins WHERE operation=?1 AND plugin=?2",
            params![operation, plugin],
            |row| row.get(0),
        )
        .optional()
        .map_err(receipt_error)?;
    serde_json::from_str(&value.ok_or("plugin origin receipt unavailable")?)
        .map_err(|_| "invalid stored plugin origin receipt".into())
}

fn receipt_error(_: rusqlite::Error) -> String {
    "plugin origin receipt persistence failed".into()
}
