mod worker_binding;

use super::{AgentRestartError, RestartAttemptError, RestartOutcome, TabRestartSnapshot};
use crate::multiplexer::types::TabBacking;
use crate::service::{agent_runtime, ServiceContext};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RestartRoute {
    Runtime,
    Pty,
}

pub(super) fn restart_route(runtime_declared: bool, backing: &TabBacking) -> RestartRoute {
    if runtime_declared && matches!(backing, TabBacking::ChatBackend { .. }) {
        RestartRoute::Runtime
    } else {
        RestartRoute::Pty
    }
}

impl ServiceContext {
    pub(super) fn require_restart_provider_handoff(
        &self,
        source: &str,
        target: &str,
    ) -> Result<(), AgentRestartError> {
        if source != target
            && (agent_runtime::engine::requested(self, source)
                || agent_runtime::engine::requested(self, target))
        {
            return Err(AgentRestartError::RuntimeProviderHandoffUnsupported);
        }
        Ok(())
    }

    pub(super) fn restart_runtime_agent(
        &self,
        tab_id: &str,
        force: bool,
        fresh: bool,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.require_background_work_safe(tab_id, force)?;
        let _claim = self.claim_restart(tab_id, false)?;
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {tab_id} not found")))?;
        let provider = tab
            .agent_type
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("Runtime provider absent".into()))?;
        if restart_route(
            agent_runtime::engine::requested(self, &provider),
            &tab.backing,
        ) != RestartRoute::Runtime
        {
            return Err(AgentRestartError::BadRequest("Runtime backing changed".into()).into());
        }
        if !fresh && tab.agent_session_id.as_deref().is_none_or(str::is_empty) {
            return Err(AgentRestartError::BadRequest(
                "Runtime thread identity missing; an explicit --fresh action is required".into(),
            )
            .into());
        }
        let snap = TabRestartSnapshot {
            tab_id: tab.id.clone(),
            pty_id: String::new(),
            provider: provider.clone(),
            target_provider: None,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve: tab.restorable_launch.permission_bypass_requested,
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
            handoff_history: None,
        };
        if fresh {
            self.require_fresh_restart_allowed(&snap, force)?;
        }
        let nonce = tab
            .session_binding_nonce
            .ok_or_else(|| AgentRestartError::BadRequest("Runtime binding nonce absent".into()))?;
        if !agent_runtime::authority::verifies(self, tab_id, &provider, &nonce) {
            return Err(AgentRestartError::BadRequest(
                "Runtime restart authority unavailable".into(),
            )
            .into());
        }
        self.authorize_runtime_restart_binding(tab_id, &provider, &nonce, fresh)
            .map_err(AgentRestartError::BadRequest)?;
        let mut outcome = RestartOutcome::accepted(&snap, fresh, force);
        if fresh {
            outcome.session_id = None;
        }
        let result = crate::async_runtime::block_on(agent_runtime::restart::restart(
            self, tab_id, &provider, &nonce, fresh,
        ));
        match result {
            Ok(session) => {
                let context = session.context.lock().clone();
                if !session
                    .thread_loaded
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    let failure = agent_runtime::model::RuntimeFailure::Refused(
                        "Runtime thread was not loaded".into(),
                    );
                    let epoch = session.state.lock().epoch;
                    session.state.lock().degrade(epoch, &failure);
                    agent_runtime::engine::publish(self, tab_id, &session);
                    self.emit_state_changed();
                    return Err(RestartAttemptError::Failed {
                        outcome,
                        reason: failure.to_string(),
                        timed_out: false,
                    });
                }
                outcome.session_id = context.thread_id;
                outcome.agent_ready = true;
                if let Err(reason) =
                    self.restore_caller_binding(tab_id, outcome.session_id.as_deref())
                {
                    let failure = match reason {
                        crate::service::restored_binding::RestoreBindingError::RebindFailed(
                            reason,
                        ) => agent_runtime::plan_warning::PlanBindingFailure::bookkeeping(reason),
                        reason => agent_runtime::plan_warning::PlanBindingFailure::from(
                            reason.to_string(),
                        ),
                    };
                    self.warn_runtime_plan_binding(
                        tab_id,
                        agent_runtime::plan_warning::PlanBindingWarningStage::RestartCallerRestoration,
                        &failure,
                    );
                } else {
                    outcome.binding_restored = true;
                }
                self.schedule_session_persist_debounced();
                self.emit_state_changed();
                Ok(outcome)
            }
            Err(reason) => Err(RestartAttemptError::Failed {
                outcome,
                timed_out: matches!(reason, agent_runtime::model::RuntimeFailure::Deadline),
                reason: reason.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests;
