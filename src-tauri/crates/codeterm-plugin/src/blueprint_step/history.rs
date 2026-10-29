use super::*;
use rusqlite::params;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationView {
    pub decision_key: String,
    pub operation: String,
    pub context: StepContext,
    pub state: OperationState,
    pub job: Option<String>,
    pub waiting: Option<Waiting>,
    pub estimate: Option<Estimate>,
    pub authorization: Option<OperationApproval>,
    pub cancel_requested: bool,
}
impl From<OperationRecord> for OperationView {
    fn from(record: OperationRecord) -> Self {
        Self {
            decision_key: owner_decision_key(&record),
            operation: record.request.context.operation.clone(),
            context: record.request.context,
            state: record.state,
            job: record.job,
            waiting: record.waiting,
            estimate: record.estimate,
            authorization: record.authorized,
            cancel_requested: record.cancel_requested,
        }
    }
}

pub fn owner_decision_key(record: &OperationRecord) -> String {
    let hash = Sha256::digest(
        serde_json::to_vec(&(&record.estimate, record.question_revision, record.state))
            .unwrap_or_default(),
    );
    format!(
        "blueprint-plugin:{}:{hash:x}",
        record.request.context.operation
    )
}

impl OperationLedger {
    pub fn owner_operations(&self, scope: &PluginJobScope) -> Result<Vec<OperationView>, String> {
        let db = self.inner.db.lock();
        let mut query = db.prepare("SELECT record FROM blueprint_plugin_operations WHERE json_extract(record,'$.plugin')=?1 AND json_extract(record,'$.request.context.scope.workspace')=?2 AND json_extract(record,'$.request.context.scope.team') IS ?3 AND json_extract(record,'$.request.context.owner')=?4 ORDER BY rowid DESC LIMIT 256")
            .map_err(|_| "plugin operation history unavailable")?;
        let rows = query
            .query_map(
                params![scope.plugin, scope.workspace, scope.team, scope.owner],
                |row| row.get::<_, String>(0),
            )
            .map_err(|_| "plugin operation history unavailable")?;
        rows.map(|row| {
            let row = row.map_err(|_| "plugin operation history unavailable")?;
            serde_json::from_str::<OperationRecord>(&row)
                .map(OperationView::from)
                .map_err(|_| "invalid plugin operation history".into())
        })
        .collect()
    }
}
