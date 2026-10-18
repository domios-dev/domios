use crate::service::codex_rpc::lifecycle::{self, DeliveryState};
use crate::service::launch_input_gate::LaunchInputEvidence;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::ServiceContext;

fn should_latch_user_input(
    lazy_session: bool,
    evidence: LaunchInputEvidence,
    current_launch: bool,
    plain: bool,
) -> bool {
    lazy_session && evidence == LaunchInputEvidence::ProviderComposer && current_launch && plain
}

pub(super) fn requires_composer(svc: &ServiceContext, provider: &str, tab: &str) -> bool {
    svc.agent_registry
        .with_provider(provider, |provider| {
            provider.session_starts_on_first_user_turn()
        })
        .unwrap_or(false)
        && lifecycle::delivery_state(tab) == DeliveryState::Plain
        && lifecycle::for_tab(tab).is_none()
}

pub(super) fn observe(
    svc: &ServiceContext,
    provider: &str,
    attempt: &LaunchAttempt,
    evidence: LaunchInputEvidence,
) {
    let lazy_session = svc
        .agent_registry
        .with_provider(provider, |provider| {
            provider.session_starts_on_first_user_turn()
        })
        .unwrap_or(false);
    let current_launch = LaunchAttempt::current(&svc.mux, &attempt.tab_id, &attempt.pty_id)
        .as_ref()
        == Some(attempt);
    let plain = lifecycle::delivery_state(&attempt.tab_id) == DeliveryState::Plain
        && lifecycle::for_tab(&attempt.tab_id).is_none();
    if !should_latch_user_input(lazy_session, evidence, current_launch, plain) {
        return;
    }
    let Ok(authority) = svc.user_send_authority(&attempt.tab_id) else {
        return;
    };
    if authority.pty_id.as_deref() != Some(attempt.pty_id.as_str())
        || authority.binding_nonce != attempt.binding_nonce
    {
        return;
    }
    if svc.latch_user_input_authority(&authority) {
        log::info!("[user-input-readiness] tab={} pty={} evidence=provider_composer supervision=session_binding", attempt.tab_id, attempt.pty_id);
        svc.wake_parked_user_send(&attempt.tab_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parked_user_delivery_lazy_first_input_waits_for_provider_composer() {
        assert!(should_latch_user_input(
            true,
            LaunchInputEvidence::ProviderComposer,
            true,
            true
        ));
        assert!(!should_latch_user_input(
            true,
            LaunchInputEvidence::SessionBinding,
            true,
            true
        ));
    }

    #[test]
    fn lazy_user_input_does_not_latch_foreign_launch_or_structured_sidecar() {
        for (lazy, current, plain) in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            assert!(!should_latch_user_input(
                lazy,
                LaunchInputEvidence::ProviderComposer,
                current,
                plain
            ));
        }
    }
}
