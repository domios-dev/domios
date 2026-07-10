use std::collections::HashMap;

use serde::Serialize;

use super::ServiceContext;
use crate::multiplexer::types::AgentRole;
use crate::plan::cleanup_debt::CleanupDebtTask;
use crate::plan::lane_liveness::LaneThresholds;
use crate::plan::reconcile::{
    detect_lane_demotions, detect_pane_gone, detect_plan_drift, DriftItem, DriftKind, PaneProbe,
    PaneRole, ReconcileConfig, DEFAULT_STALE_REVIEW_MS,
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

    /// Periodic watchdog: release in_progress lanes whose pane is gone past the
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
            config.current_orchestrator_pane_id = plan.owner.pane_id.clone();
            let expected: HashMap<String, String> = plan
                .tasks
                .iter()
                .filter(|task| task.status == TaskStatus::InProgress)
                .filter_map(|task| {
                    Some((task.assigned_pane_id.clone()?, live_attempt_session(task)?))
                })
                .collect();
            let demotions = detect_lane_demotions(&plan, &config, &thresholds, |pane_id| {
                self.lane_pane_probe(pane_id, expected.get(pane_id).map(String::as_str))
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

    // Probe keyed off the lane's OWN session: a recycled pane id now hosting a
    // different agent reads as gone, not live.
    fn lane_pane_probe(&self, pane_id: &str, expected_session: Option<&str>) -> PaneProbe {
        let mut probe = self.pane_probe(pane_id);
        if probe.exists {
            if let Some(expected) = expected_session {
                let same = {
                    let mux = self.mux.read();
                    mux.get_pane(pane_id)
                        .and_then(|pane| pane.agent_session_id.clone())
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

    pub(crate) fn pane_probe(&self, pane_id: &str) -> PaneProbe {
        let (
            exists,
            role,
            parent_pane_id,
            agent_type,
            session_id,
            nonce,
            reported_nonce,
            state,
            boot_stage,
            chat_delivery_target,
        ) = {
            let mux = self.mux.read();
            match mux.get_pane(pane_id) {
                Some(pane) => (
                    true,
                    pane.role.map(map_role),
                    pane.parent_pane_id.clone(),
                    pane.agent_type.clone(),
                    pane.agent_session_id.clone(),
                    pane.session_binding_nonce.clone(),
                    pane.reported_session_binding_nonce.clone(),
                    pane.agent_state.clone(),
                    pane.boot_stage,
                    pane.chat_view
                        && match &pane.backing {
                            crate::multiplexer::types::PaneBacking::ChatBackend { .. } => true,
                            crate::multiplexer::types::PaneBacking::Pty => pane.pty_id.is_some(),
                        },
                ),
                None => (false, None, None, None, None, None, None, None, None, false),
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
        let live_agent = agent_type.is_some()
            && session_id.is_some()
            && session_binding_valid
            && chat_delivery_target
            && agent_accepting_work;
        PaneProbe {
            exists,
            live_agent,
            agent_session_id: session_id,
            session_binding_valid,
            chat_delivery_target,
            agent_accepting_work,
            role,
            parent_pane_id,
        }
    }

    fn emit_plan_changed(&self, plan_id: &str) {
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

fn map_role(role: AgentRole) -> PaneRole {
    match role {
        AgentRole::Orchestrator => PaneRole::Orchestrator,
        AgentRole::Manager => PaneRole::Worker,
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
