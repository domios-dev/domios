use super::{PlanError, PlanStore};
use crate::plan::loop_engine::{advance, can_advance, record_iteration, stop_for};
use crate::plan::{LoopSpec, LoopStage, LoopStatus};

fn gen_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

impl PlanStore {
    pub fn create_loop(
        &self,
        plan_id: &str,
        target: &str,
        stages: Vec<LoopStage>,
        budget: u32,
        stop_when: Option<&str>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        if budget == 0 {
            return Err(PlanError::BudgetExhausted { count: 0, budget });
        }
        let lid = gen_id("loop");
        let spec = LoopSpec {
            id: lid.clone(),
            target: target.into(),
            stages,
            budget,
            count: 0,
            stop_when: stop_when.map(|s| s.into()),
            status: LoopStatus::Running,
            stagnation: 0,
            deviation: None,
        };
        self.with_plan(plan_id, now_ms, |p| {
            p.loops.push(spec);
            Ok(())
        })?;
        Ok(lid)
    }

    pub fn set_loop(
        &self,
        loop_id: &str,
        budget: Option<u32>,
        stop_when: Option<Option<&str>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let plan_id = self
            .find_plan_with_loop(loop_id)
            .ok_or(PlanError::NotFound)?;
        self.with_plan(&plan_id, now_ms, |p| {
            let lp = p
                .loops
                .iter_mut()
                .find(|l| l.id == loop_id)
                .ok_or(PlanError::NotFound)?;
            if let Some(b) = budget {
                if lp.count >= b {
                    return Err(PlanError::BudgetExhausted {
                        count: lp.count,
                        budget: b,
                    });
                }
                lp.budget = b;
            }
            if let Some(sw) = stop_when {
                lp.stop_when = sw.map(|s| s.into());
            }
            Ok(())
        })
    }

    pub fn loop_status(&self, loop_id: &str) -> Option<LoopSpec> {
        let plan_id = self.find_plan_with_loop(loop_id)?;
        self.get(&plan_id)
            .and_then(|p| p.loops.iter().find(|l| l.id == loop_id).cloned())
    }

    pub fn stop_loop(&self, loop_id: &str, now_ms: u64) -> Result<(), PlanError> {
        let plan_id = self
            .find_plan_with_loop(loop_id)
            .ok_or(PlanError::NotFound)?;
        self.with_plan(&plan_id, now_ms, |p| {
            let lp = p
                .loops
                .iter_mut()
                .find(|l| l.id == loop_id)
                .ok_or(PlanError::NotFound)?;
            stop_for(lp);
            Ok(())
        })
    }

    pub fn advance_loop(
        &self,
        loop_id: &str,
        made_progress: bool,
        now_ms: u64,
    ) -> Result<LoopSpec, PlanError> {
        let plan_id = self
            .find_plan_with_loop(loop_id)
            .ok_or(PlanError::NotFound)?;
        let goal_vector = self
            .get(&plan_id)
            .and_then(|p| p.goal.map(|g| g.vector))
            .unwrap_or_default();
        let mut updated = None;
        self.with_plan(&plan_id, now_ms, |p| {
            let lp = p
                .loops
                .iter_mut()
                .find(|l| l.id == loop_id)
                .ok_or(PlanError::NotFound)?;
            if !can_advance(lp) {
                return Err(PlanError::BudgetExhausted {
                    count: lp.count,
                    budget: lp.budget,
                });
            }
            advance(lp);
            record_iteration(lp, made_progress, &goal_vector);
            updated = Some(lp.clone());
            Ok(())
        })?;
        updated.ok_or(PlanError::NotFound)
    }

    fn find_plan_with_loop(&self, loop_id: &str) -> Option<String> {
        for entry in self.list_index() {
            if let Some(plan) = self.get(&entry.plan_id) {
                if plan.loops.iter().any(|l| l.id == loop_id) {
                    return Some(entry.plan_id);
                }
            }
        }
        None
    }
}
