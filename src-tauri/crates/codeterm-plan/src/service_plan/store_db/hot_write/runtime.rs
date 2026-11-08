use super::{append_stride, db_error, json_error, CommittedRows, PlanDatabase};
use crate::plan::{
    Milestone, OrchestratorHeartbeat, PlanMode, PlanPauseOperation, PlannerInboxEntry, PlannerRef,
    PlannerRequest,
};
use crate::service_plan::{store_rows, PlanError, PlanStore};
use rusqlite::{params, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

mod curator;
pub use curator::{CuratorRoundRows, CuratorSaveRows, CuratorStateRows};

pub(in crate::service_plan) trait RuntimeRows:
    Serialize + DeserializeOwned
{
    const FIELDS: &'static [&'static str];
    const HEARTBEAT: bool = false;
    const TASK_STATES: bool = false;
    const SKILL_EPISODES: bool = false;
    fn strides(&mut self) -> Vec<Milestone> {
        Vec::new()
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::service_plan) struct PlannerRows {
    pub planner: Option<PlannerRef>,
    pub planner_requests: Vec<PlannerRequest>,
    pub planner_inbox: Vec<PlannerInboxEntry>,
}
impl RuntimeRows for PlannerRows {
    const FIELDS: &'static [&'static str] = &["planner", "planner_requests", "planner_inbox"];
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::service_plan) struct PlannerSessionRows {
    pub planner: Option<PlannerRef>,
}
impl RuntimeRows for PlannerSessionRows {
    const FIELDS: &'static [&'static str] = &["planner"];
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::service_plan) struct PauseRows {
    pub mode: PlanMode,
    pub paused_from_mode: Option<PlanMode>,
    pub team_id: Option<String>,
    pub pause_operations: Vec<PlanPauseOperation>,
    #[serde(skip)]
    pub strides: Vec<Milestone>,
}
impl RuntimeRows for PauseRows {
    const FIELDS: &'static [&'static str] =
        &["mode", "paused_from_mode", "team_id", "pause_operations"];
    fn strides(&mut self) -> Vec<Milestone> {
        std::mem::take(&mut self.strides)
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::service_plan) struct HeartbeatRows {
    pub heartbeat: OrchestratorHeartbeat,
}
impl RuntimeRows for HeartbeatRows {
    const FIELDS: &'static [&'static str] = &[];
    const HEARTBEAT: bool = true;
}

impl PlanStore {
    pub(in crate::service_plan) fn with_runtime_rows<R: RuntimeRows, T>(
        &self,
        plan_id: &str,
        now: u64,
        change: impl FnOnce(&mut R) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self.database.edit_runtime_rows(plan_id, now, change)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(result)
    }
}

impl PlanDatabase {
    fn edit_runtime_rows<R: RuntimeRows, T>(
        &self,
        plan_id: &str,
        now: u64,
        change: impl FnOnce(&mut R) -> Result<T, PlanError>,
    ) -> Result<(T, CommittedRows), PlanError> {
        let result;
        let mut connection = self.connection.lock();
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let mut receipt = super::row_receipt(&tx, plan_id, now, 0)?;
        let receipt = &mut receipt;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                [plan_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        {
            let tx = &tx;
            let mut fields = serde_json::Map::new();
            for &field in R::FIELDS {
                let path = format!("$.{field}");
                let json: String = tx
                    .query_row(
                        "SELECT json_quote(json_extract(header_json, ?2)) FROM plans WHERE id=?1",
                        params![plan_id, path],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                receipt.decoded_bytes += json.len();
                let value: serde_json::Value = serde_json::from_str(&json).map_err(json_error)?;
                if !value.is_null() {
                    fields.insert(field.into(), value);
                }
            }
            if R::HEARTBEAT {
                let json: Option<String> = tx
                    .query_row(
                        "SELECT row_json FROM plan_heartbeat WHERE plan_id=?1",
                        [plan_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(db_error)?;
                if let Some(json) = json {
                    receipt.decoded_bytes += json.len();
                    fields.insert(
                        "heartbeat".into(),
                        serde_json::from_str(&json).map_err(json_error)?,
                    );
                }
            }
            curator::load_context::<R>(tx, plan_id, &mut fields, receipt)?;
            let before = serde_json::Value::Object(fields);
            let mut rows: R = serde_json::from_value(before).map_err(json_error)?;
            let before = serde_json::to_value(&rows).map_err(json_error)?;
            let output = change(&mut rows)?;
            let strides = rows.strides();
            let after = serde_json::to_value(rows).map_err(json_error)?;
            crate::service_plan::text_bounds::validate_stored_entity(&after)?;
            let changed = !strides.is_empty()
                || R::FIELDS
                    .iter()
                    .any(|field| before.get(*field) != after.get(*field))
                || (R::HEARTBEAT && before.get("heartbeat") != after.get("heartbeat"));
            if !changed {
                return Ok((output, std::mem::take(receipt)));
            }
            let operation_id = super::super::queue_operation(tx, plan_id, "persist_plan", now)?;
            receipt.revision =
                super::super::change_feed::next_revision(receipt.previous_revision, operation_id)?;
            for stride in strides {
                append_stride(tx, plan_id, &stride, receipt)?;
            }
            for &field in R::FIELDS {
                if before.get(field) == after.get(field) {
                    continue;
                }
                let json = serde_json::to_string(&after[field]).map_err(json_error)?;
                receipt.serialized_bytes += json.len();
                tx.execute(
                    "UPDATE plans SET header_json=json_set(header_json, ?2, json(?3)) WHERE id=?1",
                    params![plan_id, format!("$.{field}"), json],
                )
                .map_err(db_error)?;
                receipt.header_rows += 1;
            }
            if R::HEARTBEAT && before.get("heartbeat") != after.get("heartbeat") {
                let json = serde_json::to_string(&after["heartbeat"]).map_err(json_error)?;
                receipt.serialized_bytes += json.len();
                receipt.hashed_bytes += json.len();
                tx.execute(
                    "INSERT INTO plan_heartbeat(plan_id, content_hash, row_json) VALUES (?1, ?2, ?3)
                     ON CONFLICT(plan_id) DO UPDATE SET content_hash=excluded.content_hash, row_json=excluded.row_json",
                    params![plan_id, store_rows::row_hash(&json), json],
                ).map_err(db_error)?;
                receipt.heartbeat_rows += 1;
            }
            super::touch_header(tx, receipt)?;
            tx.execute(
                "UPDATE plan_operations SET state='committed',finished_at=?2 WHERE id=?1",
                params![operation_id, now],
            )
            .map_err(db_error)?;
            result = output;
        }
        let receipt = std::mem::take(receipt);
        tx.commit().map_err(db_error)?;
        drop(connection);
        self.record_row_change(&receipt);
        Ok((result, receipt))
    }
}

#[cfg(test)]
mod tests;
