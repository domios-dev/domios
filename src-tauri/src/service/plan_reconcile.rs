use std::collections::HashMap;

use serde::Serialize;

use super::ServiceContext;

pub(crate) fn tab_has_live_agent(tab: &crate::multiplexer::types::Tab) -> bool {
    let accepting_work = match tab.agent_state.as_ref() {
        Some(state) => matches!(
            state.state,
            crate::agent::offline_state_classifier::TimelineState::Working
                | crate::agent::offline_state_classifier::TimelineState::Idle
        ),
        None => tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady),
    };
    let delivery_target = tab.chat_view
        && match &tab.backing {
            crate::multiplexer::types::TabBacking::ChatBackend { .. } => true,
            crate::multiplexer::types::TabBacking::Pty => tab.pty_id.is_some(),
        };
    tab.agent_type.is_some()
        && tab.agent_session_id.is_some()
        && tab.session_binding_nonce.is_some()
        && tab.session_binding_nonce == tab.reported_session_binding_nonce
        && tab.exit_suspected_at.is_none()
        && delivery_target
        && accepting_work
}
use crate::multiplexer::types::AgentRole;
use crate::plan::cleanup_debt::CleanupDebtTask;
use crate::plan::lane_liveness::LaneThresholds;
use crate::plan::reconcile::{
    detect_lane_demotions, detect_plan_drift, detect_tab_gone, DriftItem, DriftKind,
    ReconcileConfig, TabProbe, TabRole, DEFAULT_STALE_REVIEW_MS,
};
use crate::plan::{PlanStatus, Task, TaskStatus};
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
        caller_tab_id: Option<&str>,
        now_ms: u64,
    ) -> Result<ReconcileResult, PlanError> {
        let plan = self.plan_store.get(plan_id).ok_or(PlanError::NotFound)?;
        let mut config = ReconcileConfig::new(now_ms);
        config.stale_review_ms = DEFAULT_STALE_REVIEW_MS;
        config.current_orchestrator_tab_id = caller_tab_id
            .map(str::to_string)
            .or_else(|| plan.owner.tab_id.clone());
        let drift = detect_plan_drift(&plan, &config, |tab_id| self.tab_probe(tab_id));
        let cleanup_debt =
            cleanup_debt::detect_plan_cleanup_debt(&plan, |tab_id| self.tab_probe(tab_id).exists);
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

    pub fn flag_bound_tasks_for_gone_tab(&self, tab_id: &str, now_ms: u64) {
        let bindings = self.plan_store.task_bindings_for_tab(tab_id);
        if bindings.is_empty() {
            return;
        }
        let mut changed_plans = Vec::new();
        for (plan_id, task_id) in bindings {
            let Some(plan) = self.plan_store.get(&plan_id) else {
                continue;
            };
            let item_exists = detect_tab_gone(&plan, tab_id)
                .iter()
                .any(|item| item.task_id == task_id);
            if !item_exists {
                continue;
            }
            match self.plan_store.flag_tab_gone(&plan_id, &task_id, now_ms) {
                Ok(true) => changed_plans.push(plan_id),
                Ok(false) => {}
                Err(e) => log::warn!(
                    "[plan-reconcile] failed to flag tab-gone for {plan_id}/{task_id}: {e}"
                ),
            }
        }
        changed_plans.sort();
        changed_plans.dedup();
        for plan_id in changed_plans {
            self.emit_plan_changed(&plan_id);
        }
    }

    /// Periodic watchdog: release in_progress lanes whose tab is gone past the
    /// grace window back to pending so the Board stops showing phantom active
    /// work. Runs on the reminder-scheduler cadence, not a busy loop.
    pub fn run_lane_watchdog(&self) {
        let now_ms = now_ms();
        let thresholds = LaneThresholds::default();
        let mut changed_plans = Vec::new();
        for plan in self.plan_store.list_full() {
            if plan.status != PlanStatus::Active {
                continue;
            }
            let mut config = ReconcileConfig::new(now_ms);
            config.current_orchestrator_tab_id = plan.owner.tab_id.clone();
            let expected: HashMap<String, String> = plan
                .tasks
                .iter()
                .filter(|task| task.status == TaskStatus::InProgress)
                .filter_map(|task| {
                    Some((task.assigned_tab_id.clone()?, live_attempt_session(task)?))
                })
                .collect();
            let demotions = detect_lane_demotions(&plan, &config, &thresholds, |tab_id| {
                self.lane_tab_probe(tab_id, expected.get(tab_id).map(String::as_str))
            });
            if demotions.is_empty() {
                continue;
            }
            match self
                .plan_store
                .apply_lane_demotions(&plan.id, &demotions, now_ms)
            {
                Ok(fixed) if !fixed.is_empty() => changed_plans.push(plan.id.clone()),
                Ok(_) => {}
                Err(e) => log::warn!("[lane-watchdog] demotion failed for {}: {e}", plan.id),
            }
        }
        for plan_id in changed_plans {
            self.emit_plan_changed(&plan_id);
        }
    }

    // Probe keyed off the lane's OWN session: a recycled tab id now hosting a
    // different agent reads as gone, not live.
    fn lane_tab_probe(&self, tab_id: &str, expected_session: Option<&str>) -> TabProbe {
        let mut probe = self.tab_probe(tab_id);
        if probe.exists {
            if let Some(expected) = expected_session {
                let same = {
                    let mux = self.mux.read();
                    mux.get_tab(tab_id)
                        .and_then(|tab| tab.agent_session_id.clone())
                        .map(|actual| actual == expected)
                        .unwrap_or(true)
                };
                if !same {
                    probe.exists = false;
                    probe.live_agent = false;
                }
            }
        }
        probe
    }

    pub(crate) fn tab_probe(&self, tab_id: &str) -> TabProbe {
        let (
            exists,
            role,
            parent_tab_id,
            session_id,
            nonce,
            reported_nonce,
            state,
            boot_stage,
            chat_delivery_target,
        ) = {
            let mux = self.mux.read();
            match mux.get_tab(tab_id) {
                Some(tab) => (
                    true,
                    tab.role.map(map_role),
                    tab.parent_tab_id.clone(),
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                    tab.reported_session_binding_nonce.clone(),
                    tab.agent_state.clone(),
                    tab.boot_stage,
                    tab.chat_view
                        && match &tab.backing {
                            crate::multiplexer::types::TabBacking::ChatBackend { .. } => true,
                            crate::multiplexer::types::TabBacking::Pty => tab.pty_id.is_some(),
                        },
                ),
                None => (false, None, None, None, None, None, None, None, false),
            }
        };
        let agent_accepting_work = chat_delivery_target
            && match state.as_ref() {
                Some(state) => matches!(
                    state.state,
                    crate::agent::offline_state_classifier::TimelineState::Working
                        | crate::agent::offline_state_classifier::TimelineState::Idle
                ),
                None => boot_stage == Some(codeterm_types::pty::BootStage::AgentReady),
            };
        let session_binding_valid = nonce.is_some() && nonce == reported_nonce;
        let live_agent = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).is_some_and(tab_has_live_agent)
        };
        TabProbe {
            exists,
            live_agent,
            agent_session_id: session_id,
            session_binding_valid,
            chat_delivery_target,
            agent_accepting_work,
            role,
            parent_tab_id,
        }
    }

    pub(super) fn emit_plan_changed(&self, plan_id: &str) {
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": plan_id }),
        );
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn live_attempt_session(task: &Task) -> Option<String> {
    task.execution_attempts
        .iter()
        .rev()
        .find(|attempt| attempt.closed_at.is_none())
        .and_then(|attempt| attempt.session_id.clone())
}

fn map_role(role: AgentRole) -> TabRole {
    match role {
        AgentRole::Orchestrator => TabRole::Orchestrator,
        AgentRole::Manager => TabRole::Worker,
        AgentRole::Worker => TabRole::Worker,
    }
}

fn drift_counts(items: &[DriftItem]) -> HashMap<DriftKind, usize> {
    let mut counts = HashMap::new();
    for item in items {
        *counts.entry(item.kind.clone()).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::TabBacking;

    #[test]
    fn tab_has_live_agent_requires_verified_session_and_supports_both_backings() {
        let (mut mux, tab_id) = MultiplexerState::new();
        assert!(!tab_has_live_agent(mux.get_tab(&tab_id).unwrap()));

        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("provider-neutral".into());
        tab.agent_session_id = Some("session-1".into());
        tab.session_binding_nonce = Some("nonce-1".into());
        tab.reported_session_binding_nonce = Some("nonce-1".into());
        tab.chat_view = true;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        tab.pty_id = Some("pty-1".into());
        assert!(tab_has_live_agent(tab));

        tab.exit_suspected_at = Some(std::time::Instant::now());
        assert!(!tab_has_live_agent(tab));
        tab.exit_suspected_at = None;

        tab.pty_id = None;
        tab.backing = TabBacking::ChatBackend {
            plugin_id: "plugin".into(),
            session_id: "session-1".into(),
        };
        assert!(tab_has_live_agent(tab));
    }
}
