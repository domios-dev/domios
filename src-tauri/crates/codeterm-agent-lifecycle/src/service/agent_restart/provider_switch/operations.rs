use super::*;
use parking_lot::{Condvar, Mutex};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub use codeterm_wire::api_contracts::ProviderSwitchStatus;

struct Operation {
    state: Mutex<ProviderSwitchStatus>,
    changed: Condvar,
    target: Mutex<Option<TargetBinding>>,
}

#[derive(Clone)]
pub(super) struct TargetBinding {
    provider: String,
    nonce: String,
    epoch: Option<u64>,
    operation: Option<String>,
    team: Option<String>,
    parent: Option<String>,
    role: Option<crate::multiplexer::types::AgentRole>,
    profile: Option<String>,
    task: Option<crate::multiplexer::types::TaskBinding>,
    ownership: Option<codeterm_types::pty::RemoteOwnership>,
}

impl TargetBinding {
    pub(super) fn matches(&self, tab: &Tab) -> bool {
        tab.agent_type.as_deref() == Some(self.provider.as_str())
            && tab.session_binding_nonce.as_deref() == Some(self.nonce.as_str())
            && tab.lifecycle_epoch == self.epoch
            && tab.lifecycle_operation_id == self.operation
            && tab.team_id == self.team && tab.parent_tab_id == self.parent
            && tab.role == self.role && tab.role_profile == self.profile
            && tab.task_binding == self.task && tab.ownership == self.ownership
    }

    pub(super) fn capture(svc: &ServiceContext, tab: &str, provider: &str, nonce: &str) -> Result<Self, String> {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab).filter(|tab| {
            tab.agent_type.as_deref() == Some(provider)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
        }).ok_or("provider_switch_superseded: target launch changed")?;
        Ok(Self {
            provider: provider.into(), nonce: nonce.into(), epoch: tab.lifecycle_epoch,
            operation: tab.lifecycle_operation_id.clone(),
            team: tab.team_id.clone(), parent: tab.parent_tab_id.clone(), role: tab.role,
            profile: tab.role_profile.clone(), task: tab.task_binding.clone(), ownership: tab.ownership.clone(),
        })
    }
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<Arc<Operation>>> = const { std::cell::RefCell::new(None) };
}

struct OperationScope(Option<Arc<Operation>>);

impl Drop for OperationScope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.replace(self.0.take()));
    }
}

const STAGES: &[&str] = &[
    "preflight", "source_retirement", "target_launch", "target_binding", "target_ready", "handoff",
];

pub(super) fn progress(svc: &ServiceContext, tab: &str, stage: &'static str) {
    CURRENT.with(|current| {
        let operation = current.borrow().clone();
        let Some(operation) = operation else { return };
        let status = {
            let mut state = operation.state.lock();
            if state.tab_id != tab || !active(&state) {
                return;
            }
            let next = STAGES.iter().position(|value| *value == stage);
            let previous = state.stage.as_deref().and_then(|stage| STAGES.iter().position(|value| *value == stage));
            if next.is_none() || next < previous {
                return;
            }
            state.stage = Some(stage.into());
            state.clone()
        };
        operation.changed.notify_all();
        emit_status(svc, &status);
    });
}

pub(super) fn target_launched(target: &TargetBinding) {
    CURRENT.with(|current| {
        let Some(operation) = current.borrow().clone() else { return };
        *operation.target.lock() = Some(target.clone());
    });
}

pub(super) fn owns_target(svc: &ServiceContext, tab: &str) -> bool {
    CURRENT.with(|current| {
        let Some(operation) = current.borrow().clone() else { return true };
        let target = operation.target.lock().clone();
        target.is_none_or(|target| svc.mux.read().get_tab(tab).is_some_and(|tab| target.matches(tab)))
    })
}

pub(super) fn await_target_binding(svc: &ServiceContext, tab: &str, target: &TargetBinding, timeout: Duration) -> Result<String, String> {
    let mut result = None;
    crate::service::boot_wait::wait_until(Instant::now() + timeout, || {
        let awaiting_binding = svc.mux.read().get_tab(tab).is_some_and(|current| {
            target.matches(current) && matches!(current.boot_stage, Some(codeterm_types::pty::BootStage::LaunchWritten | codeterm_types::pty::BootStage::ProviderReady | codeterm_types::pty::BootStage::AgentReady | codeterm_types::pty::BootStage::LaunchBindingUnconfirmed))
        });
        if awaiting_binding { progress(svc, tab, "target_binding"); }
        result = {
            let mux = svc.mux.read();
            match mux.get_tab(tab) {
                Some(current) if target.matches(current) => {
                    if current.launch_session_authenticated() && current.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady) {
                        current.agent_session_id.clone().map(Ok)
                    } else if current.boot_stage == Some(codeterm_types::pty::BootStage::SpawnFailed) {
                        Some(Err(format!("provider_switch_launch_failed: {}", current.boot_failure_reason.as_deref().unwrap_or("target launch failed"))))
                    } else { None }
                }
                _ => Some(Err("provider_switch_superseded: target launch changed; handoff was not sent".into())),
            }
        };
        result.is_some()
    });
    result.unwrap_or_else(|| Err(format!("provider_switch_readiness_unconfirmed: target {} nonce {} has no exact ready binding; launch may still bind; handoff was not sent and will not be replayed", target.provider, target.nonce)))
}

fn active(state: &ProviderSwitchStatus) -> bool {
    matches!(state.phase.as_str(), "pending" | "running")
}

#[derive(Default)]
pub struct ProviderSwitchOperations(Mutex<Vec<(String, Arc<Operation>)>>);

pub struct AcceptedProviderSwitch {
    pub status: ProviderSwitchStatus,
    pub response_complete: tokio::sync::oneshot::Sender<()>,
}

pub struct ProviderSwitchRequest {
    pub provider: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub force: bool,
}

pub trait ProviderSwitchOperationsExt {
    fn provider_switch_operations(&self) -> &ProviderSwitchOperations;
    fn accept_provider_switch(
        &self,
        tab: &str,
        request: ProviderSwitchRequest,
        caller: Option<&str>,
        caller_nonce: Option<&str>,
    ) -> Result<AcceptedProviderSwitch, String>;
    fn accept_provider_switch_with(
        &self,
        tab: &str,
        provider: String,
        caller: Option<&str>,
        caller_nonce: Option<&str>,
        run: impl FnOnce(&ServiceContext, &str, &str, bool) -> Result<ProviderSwitchOutcome, String>
            + Send
            + 'static,
    ) -> Result<AcceptedProviderSwitch, String>;
    fn provider_switch_status(
        &self,
        tab: &str,
        operation: &str,
        wait: bool,
    ) -> Result<ProviderSwitchStatus, String>;
    fn provider_switch_notice(&self, tab: &str) -> Option<crate::agent::chat_types::ChatMsg>;
}

impl ProviderSwitchOperationsExt for ServiceContext {
    fn provider_switch_operations(&self) -> &ProviderSwitchOperations {
        self.extensions
            .get_or_init(ProviderSwitchOperations::default)
    }

    fn accept_provider_switch(
        &self,
        tab: &str,
        request: ProviderSwitchRequest,
        caller: Option<&str>,
        caller_nonce: Option<&str>,
    ) -> Result<AcceptedProviderSwitch, String> {
        self.accept_provider_switch_with(
            tab,
            request.provider,
            caller,
            caller_nonce,
            move |svc, tab, provider, own| {
                svc.switch_provider_in_place(
                    tab,
                    provider,
                    request.model,
                    request.reasoning_effort,
                    request.force || own,
                )
            },
        )
    }

    fn accept_provider_switch_with(
        &self,
        tab: &str,
        provider: String,
        caller: Option<&str>,
        caller_nonce: Option<&str>,
        run: impl FnOnce(&ServiceContext, &str, &str, bool) -> Result<ProviderSwitchOutcome, String>
            + Send
            + 'static,
    ) -> Result<AcceptedProviderSwitch, String> {
        if let Some(refusal) = self.tab_write_lock(tab) {
            return Err(refusal.to_string());
        }
        let (tab, source, own) = {
            let mux = self.mux.read();
            let id = mux
                .resolve_tab_id(tab)
                .ok_or("provider_switch_not_found: tab not found")?;
            let tab = mux.get_tab(&id).ok_or("tab not found")?;
            let own = caller == Some(id.as_str());
            if own
                && (caller_nonce.is_none() || caller_nonce != tab.session_binding_nonce.as_deref())
            {
                return Err("provider_switch_caller_stale: self-switch requires the current session identity".into());
            }
            (
                id,
                tab.clone(),
                own,
            )
        };
        let status = ProviderSwitchStatus {
            operation_id: uuid::Uuid::new_v4().to_string(),
            tab_id: tab.clone(),
            provider: provider.clone(),
            phase: "pending".into(),
            stage: None,
            outcome: None,
            error: None,
        };
        let operation = Arc::new(Operation {
            state: Mutex::new(status.clone()),
            changed: Condvar::new(),
            target: Mutex::new(None),
        });
        {
            let mut operations = self.provider_switch_operations().0.lock();
            if operations.iter().any(|(_, entry)| {
                let entry = entry.state.lock();
                entry.tab_id == tab && matches!(entry.phase.as_str(), "pending" | "running")
            }) {
                return Err(
                    "provider_switch_in_progress: this tab already has a switch operation".into(),
                );
            }
            if operations.len() >= 128 {
                operations.retain(|(_, entry)| {
                    matches!(entry.state.lock().phase.as_str(), "pending" | "running")
                });
            }
            if operations.len() >= 128 {
                return Err("provider_switch_capacity: too many active operations".into());
            }
            operations.push((status.operation_id.clone(), operation.clone()));
        }
        emit_status(self, &status);
        let (response_complete, response_sent) = tokio::sync::oneshot::channel();
        let svc = self.clone();
        self.rt.spawn(async move {
            let _ = response_sent.await;
            let failed = operation.clone();
            let events = svc.clone();
            let result = tokio::task::spawn_blocking(move || {
                let _scope = OperationScope(CURRENT.with(|current| current.replace(Some(operation.clone()))));
                publish_phase(&svc, &operation, "running");
                progress(&svc, &tab, "preflight");
                let current = svc.mux.read().get_tab(&tab).cloned();
                let result = match current {
                    Some(current)
                        if current.agent_type == source.agent_type
                            && current.agent_session_id == source.agent_session_id
                            && current.session_binding_nonce == source.session_binding_nonce
                            && current.team_id == source.team_id
                            && current.parent_tab_id == source.parent_tab_id
                            && current.role == source.role && current.role_profile == source.role_profile
                            && current.task_binding == source.task_binding && current.ownership == source.ownership
                            && current.cwd == source.cwd && current.backing == source.backing =>
                    {
                        run(&svc, &tab, &provider, own)
                    }
                    _ => Err(
                        "provider_switch_superseded: source identity changed after acceptance"
                            .into(),
                    ),
                };
                complete(&svc, &operation, result);
            })
            .await;
            if let Err(error) = result {
                complete(
                    &events,
                    &failed,
                    Err(format!("provider_switch_worker_failed: {error}")),
                );
            }
        });
        Ok(AcceptedProviderSwitch {
            status,
            response_complete,
        })
    }

    fn provider_switch_status(
        &self,
        tab: &str,
        operation: &str,
        wait: bool,
    ) -> Result<ProviderSwitchStatus, String> {
        let operation = self
            .provider_switch_operations()
            .0
            .lock()
            .iter()
            .find(|(id, _)| id == operation)
            .map(|(_, entry)| entry.clone())
            .ok_or("provider_switch_operation_not_found")?;
        let id = self.mux.read().resolve_tab_id(tab).ok_or("tab not found")?;
        if operation.state.lock().tab_id != id {
            return Err("provider_switch_operation_not_found".into());
        }
        let state = wait_status(&operation, wait, Instant::now() + Duration::from_secs(180));
        if state.phase == "failed" && state.error.as_deref().is_some_and(|error| error.starts_with("provider_switch_readiness_unconfirmed:")) {
            reconcile_late_binding(self, &operation);
        }
        let status = operation.state.lock().clone();
        Ok(status)
    }

    fn provider_switch_notice(&self, tab: &str) -> Option<crate::agent::chat_types::ChatMsg> {
        let operations = self.provider_switch_operations().0.lock();
        let state = operations.iter().rev().find_map(|(_, operation)| {
            let state = operation.state.lock();
            (state.tab_id == tab).then(|| state.clone())
        })?;
        Some(crate::agent::chat_types::ChatMsg {
            id: format!("provider-switch-{}", state.operation_id),
            kind: "system".into(),
            content: match state.outcome {
                Some(outcome) => format!(
                    "Provider switched to {}. Previous provider transcript: {}",
                    outcome.provider, outcome.previous_history_path
                ),
                None => format!(
                    "Provider switch {}: {}{}",
                    state.operation_id,
                    state.phase,
                    state
                        .error
                        .map(|error| format!(" — {error}"))
                        .unwrap_or_default()
                ),
            },
            ..Default::default()
        })
    }
}

fn complete(
    svc: &ServiceContext,
    operation: &Operation,
    result: Result<ProviderSwitchOutcome, String>,
) {
    let (status, observation) = {
        let mut state = operation.state.lock();
        if !active(&state) {
            return;
        }
        let result = result.and_then(|outcome| {
            let target = operation.target.lock();
            let current = svc.mux.read();
            let bound = current.get_tab(&state.tab_id).is_some_and(|tab| {
                outcome.tab_id == state.tab_id && outcome.provider == state.provider
                    && tab.agent_type.as_deref() == Some(outcome.provider.as_str())
                    && tab.agent_session_id.as_deref() == Some(outcome.session_id.as_str())
                    && tab.launch_session_authenticated()
                    && target.as_ref().is_none_or(|target| target.matches(tab))
            });
            if !bound || !outcome.binding_restored || !outcome.handoff_delivered {
                return Err("provider_switch_completion_unconfirmed: exact current target binding and handoff are required; input will not be replayed".into());
            }
            Ok(outcome)
        });
        let observation = result.as_ref().ok().cloned();
        match result {
            Ok(outcome) => {
                state.phase = "completed".into();
                state.outcome = Some(outcome);
            }
            Err(error) => {
                state.phase = "failed".into();
                state.error = Some(error);
            }
        }
        (state.clone(), observation)
    };
    if let Some(outcome) = observation {
        svc.record_product_provider_switch(&outcome);
    }
    operation.changed.notify_all();
    emit_status(svc, &status);
    svc.emit_state_changed(crate::service::MuxEmitCause::AgentRestart);
}

fn reconcile_late_binding(svc: &ServiceContext, operation: &Operation) {
    let target = operation.target.lock().clone();
    let Some(target) = target else { return };
    let mut state = operation.state.lock();
    if state.phase != "failed" || !state.error.as_deref().is_some_and(|error| error.starts_with("provider_switch_readiness_unconfirmed:")) {
        return;
    }
    let mux = svc.mux.read();
    let Some(tab) = mux.get_tab(&state.tab_id).filter(|tab| target.matches(tab)
        && tab.launch_session_authenticated()
        && tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady)) else { return };
    if state.stage.as_deref() == Some("target_ready") {
        return;
    }
    state.stage = Some("target_ready".into());
    state.error = Some(format!("{}; late exact target session {} is ready; handoff was not sent and will not be replayed", state.error.as_deref().unwrap_or_default(), tab.agent_session_id.as_deref().unwrap_or("unknown")));
    let status = state.clone();
    drop(mux);
    drop(state);
    emit_status(svc, &status);
}

fn publish_phase(svc: &ServiceContext, operation: &Operation, phase: &str) {
    let status = {
        let mut state = operation.state.lock();
        if !active(&state) {
            return;
        }
        state.phase = phase.into();
        state.clone()
    };
    emit_status(svc, &status);
}

fn wait_status(operation: &Operation, wait: bool, deadline: Instant) -> ProviderSwitchStatus {
    let mut state = operation.state.lock();
    while wait && active(&state) {
        if operation.changed.wait_until(&mut state, deadline).timed_out() {
            break;
        }
    }
    state.clone()
}

fn emit_status(svc: &ServiceContext, status: &ProviderSwitchStatus) {
    svc.ui.emit(
        "provider-switch-status",
        serde_json::to_value(status).unwrap_or_default(),
    );
}

#[cfg(test)]
mod tests;
