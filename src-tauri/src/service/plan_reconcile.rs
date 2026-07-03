use std::collections::HashMap;

use serde::Serialize;

use super::ServiceContext;
use crate::multiplexer::types::AgentRole;
use crate::plan::cleanup_debt::CleanupDebtTask;
use crate::plan::reconcile::{
    detect_pane_gone, detect_plan_drift, DriftItem, DriftKind, PaneProbe, PaneRole,
    ReconcileConfig, DEFAULT_STALE_REVIEW_MS,
};
use crate::service::plan::{cleanup_debt, PlanError, ReconcileFixResult};

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileResult {
    pub plan_id: String,
    pub drift: Vec<DriftItem>,
    pub cleanup_debt: Vec<CleanupDebtTask>,
    pub counts: HashMap<DriftKind, usize>,
    pub fixed: Vec<ReconcileFixResult>,
    pub accepted_signal: &'static str,
}

impl ServiceContext {
    pub fn reconcile_plan(
        &self,
        plan_id: &str,
        fix: bool,
        caller_pane_id: Option<&str>,
        now_ms: u64,
    ) -> Result<ReconcileResult, PlanError> {
        let plan = self.plan_store.get(plan_id).ok_or(PlanError::NotFound)?;
        let mut config = ReconcileConfig::new(now_ms);
        config.stale_review_ms = DEFAULT_STALE_REVIEW_MS;
        config.current_orchestrator_pane_id = caller_pane_id
            .map(str::to_string)
            .or_else(|| plan.owner.pane_id.clone());
        let drift = detect_plan_drift(&plan, &config, |pane_id| self.pane_probe(pane_id));
        let cleanup_debt = cleanup_debt::detect_plan_cleanup_debt(&plan, |pane_id| {
            self.pane_probe(pane_id).exists
        });
        let counts = drift_counts(&drift);
        let fixed = if fix {
            self.plan_store
                .apply_reconcile_fixes(plan_id, &drift, now_ms)?
        } else {
            Vec::new()
        };
        if !fixed.is_empty() {
            self.emit_plan_changed(plan_id);
        }
        Ok(ReconcileResult {
            plan_id: plan_id.into(),
            drift,
            cleanup_debt,
            counts,
            fixed,
            accepted_signal: "none",
        })
    }

    pub fn flag_bound_tasks_for_gone_pane(&self, pane_id: &str, now_ms: u64) {
        let bindings = self.plan_store.task_bindings_for_pane(pane_id);
        if bindings.is_empty() {
            return;
        }
        let mut changed_plans = Vec::new();
        for (plan_id, task_id) in bindings {
            let Some(plan) = self.plan_store.get(&plan_id) else {
                continue;
            };
            let item_exists = detect_pane_gone(&plan, pane_id)
                .iter()
                .any(|item| item.task_id == task_id);
            if !item_exists {
                continue;
            }
            match self.plan_store.flag_pane_gone(&plan_id, &task_id, now_ms) {
                Ok(true) => changed_plans.push(plan_id),
                Ok(false) => {}
                Err(e) => log::warn!(
                    "[plan-reconcile] failed to flag pane-gone for {plan_id}/{task_id}: {e}"
                ),
            }
        }
        changed_plans.sort();
        changed_plans.dedup();
        for plan_id in changed_plans {
            self.emit_plan_changed(&plan_id);
        }
    }

    pub(crate) fn pane_probe(&self, pane_id: &str) -> PaneProbe {
        let (exists, role, parent_pane_id, agent_type, pty_id) = {
            let mux = self.mux.read();
            match mux.get_pane(pane_id) {
                Some(pane) => (
                    true,
                    pane.role.map(map_role),
                    pane.parent_pane_id.clone(),
                    pane.agent_type.clone(),
                    pane.pty_id.clone(),
                ),
                None => (false, None, None, None, None),
            }
        };
        let live_agent = agent_type.is_some()
            && pty_id
                .as_deref()
                .is_some_and(|pty| self.pty_has_live_agent_tui(pty));
        PaneProbe {
            exists,
            live_agent,
            role,
            parent_pane_id,
        }
    }

    fn pty_has_live_agent_tui(&self, pty_id: &str) -> bool {
        let screen = self
            .screen_state
            .lock()
            .get_screen_text(pty_id)
            .unwrap_or_default();
        crate::pty::screen_state::screen_has_agent_tui(&screen)
            || self.agent_registry.any_provider_screen_has_tui(&screen)
    }

    fn emit_plan_changed(&self, plan_id: &str) {
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": plan_id }),
        );
    }
}

fn map_role(role: AgentRole) -> PaneRole {
    match role {
        AgentRole::Orchestrator => PaneRole::Orchestrator,
        AgentRole::Worker => PaneRole::Worker,
    }
}

fn drift_counts(items: &[DriftItem]) -> HashMap<DriftKind, usize> {
    let mut counts = HashMap::new();
    for item in items {
        *counts.entry(item.kind.clone()).or_insert(0) += 1;
    }
    counts
}
