use crate::service::{inject_verify::InjectionError, ServiceContext};

pub(crate) const DELIVERY_MODAL_OPEN: &str = "delivery_modal_open";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PayloadDecision {
    Write,
    HoldModal,
}

pub(super) fn payload_decision(delivery_modal_visible: bool) -> PayloadDecision {
    if delivery_modal_visible {
        PayloadDecision::HoldModal
    } else {
        PayloadDecision::Write
    }
}

pub(super) fn submit_key_readiness(delivery_modal_visible: bool) -> bool {
    payload_decision(delivery_modal_visible) == PayloadDecision::Write
}

pub(super) fn exact_session_turn_source_available(
    require_exact_session_turn: bool,
    has_anchor: bool,
) -> bool {
    require_exact_session_turn && has_anchor
}

pub(crate) fn observe_delivery_readiness(
    svc: &ServiceContext,
    pty_id: &str,
) -> Result<bool, InjectionError> {
    delivery_readiness(svc, pty_id, true).map(|(_, changed)| changed)
}

pub(super) fn pty_owner_guard(svc: &ServiceContext, pty_id: &str) -> Result<(), InjectionError> {
    let (modal_visible, _) = delivery_readiness(svc, pty_id, false)?;
    if !submit_key_readiness(modal_visible) {
        Err(InjectionError::AgentTuiPresent {
            pty_id: pty_id.into(),
        })
    } else {
        Ok(())
    }
}

fn delivery_readiness(
    svc: &ServiceContext,
    pty_id: &str,
    observe: bool,
) -> Result<(bool, bool), InjectionError> {
    let target = {
        let mux = svc.mux.read();
        mux.tab_id_for_pty(pty_id).and_then(|tab_id| {
            mux.get_tab(&tab_id).and_then(|tab| {
                (tab.pty_id.as_deref() == Some(pty_id)).then(|| {
                    (
                        tab_id,
                        tab.agent_type.clone(),
                        tab.agent_session_id.clone(),
                        tab.cwd.clone(),
                    )
                })
            })
        })
    };
    let Some((tab_id, provider_id, session_id, cwd)) = target else {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    };
    let Some(provider_id) = provider_id else {
        return Ok((false, false));
    };
    let snapshot = crate::agent::tab_state::TabStateSnapshot {
        tab_id: tab_id.clone(),
        agent_type: provider_id.clone(),
        session_id,
        cwd,
        screen_text: svc
            .screen_state
            .lock()
            .get_screen_text(pty_id)
            .unwrap_or_default(),
    };
    let modal_visible = svc
        .agent_registry
        .with_provider(&provider_id, |provider| {
            provider.delivery_modal_visible(&snapshot)
        })
        .unwrap_or(false);
    let generation = svc.screen_state.lock().pty_generation(pty_id);
    let readiness = crate::service::output_feed::delivery_readiness::DeliveryReadiness {
        pty_id: pty_id.into(),
        generation,
        modal_visible,
    };
    let events = &svc.pending_delivery_payloads.readiness_events;
    let changed = if observe {
        events.observe(&tab_id, readiness)
    } else {
        events.prime(&tab_id, readiness);
        false
    };
    Ok((modal_visible, changed))
}
