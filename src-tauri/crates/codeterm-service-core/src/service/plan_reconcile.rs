use std::collections::HashMap;

use serde::Serialize;

use super::ServiceContext;

pub fn tab_has_live_agent(tab: &crate::multiplexer::types::Tab) -> bool {
    let delivery_target = tab.chat_view
        && match &tab.backing {
            crate::multiplexer::types::TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } => {
                tab.agent_type.as_deref() == Some(plugin_id.as_str())
                    && tab.agent_session_id.as_deref() == Some(session_id.as_str())
                    && (tab.agent_runtime_state.is_none()
                        || super::agent_runtime::engine::has_live_bound_session(tab))
            }
            crate::multiplexer::types::TabBacking::Pty => tab.pty_id.is_some(),
            crate::multiplexer::types::TabBacking::Surface(_) => false,
        };
    tab.agent_type.is_some()
        && tab.agent_session_id.is_some()
        && tab.session_binding_nonce.is_some()
        && tab.session_binding_nonce == tab.reported_session_binding_nonce
        && tab.exit_suspected_at.is_none()
        && delivery_target
}
use crate::multiplexer::types::AgentRole;
use crate::plan::cleanup_debt::CleanupDebtTask;
use crate::plan::lane_liveness::LaneThresholds;
use crate::plan::reconcile::{
    detect_lane_demotions, detect_plan_drift, DriftItem, DriftKind, ReconcileConfig, TabProbe,
    TabRole, DEFAULT_STALE_REVIEW_MS,
};
use crate::plan::TaskStatus;
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
        let read = self.plan_store.status_projection(plan_id, now_ms)?;
        self.reconcile_plan_record(read.input(), fix, caller_tab_id, now_ms)
    }

    pub(crate) fn reconcile_plan_record(
        &self,
        plan: &codeterm_plan::service_plan::state::input::StateInput,
        fix: bool,
        caller_tab_id: Option<&str>,
        now_ms: u64,
    ) -> Result<ReconcileResult, PlanError> {
        let plan_id = plan.id.as_str();
        let mut config = ReconcileConfig::new(now_ms);
        config.stale_review_ms = DEFAULT_STALE_REVIEW_MS;
        config.current_orchestrator_tab_id = caller_tab_id
            .map(str::to_string)
            .or_else(|| plan.owner.tab_id.clone());
        let drift = detect_plan_drift(plan, &config, |tab_id| self.tab_probe(tab_id));
        let cleanup_debt =
            cleanup_debt::detect_plan_cleanup_debt(plan, |tab_id| self.tab_probe(tab_id).exists);
        let counts = drift_counts(&drift);
        let fixed = if fix {
            let gone_attempts = plan
                .tasks
                .iter()
                .flat_map(|task| {
                    let task_id = task.id.clone();
                    task.execution_attempts
                        .iter()
                        .filter(|attempt| attempt.closed_at.is_none())
                        .filter(move |attempt| !self.tab_probe(&attempt.executor_tab_id).exists)
                        .map(move |attempt| (task_id.clone(), attempt.executor_tab_id.clone()))
                })
                .collect::<Vec<_>>();
            let mut fixed = self
                .plan_store
                .apply_reconcile_fixes(plan_id, &drift, now_ms)?;
            fixed.extend(self.plan_store.close_gone_task_attempts(
                plan_id,
                &gone_attempts,
                now_ms,
            )?);
            fixed
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
        let losses = match self.worker_losses_for_tab(
            tab_id,
            crate::service::worker_loss::WorkerLossCause::TabGone,
        ) {
            Ok(losses) => losses,
            Err(error) => {
                log::error!("[worker-lost] assignment read failed before reconciliation: {error}");
                return;
            }
        };
        if let Err(error) = self.flag_bound_tasks_without_report(tab_id, now_ms) {
            log::warn!("[plan-reconcile] task binding lookup failed for {tab_id}: {error}");
            return;
        }
        self.report_worker_losses(losses);
    }

    fn flag_bound_tasks_without_report(
        &self,
        tab_id: &str,
        now_ms: u64,
    ) -> Result<Vec<String>, super::plan::PlanError> {
        let bindings = self.plan_store.task_bindings_for_tab(tab_id)?;
        if bindings.is_empty() {
            return Ok(Vec::new());
        }
        let mut changed_plans = Vec::new();
        for (plan_id, task_id) in bindings {
            match self
                .plan_store
                .flag_tab_gone(&plan_id, &task_id, tab_id, now_ms)
            {
                Ok(true) => changed_plans.push(plan_id),
                Ok(false) => {}
                Err(e) => log::warn!(
                    "[plan-reconcile] failed to flag tab-gone for {plan_id}/{task_id}: {e}"
                ),
            }
        }
        changed_plans.sort();
        changed_plans.dedup();
        for plan_id in &changed_plans {
            self.emit_plan_changed(plan_id);
        }
        Ok(changed_plans)
    }

    /// Periodic watchdog: release in_progress lanes whose tab is gone past the
    /// grace window back to pending so the Board stops showing phantom active
    /// work. Runs on the reminder-scheduler cadence, not a busy loop.
    pub fn run_lane_watchdog(&self) {
        let now_ms = now_ms();
        let thresholds = LaneThresholds::default();
        let mut changed_plans = Vec::new();
        let plans = match self.plan_store.lane_watchdog_projection() {
            Ok(plans) => plans,
            Err(error) => {
                log::error!("[lane-watchdog] cannot read candidates: {error}");
                return;
            }
        };
        for plan in plans {
            let mut config = ReconcileConfig::new(now_ms);
            config.current_orchestrator_tab_id = plan.owner_tab_id.clone();
            let expected: HashMap<String, String> = plan
                .tasks
                .iter()
                .filter(|task| task.status == TaskStatus::InProgress)
                .filter_map(|task| {
                    Some((
                        task.assigned_tab_id.clone()?,
                        task.expected_session.clone()?,
                    ))
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

    pub fn tab_probe(&self, tab_id: &str) -> TabProbe {
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
                    tab.role
                        .map(|role| map_role(role, tab.role_profile.as_deref())),
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
                            crate::multiplexer::types::TabBacking::Surface(_) => false,
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

    pub fn emit_plan_changed(&self, plan_id: &str) {
        self.provider_update_task_state_changed();
        self.emit_committed_plan_changed(plan_id);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
fn live_attempt_session_for_assignment(
    assigned_tab_id: Option<&str>,
    attempts: &[crate::plan::TaskExecutionAttempt],
) -> Option<String> {
    let assigned_tab_id = assigned_tab_id
        .map(str::trim)
        .filter(|tab_id| !tab_id.is_empty())?;
    let mut matching = attempts
        .iter()
        .filter(|attempt| attempt.closed_at.is_none())
        .filter(|attempt| attempt.executor_tab_id == assigned_tab_id);
    let attempt = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    attempt
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_string)
}

fn map_role(role: AgentRole, role_profile: Option<&str>) -> TabRole {
    if codeterm_types::agent::role_kind::anchors_team(Some(role), role_profile) {
        return TabRole::Orchestrator;
    }
    TabRole::Worker
}

fn drift_counts(items: &[DriftItem]) -> HashMap<DriftKind, usize> {
    let mut counts = HashMap::new();
    for item in items {
        *counts.entry(item.kind.clone()).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests;
