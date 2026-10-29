use crate::service::launch_input_gate::LaunchInputEvidence;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::{MuxEmitCause, ServiceContext};

pub(super) fn awaiting_user_turn(lazy: bool, resuming: bool, first_turn_planned: bool) -> bool {
    lazy && !resuming && !first_turn_planned
}

pub(super) fn prepare(svc: &crate::ServiceState, tab: &str, pty: &str, provider: &str) -> bool {
    let Some(attempt) = LaunchAttempt::current(&svc.mux, tab, pty) else {
        return true;
    };
    let timeout = svc
        .agent_registry
        .with_provider(provider, |provider| {
            provider.provider_readiness_strategy().timeout()
        })
        .unwrap_or(std::time::Duration::from_secs(15));
    match svc.await_lazy_session_input_readiness(&attempt, provider, timeout) {
        Ok(LaunchInputEvidence::SessionBinding) => false,
        Ok(LaunchInputEvidence::ProviderComposer) => {
            publish(svc, &attempt);
            true
        }
        Err(reason) => {
            super::defer_general_agent_start(
                svc,
                &svc.mux,
                tab,
                pty,
                crate::service::boot_stage::BootStage::LaunchBindingUnconfirmed,
                reason.reason(),
            );
            true
        }
    }
}

fn publish(svc: &ServiceContext, attempt: &LaunchAttempt) {
    svc.record_first_turn_input_ready(attempt, "provider composer accepts the first user turn");
    if let Some(tab) = svc.mux.write().get_tab_mut(&attempt.tab_id) {
        if tab.pty_id.as_deref() == Some(attempt.pty_id.as_str())
            && tab.session_binding_nonce.as_deref() == Some(attempt.binding_nonce.as_str())
        {
            tab.agent_booting = false;
        }
    }
    svc.wake_parked_user_send(&attempt.tab_id);
    svc.emit_state_changed(MuxEmitCause::GeneralAgent);
}

#[cfg(test)]
mod tests;
