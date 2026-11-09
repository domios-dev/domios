use super::*;
use crate::plan::LaneTurnChangeStats;
use crate::service_plan::store_db::read_projections::lane_settlement::LaneSettlementRead;
use crate::service_plan::{PlanStore, TaskLaneUpdate};

impl PlanDatabase {
    pub(in crate::service_plan) fn edit_lane_turn_rows<T>(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        now_ms: u64,
        update: impl FnOnce(&mut Vec<LaneTurnChangeStats>) -> Result<Option<T>, PlanError>,
    ) -> Result<(Option<T>, CommittedRows), PlanError> {
        let mut output = None;
        let receipt=self.hot_transaction(plan_id,now_ms,|tx,receipt| {
            let (position,document):(i64,String)=tx.query_row("SELECT position,row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND attempt_id=?3 ORDER BY position LIMIT 1",params![plan_id,task_id,attempt_id],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
            receipt.decoded_bytes += document.len();
            let mut attempt:TaskExecutionAttempt=serde_json::from_str(&document).map_err(json_error)?;
            output=update(&mut attempt.lane_turn_stats)?;
            if output.is_none() { return Ok(()); }
            let document=encode(&attempt,receipt)?;
            receipt.hashed_bytes += document.len();
            receipt.attempts += tx.execute("UPDATE plan_task_attempts SET row_json=?4,content_hash=?5 WHERE plan_id=?1 AND task_id=?2 AND position=?3",params![plan_id,task_id,position,document,store_rows::row_hash(&document)]).map_err(db_error)?;
            Ok(())
        })?;
        Ok((output, receipt))
    }

    pub(in crate::service_plan) fn settle_lane_history_rows(
        &self,
        plan_id: &str,
        now_ms: u64,
        decide: impl FnOnce(&[LaneSettlementRead]) -> Vec<(String, TaskLaneUpdate)>,
    ) -> Result<(usize, CommittedRows), PlanError> {
        let mut count = 0;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let facts = super::super::read_projections::lane_settlement::lane_settlement_rows(
                tx,
                Some(plan_id),
                None,
                None,
                |raw| receipt.decoded_bytes += raw.len(),
            )?;
            let updates = decide(&facts);
            count = updates.len();
            for (id, update) in updates {
                if !facts.iter().any(|fact| fact.task_id == id) {
                    return Err(PlanError::TaskNotFound);
                }
                task_fields::apply(
                    tx,
                    plan_id,
                    &id,
                    TaskFields {
                        lane: Some(update),
                        ..Default::default()
                    },
                    now_ms,
                    receipt,
                )?;
            }
            Ok(())
        })?;
        Ok((count, receipt))
    }
}

impl PlanStore {
    pub fn edit_lane_turn_stats<T>(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        now_ms: u64,
        update: impl FnOnce(&mut Vec<LaneTurnChangeStats>) -> Result<Option<T>, PlanError>,
    ) -> Result<Option<T>, PlanError> {
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self
            .database
            .edit_lane_turn_rows(plan_id, task_id, attempt_id, now_ms, update)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(result)
    }

    pub fn settle_lane_history(
        &self,
        plan_id: &str,
        now_ms: u64,
        decide: impl FnOnce(&[LaneSettlementRead]) -> Vec<(String, TaskLaneUpdate)>,
    ) -> Result<usize, PlanError> {
        let turn = self.mutation_writer.enter();
        let (count, receipt) = self
            .database
            .settle_lane_history_rows(plan_id, now_ms, decide)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
