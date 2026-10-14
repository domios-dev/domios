use crate::service::{inject_verify::InjectionError, ServiceContext};

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
    let target = owned_target(svc, pty_id)?;
    Ok(svc
        .pending_delivery_payloads
        .readiness_events
        .observe(&target.tab_id, readiness(svc, pty_id)))
}

pub(super) fn pty_owner_guard(svc: &ServiceContext, pty_id: &str) -> Result<(), InjectionError> {
    let target = owned_target(svc, pty_id)?;
    svc.pending_delivery_payloads
        .readiness_events
        .prime(&target.tab_id, readiness(svc, pty_id));
    Ok(())
}

struct OwnedTarget {
    tab_id: String,
}

fn owned_target(svc: &ServiceContext, pty_id: &str) -> Result<OwnedTarget, InjectionError> {
    let mux = svc.mux.read();
    mux.tab_id_for_pty(pty_id)
        .and_then(|tab_id| {
            mux.get_tab(&tab_id).and_then(|tab| {
                (tab.pty_id.as_deref() == Some(pty_id)).then(|| OwnedTarget {
                    tab_id: tab_id.clone(),
                })
            })
        })
        .ok_or_else(|| InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        })
}

fn readiness(
    svc: &ServiceContext,
    pty_id: &str,
) -> crate::service::output_feed::delivery_readiness::DeliveryReadiness {
    crate::service::output_feed::delivery_readiness::DeliveryReadiness {
        pty_id: pty_id.into(),
        generation: svc.screen_state.lock().pty_generation(pty_id),
    }
}
