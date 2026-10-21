use super::mutations::gen_id;
use super::{PlanError, PlanStore};
use crate::plan::loop_engine::{advance, can_advance, record_iteration, stop_for};
use crate::plan::{AgentRef, LoopSpec, LoopStatus, Milestone, TaskStatus};
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
        let lid = gen_id("loop");
        let spec = LoopSpec {
            id: lid.clone(),
            target: target.into(),
            budget,
            count: 0,
            stop_when: stop_when.map(|s| s.into()),
            status: LoopStatus::Running,
            stagnation: 0,
            deviation: None,
            last_iteration_at: None,
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
            lp.last_iteration_at = Some(now_ms);
            updated = Some(lp.clone());
            Ok(())
        })?;
        updated.ok_or(PlanError::NotFound)
    }

    pub fn record_loop(
        &self,
        loop_id: &str,
        note: &str,
        stop: bool,
        preferred_task_id: Option<&str>,
        now_ms: u64,
    ) -> Result<LoopRecordResult, PlanError> {
        let plan_id = self
            .find_plan_with_loop(loop_id)
            .ok_or(PlanError::NotFound)?;
        let mut result = None;
        self.with_plan(&plan_id, now_ms, |p| {
            let loop_idx = p
                .loops
                .iter()
                .position(|l| l.id == loop_id)
                .ok_or(PlanError::NotFound)?;
            let task_idx = resolve_loop_stride_task_index(p, preferred_task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let milestone_kind = ensure_loop_stride_kind(p);
            let (recorded_loop_id, iteration, count, budget, status) = {
                let loop_ref = &mut p.loops[loop_idx];
                if !can_advance(loop_ref) {
                    return Err(PlanError::BudgetExhausted {
                        count: loop_ref.count,
                        budget: loop_ref.budget,
                    });
                }
                advance(loop_ref);
                if stop {
                    stop_for(loop_ref);
                }
                loop_ref.last_iteration_at = Some(now_ms);
                (
                    loop_ref.id.clone(),
                    loop_ref.count,
                    loop_ref.count,
                    loop_ref.budget,
                    loop_ref.status.clone(),
                )
            };
            let milestone_id = gen_id("ms");
            let task_ref = &mut p.tasks[task_idx];
            task_ref.milestones.push(Milestone {
                id: milestone_id.clone(),
                from: AgentRef::from_tab("loop-recorder"),
                kind: milestone_kind,
                text: note.trim().to_string(),
                ts: now_ms,
                links: vec![],
                loop_id: Some(recorded_loop_id.clone()),
                iteration: Some(iteration),
                canvas: Default::default(),
                attachments: vec![],
            });
            task_ref.updated_at = now_ms;
            result = Some(LoopRecordResult {
                plan_id: p.id.clone(),
                loop_id: recorded_loop_id,
                task_id: task_ref.id.clone(),
                milestone_id,
                count,
                budget,
                status,
            });
            Ok(())
        })?;
        result.ok_or(PlanError::NotFound)
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

fn resolve_loop_stride_task_index(
    plan: &crate::plan::Plan,
    preferred_task_id: Option<&str>,
) -> Option<usize> {
    if let Some(task_id) = preferred_task_id {
        if let Some(index) = crate::plan::task_key::find_task_index(&plan.tasks, task_id) {
            return Some(index);
        }
    }

    plan.tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled))
        .filter(|(_, task)| task.kind == "planning")
        .max_by_key(|(_, task)| task.updated_at)
        .map(|(index, _)| index)
        .or_else(|| {
            plan.tasks
                .iter()
                .enumerate()
                .filter(|(_, task)| {
                    !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled)
                })
                .max_by_key(|(_, task)| task.updated_at)
                .map(|(index, _)| index)
        })
}

fn ensure_loop_stride_kind(plan: &mut crate::plan::Plan) -> String {
    for candidate in ["decision", "status_update"] {
        if plan
            .milestone_kinds
            .iter()
            .any(|kind| kind.key == candidate)
        {
            return candidate.to_string();
        }
    }
    plan.milestone_kinds.push(crate::plan::MilestoneKind {
        key: "status_update".into(),
        label: "status_update".into(),
        color: None,
    });
    "status_update".into()
}
