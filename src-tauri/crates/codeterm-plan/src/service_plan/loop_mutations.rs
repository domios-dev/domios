use super::mutations::gen_id;
use super::store_db::hot_write::LoopChange;
use super::{PlanError, PlanStore};
use crate::plan::{LoopSpec, LoopStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopRecordResult {
    pub plan_id: String,
    pub loop_id: String,
    pub task_id: String,
    pub milestone_id: String,
    pub count: u32,
    pub budget: u32,
    pub status: LoopStatus,
}

impl PlanStore {
    pub fn create_loop(
        &self,
        plan_id: &str,
        target: &str,
        budget: u32,
        stop_when: Option<&str>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        if budget == 0 {
            return Err(PlanError::BudgetExhausted { count: 0, budget });
        }
        let id = gen_id("loop");
        let spec = LoopSpec {
            id: id.clone(),
            target: target.into(),
            budget,
            count: 0,
            stop_when: stop_when.map(str::to_string),
            status: LoopStatus::Running,
            stagnation: 0,
            deviation: None,
            last_iteration_at: None,
        };
        let turn = self.mutation_writer.enter();
        let (_, _, receipt) =
            self.database
                .change_loop(plan_id, &id, LoopChange::Create(spec), now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(id)
    }

    pub fn set_loop(
        &self,
        loop_id: &str,
        budget: Option<u32>,
        stop_when: Option<Option<&str>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.edit_loop(
            loop_id,
            LoopChange::Set {
                budget,
                stop_when: stop_when.map(|value| value.map(str::to_string)),
            },
            now_ms,
        )?;
        Ok(())
    }

    pub fn loop_status(&self, loop_id: &str) -> Result<LoopSpec, PlanError> {
        self.database.loop_status(loop_id)
    }

    pub fn stop_loop(&self, loop_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.edit_loop(loop_id, LoopChange::Stop, now_ms)?;
        Ok(())
    }

    pub fn advance_loop(
        &self,
        loop_id: &str,
        made_progress: bool,
        now_ms: u64,
    ) -> Result<LoopSpec, PlanError> {
        self.edit_loop(loop_id, LoopChange::Advance { made_progress }, now_ms)
            .map(|(spec, _)| spec)
    }

    pub fn record_loop(
        &self,
        loop_id: &str,
        note: &str,
        stop: bool,
        preferred_task_id: Option<&str>,
        now_ms: u64,
    ) -> Result<LoopRecordResult, PlanError> {
        let (_, record) = self.edit_loop(
            loop_id,
            LoopChange::Record {
                note: note.into(),
                stop,
                preferred_task_id: preferred_task_id.map(str::to_string),
                milestone_id: gen_id("ms"),
            },
            now_ms,
        )?;
        record.ok_or(PlanError::NotFound)
    }

    fn edit_loop(
        &self,
        loop_id: &str,
        change: LoopChange,
        now_ms: u64,
    ) -> Result<(LoopSpec, Option<LoopRecordResult>), PlanError> {
        let plan_id = self.loop_plan_id(loop_id)?.ok_or(PlanError::NotFound)?;
        let turn = self.mutation_writer.enter();
        let (spec, record, receipt) = self
            .database
            .change_loop(&plan_id, loop_id, change, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok((spec, record))
    }
}
