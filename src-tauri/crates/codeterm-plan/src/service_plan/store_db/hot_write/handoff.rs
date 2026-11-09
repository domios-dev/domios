use super::*;
use crate::plan::{CanvasElement, LoopSpec, OrchestratorHeartbeat};
use crate::service_plan::PlanStore;

pub(in crate::service_plan) struct HandoffRow {
    pub id: String,
    pub elements: Vec<CanvasElement>,
    pub heartbeat: OrchestratorHeartbeat,
    pub loops: Vec<LoopSpec>,
    pub active_reseed: bool,
}

impl PlanStore {
    pub(in crate::service_plan) fn edit_handoff<T>(
        &self,
        id: &str,
        selector: Option<&str>,
        active_tab: Option<&str>,
        snapshot: bool,
        now: u64,
        change: impl FnOnce(&mut HandoffRow) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self
            .database
            .edit_handoff_row(id, selector, active_tab, snapshot, now, change)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(result)
    }
}

impl PlanDatabase {
    fn edit_handoff_row<T>(
        &self,
        id: &str,
        selector: Option<&str>,
        active_tab: Option<&str>,
        snapshot: bool,
        now: u64,
        change: impl FnOnce(&mut HandoffRow) -> Result<T, PlanError>,
    ) -> Result<(T, CommittedRows), PlanError> {
        let mut result = None;
        let receipt = self.hot_transaction(id, now, |tx, receipt| {
            let json: Option<String> = tx.query_row(
                "SELECT row_json FROM plan_elements WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='handoff'
                 AND (?2 IS NULL OR element_id=?2 OR domios_handoff_seq(row_json->'$.content.seq')=?3)
                 ORDER BY CASE WHEN ?2 IS NULL THEN domios_handoff_seq(row_json->'$.content.seq') END DESC,
                    CASE WHEN ?2 IS NULL THEN position END DESC, position ASC LIMIT 1",
                params![id, selector, selector.and_then(|value| value.parse::<u64>().ok()).map(|value| format!("{value:020}"))],
                |row| row.get(0),
            ).optional().map_err(db_error)?;
            let elements: Vec<CanvasElement> = json.as_ref().map(|json| {
                receipt.decoded_bytes += json.len();
                serde_json::from_str(json).map_err(json_error)
            }).transpose()?.into_iter().collect();
            let heartbeat: OrchestratorHeartbeat = if snapshot {
                let json: Option<String> = tx.query_row("SELECT row_json FROM plan_heartbeat WHERE plan_id=?1", [id], |row| row.get(0)).optional().map_err(db_error)?;
                json.map(|json| { receipt.decoded_bytes += json.len(); serde_json::from_str(&json).map_err(json_error) }).transpose()?.unwrap_or_default()
            } else { Default::default() };
            let loops = if snapshot {
                let json: String = tx.query_row("SELECT coalesce(json_extract(header_json,'$.loops'),'[]') FROM plans WHERE id=?1", [id], |row| row.get(0)).map_err(db_error)?;
                receipt.decoded_bytes += json.len();
                serde_json::from_str(&json).map_err(json_error)?
            } else { Vec::new() };
            let active_reseed = if let Some(tab) = active_tab {
                tx.query_row("SELECT EXISTS(SELECT 1 FROM plan_elements WHERE plan_id=?1
                    AND json_extract(row_json,'$.kind')='handoff' AND json_type(row_json,'$.content.reseed_pending')='object'
                    AND json_type(row_json,'$.content.reseed_pending.tab_id')='text'
                    AND json_extract(row_json,'$.content.reseed_pending.tab_id')=?2
                    AND json_extract(row_json,'$.content.reseed_state') IN ('new_sent','session_seen','resume_sent'))",
                    params![id,tab], |row| row.get(0)).map_err(db_error)?
            } else { false };
            let mut rows = HandoffRow { id: id.into(), elements, heartbeat, loops, active_reseed };
            let output = change(&mut rows)?;
            if let (Some(before), Some(element)) = (json, rows.elements.first()) {
                let after = encode(element, receipt)?;
                if before != after {
                    receipt.hashed_bytes += after.len();
                    tx.execute("UPDATE plan_elements SET row_json=?3, content_hash=?4 WHERE plan_id=?1 AND element_id=?2",
                        params![id, element.id, after, store_rows::row_hash(&after)]).map_err(db_error)?;
                    receipt.element_rows += 1;
                }
            }
            result = Some(output);
            Ok(())
        })?;
        result
            .map(|result| (result, receipt))
            .ok_or_else(|| PlanError::Io("handoff transaction produced no result".into()))
    }
}

#[cfg(test)]
mod tests;
