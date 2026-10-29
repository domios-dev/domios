use crate::service::launch_input_gate::LaunchInputEvidence;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::{MuxEmitCause, ServiceContext};

pub(super) fn awaiting_user_turn(lazy: bool, resuming: bool, first_turn_planned: bool) -> bool {
    lazy && !resuming && !first_turn_planned
}

pub(super) fn prepare(
    svc: &ServiceContext,
    attempt: &LaunchAttempt,
    provider: &str,
    resuming: bool,
    first_turn_planned: bool,
) -> bool {
    let lazy = svc
        .agent_registry
        .with_provider(provider, |provider| {
            provider.session_starts_on_first_user_turn()
        })
        .unwrap_or(false);
    if !awaiting_user_turn(lazy, resuming, first_turn_planned) {
        return false;
    }
    if bound(svc, attempt) {
        return false;
    }
    let timeout = svc
        .agent_registry
        .with_provider(provider, |provider| {
            provider.provider_readiness_strategy().timeout()
        })
        .unwrap_or(std::time::Duration::from_secs(15));
    match svc.await_lazy_session_input_readiness(attempt, provider, timeout) {
        Ok(LaunchInputEvidence::SessionBinding) => false,
        Ok(LaunchInputEvidence::ProviderComposer) => {
            if bound(svc, attempt) {
                return false;
            }
            publish(svc, attempt);
            true
        }
        Err(crate::service::launch_input_gate::LaunchInputRefusal::AttemptSuperseded) => true,
        Err(reason) => {
            if finish_booting(svc, attempt) {
                svc.record_provisional_launch(
                    attempt,
                    crate::service::boot_stage::BootStage::LaunchBindingUnconfirmed,
                    &reason.reason(),
                );
                svc.emit_state_changed(MuxEmitCause::GeneralAgent);
            }
            true
        }
    }
}

fn bound(svc: &ServiceContext, attempt: &LaunchAttempt) -> bool {
    svc.mux.read().get_tab(&attempt.tab_id).is_some_and(|tab| {
        crate::service::launch_readiness::attempt_owns_tab(tab, attempt)
            && tab.launch_session_authenticated()
    })
}

fn finish_booting(svc: &ServiceContext, attempt: &LaunchAttempt) -> bool {
    let mut mux = svc.mux.write();
    let Some(tab) = mux.get_tab_mut(&attempt.tab_id) else {
        return false;
    };
    if !crate::service::launch_readiness::attempt_owns_tab(tab, attempt) {
        return false;
    }
    tab.agent_booting = false;
    true
}

fn publish(svc: &ServiceContext, attempt: &LaunchAttempt) {
    if !finish_booting(svc, attempt) {
        return;
    }
    svc.record_first_turn_input_ready(attempt, "provider composer accepts the first user turn");
    svc.wake_parked_user_send(&attempt.tab_id);
    svc.emit_state_changed(MuxEmitCause::GeneralAgent);
}

#[cfg(test)]
mod tests;
