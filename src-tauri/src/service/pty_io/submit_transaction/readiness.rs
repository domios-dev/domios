use crate::service::{inject_verify::InjectionError, ServiceContext};

/// Ledger reason shown while a written payload waits for the user to answer a prompt.
pub(crate) const WAITING_ON_PROMPT: &str = "waiting_on_prompt";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum EnterDecision {
    Submit,
    WithholdForPrompt,
}

/// The only screen-derived input to submission: the provider classifier's confident verdict.
pub(super) fn enter_decision(confident_numbered_prompt: bool) -> EnterDecision {
    if confident_numbered_prompt {
        EnterDecision::WithholdForPrompt
    } else {
        EnterDecision::Submit
    }
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

/// Err(AgentTuiPresent) means Enter must be withheld; it never gates a payload write.
pub(super) fn enter_prompt_guard(svc: &ServiceContext, pty_id: &str) -> Result<(), InjectionError> {
    let target = owned_target(svc, pty_id)?;
    let Some(provider_id) = target.provider_id else {
        return Ok(());
    };
    let snapshot = crate::agent::tab_state::TabStateSnapshot {
        tab_id: target.tab_id,
        agent_type: provider_id.clone(),
        session_id: target.session_id,
        cwd: target.cwd,
        screen_text: svc
            .screen_state
            .lock()
            .get_screen_text(pty_id)
            .unwrap_or_default(),
    };
    let confident_prompt = svc
        .agent_registry
        .with_provider(&provider_id, |provider| {
            provider.delivery_modal_visible(&snapshot)
        })
        .unwrap_or(false);
    match enter_decision(confident_prompt) {
        EnterDecision::Submit => Ok(()),
        EnterDecision::WithholdForPrompt => Err(InjectionError::AgentTuiPresent {
            pty_id: pty_id.into(),
        }),
    }
}

struct OwnedTarget {
    tab_id: String,
    provider_id: Option<String>,
    session_id: Option<String>,
    cwd: Option<String>,
}

fn owned_target(svc: &ServiceContext, pty_id: &str) -> Result<OwnedTarget, InjectionError> {
    let mux = svc.mux.read();
    mux.tab_id_for_pty(pty_id)
        .and_then(|tab_id| {
            mux.get_tab(&tab_id).and_then(|tab| {
                (tab.pty_id.as_deref() == Some(pty_id)).then(|| OwnedTarget {
                    tab_id: tab_id.clone(),
                    provider_id: tab.agent_type.clone(),
                    session_id: tab.agent_session_id.clone(),
                    cwd: tab.cwd.clone(),
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
