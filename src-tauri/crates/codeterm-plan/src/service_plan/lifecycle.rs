use super::store_db::hot_write::runtime::PauseRows;
use super::{PlanError, PlanStore};
use crate::plan::{AgentRef, Milestone, PlanMode};

impl PlanStore {
    pub fn pause_plan(
        &self,
        plan_id: &str,
        reason: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(PlanError::PauseRequiresReason);
        }
        let mut mid = String::new();
        self.with_runtime_rows::<PauseRows, _>(plan_id, now_ms, |p| {
            if p.mode == PlanMode::Paused {
                return Ok(());
            }
            mid = crate::plan::ids::short_id("ms");
            p.paused_from_mode = Some(p.mode.clone());
            p.mode = PlanMode::Paused;
            p.strides.push(Milestone {
                id: mid.clone(),
                from,
                kind: "decision".into(),
                text: format!("paused: {reason}"),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(())
        })?;
        Ok(mid)
    }

    pub fn resume_plan(&self, plan_id: &str, now_ms: u64) -> Result<PlanMode, PlanError> {
        let mut mode = PlanMode::Planning;
        self.with_runtime_rows::<PauseRows, _>(plan_id, now_ms, |p| {
            if p.team_id
                .as_ref()
                .is_none_or(|team_id| team_id.trim().is_empty())
            {
                return Err(PlanError::ActivePlanRequiresTeam {
                    plan_id: plan_id.into(),
                });
            }
            if p.mode == PlanMode::Paused {
                mode = p.paused_from_mode.take().unwrap_or(PlanMode::Execution);
                p.mode = mode.clone();
            } else {
                mode = p.mode.clone();
            }
            Ok(())
        })?;
        Ok(mode)
    }
}

#[cfg(test)]
mod tests;
