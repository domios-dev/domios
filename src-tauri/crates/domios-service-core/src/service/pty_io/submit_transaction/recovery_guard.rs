use super::PtySubmitTransport;
use crate::service::inject_verify::InjectionError;

/// What a recovery key (any submit key after the first) may rely on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecoveryKeyInputs {
    pub session_bound: bool,
    pub provider_capability: bool,
    pub session_starts_on_first_user_turn: bool,
    pub launch_owns_pty: bool,
    pub draft_present: bool,
}

/// A first-turn provider only binds its session once a turn is submitted, so
/// waiting for the binding before retrying a dropped Enter can never succeed.
pub(super) fn recovery_key_authority(inputs: RecoveryKeyInputs) -> Result<(), &'static str> {
    if recovery_session_authorized(inputs.session_bound, inputs.provider_capability) {
        return Ok(());
    }
    if !inputs.session_starts_on_first_user_turn {
        return Err("session_unbound");
    }
    if !inputs.launch_owns_pty {
        return Err("unbound_without_launch_owner");
    }
    if !inputs.draft_present {
        return Err("unbound_draft_absent");
    }
    Ok(())
}

/// The audit line for a refused recovery key, once per distinct reason, so a
/// wait loop re-asking the guard on every frame does not flood the log.
pub(super) fn recovery_refusal_line(
    last: &std::cell::Cell<Option<&'static str>>,
    pty_id: &str,
    reason: &'static str,
) -> Option<String> {
    (last.replace(Some(reason)) != Some(reason))
        .then(|| format!("[submit-audit] recovery-key refused pty={pty_id} reason={reason}"))
}

fn log_recovery_refusal(transport: &PtySubmitTransport<'_>, reason: &'static str) {
    if let Some(line) = recovery_refusal_line(&transport.recovery_refusal, transport.pty_id, reason)
    {
        log::info!("{line}");
    }
}

pub(super) fn submit_key_guard(
    transport: &PtySubmitTransport<'_>,
    allow_initial_key: bool,
) -> Result<(), InjectionError> {
    let svc = transport.svc;
    let pty_id = transport.pty_id;
    let (agent_id, session_bound, owns_pty, launch_nonce) = {
        let mux = svc.mux.read();
        let Some(tab_id) = mux.tab_id_for_pty(pty_id) else {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            });
        };
        let Some(tab) = mux.get_tab(&tab_id) else {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            });
        };
        (
            tab.agent_type.clone(),
            tab.launch_session_authenticated(),
            tab.pty_id.as_deref() == Some(pty_id),
            tab.session_binding_nonce.is_some(),
        )
    };
    if !owns_pty {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    if allow_initial_key {
        return Ok(());
    }
    let Some(agent_id) = agent_id else {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    };
    let authority = svc
        .agent_registry
        .with_provider(&agent_id, |provider| {
            recovery_key_authority(RecoveryKeyInputs {
                session_bound,
                provider_capability: provider.supports_unbound_submit_recovery(),
                session_starts_on_first_user_turn: provider.session_starts_on_first_user_turn(),
                launch_owns_pty: launch_nonce,
                draft_present: true,
            })
        })
        .unwrap_or(Err("provider_unavailable"));
    if let Err(reason) = authority {
        log_recovery_refusal(transport, reason);
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    Ok(())
}

pub(super) fn recovery_session_authorized(session_bound: bool, provider_capability: bool) -> bool {
    session_bound || provider_capability
}

/// One bounded Enter for an unbound first-turn launch whose draft never left
/// the composer. Same authority as a recovery key, plus the owner dialog guard.
pub(crate) fn nudge_unbound_launch_draft(
    svc: &crate::service::ServiceContext,
    pty_id: &str,
    draft: &str,
    submit_key: &[u8],
) -> Result<(), &'static str> {
    let (agent_id, session_bound, owns_pty, launch_nonce) = {
        let mux = svc.mux.read();
        let tab_id = mux.tab_id_for_pty(pty_id).ok_or("tab_missing")?;
        let tab = mux.get_tab(&tab_id).ok_or("tab_missing")?;
        (
            tab.agent_type.clone(),
            tab.launch_session_authenticated(),
            tab.pty_id.as_deref() == Some(pty_id),
            tab.session_binding_nonce.is_some(),
        )
    };
    if !owns_pty {
        return Err("pty_not_owned");
    }
    if session_bound {
        return Err("session_already_bound");
    }
    let agent_id = agent_id.ok_or("agent_missing")?;
    let draft_present =
        !draft.trim().is_empty() && super::composer_draft_evidence(svc, pty_id, draft) > 0;
    let (provider_capability, session_starts_on_first_user_turn) = svc
        .agent_registry
        .with_provider(&agent_id, |provider| {
            (
                provider.supports_unbound_submit_recovery(),
                provider.session_starts_on_first_user_turn(),
            )
        })
        .ok_or("provider_unavailable")?;
    recovery_key_authority(RecoveryKeyInputs {
        session_bound,
        provider_capability,
        session_starts_on_first_user_turn,
        launch_owns_pty: launch_nonce,
        draft_present,
    })?;
    super::pty_owner_guard(svc, pty_id).map_err(|_| "pty_not_owned")?;
    let claim = super::claim_submit_cycle(pty_id);
    if claim.command_wait().is_some() {
        return Err("model_command_in_flight");
    }
    svc.pty
        .lock()
        .write_and_flush(pty_id, submit_key, std::time::Duration::from_secs(2))
        .map_err(|_| "key_write_failed")
}
