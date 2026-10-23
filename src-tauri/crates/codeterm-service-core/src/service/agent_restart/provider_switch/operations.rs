use super::*;
use parking_lot::{Condvar, Mutex};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, serde::Serialize)]
pub struct ProviderSwitchStatus {
    pub operation_id: String,
    pub tab_id: String,
    pub provider: String,
    pub phase: String,
    pub outcome: Option<ProviderSwitchOutcome>,
    pub error: Option<String>,
}

struct Operation {
    state: Mutex<ProviderSwitchStatus>,
    changed: Condvar,
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

impl ServiceContext {
    fn provider_switch_operations(&self) -> &ProviderSwitchOperations {
        self.extensions
            .get_or_init(ProviderSwitchOperations::default)
    }

    pub fn accept_provider_switch(
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
        let (tab, source, nonce, own) = {
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
                tab.agent_type.clone(),
                tab.session_binding_nonce.clone(),
                own,
            )
        };
        let status = ProviderSwitchStatus {
            operation_id: uuid::Uuid::new_v4().to_string(),
            tab_id: tab.clone(),
            provider: provider.clone(),
            phase: "pending".into(),
            outcome: None,
            error: None,
        };
        let operation = Arc::new(Operation {
            state: Mutex::new(status.clone()),
            changed: Condvar::new(),
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
        let (response_complete, response_sent) = tokio::sync::oneshot::channel();
        let svc = self.clone();
        self.rt.spawn(async move {
            let _ = response_sent.await;
            let failed = operation.clone();
            let events = svc.clone();
            let result = tokio::task::spawn_blocking(move || {
                operation.state.lock().phase = "running".into();
                let current = svc.mux.read().get_tab(&tab).cloned();
                let result = match current {
                    Some(current)
                        if current.agent_type == source
                            && current.session_binding_nonce == nonce =>
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

    pub fn provider_switch_status(
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
        let mut state = operation.state.lock();
        let id = self.mux.read().resolve_tab_id(tab).ok_or("tab not found")?;
        if state.tab_id != id {
            return Err("provider_switch_operation_not_found".into());
        }
        let deadline = Instant::now() + Duration::from_secs(180);
        while wait && matches!(state.phase.as_str(), "pending" | "running") {
            if operation
                .changed
                .wait_until(&mut state, deadline)
                .timed_out()
            {
                break;
            }
        }
        Ok(state.clone())
    }

    pub(crate) fn provider_switch_notice(
        &self,
        tab: &str,
    ) -> Option<crate::agent::chat_types::ChatMsg> {
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
    let payload = {
        let mut state = operation.state.lock();
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
        serde_json::to_value(&*state).unwrap_or_default()
    };
    operation.changed.notify_all();
    svc.ui.emit("provider-switch-status", payload);
    svc.emit_state_changed(crate::service::MuxEmitCause::AgentRestart);
}

#[cfg(test)]
mod tests;
