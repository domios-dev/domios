use super::{submit_key_readiness, PtySubmitTransport, SubmitTransport};
use crate::service::inject_verify::InjectionError;

pub(super) fn submit_key_guard(
    transport: &PtySubmitTransport<'_>,
    allow_initial_key: bool,
    turn_was_started: bool,
) -> Result<(), InjectionError> {
    let svc = transport.svc;
    let pty_id = transport.pty_id;
    let (tab_id, agent_id, session_id, cwd, session_bound, owns_pty) = {
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
            tab_id,
            tab.agent_type.clone(),
            tab.agent_session_id.clone(),
            tab.cwd.clone(),
            tab.launch_session_authenticated(),
            tab.pty_id.as_deref() == Some(pty_id),
        )
    };
    if !owns_pty {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    if allow_initial_key && agent_id.is_none() {
        transport.dialog_guard()?;
        return Ok(());
    }
    let (screen, input) = {
        let screen_state = svc.screen_state.lock();
        (
            screen_state.get_screen_text(pty_id),
            screen_state.composer_input_state(pty_id),
        )
    };
    let Some(agent_id) = agent_id else {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    };
    let Some(screen) = screen else {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    };
    let snapshot = crate::agent::tab_state::TabStateSnapshot {
        tab_id,
        agent_type: agent_id.clone(),
        session_id,
        cwd,
        screen_text: screen.clone(),
    };
    let readiness = svc.agent_registry.with_provider(&agent_id, |provider| {
        if !allow_initial_key
            && !recovery_session_authorized(
                session_bound,
                provider.supports_unbound_submit_recovery(),
            )
        {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            });
        }
        let provider_tui = provider.screen_has_tui(&screen);
        let (cursor_ready, alternate_screen) = input
            .as_ref()
            .map(|state| {
                let cursor_ready = crate::agent::offline_state_classifier::composer_cursor_ready(
                    state.cursor_visible,
                    state.cursor_row,
                    state.rows,
                ) || state
                    .inverse_caret_row
                    .zip(state.last_nonblank_row)
                    .is_some_and(|(row, bottom)| {
                        crate::agent::offline_state_classifier::composer_cursor_ready(
                            true,
                            row,
                            bottom.saturating_add(1),
                        )
                    });
                (cursor_ready, state.alternate_screen)
            })
            .unwrap_or((false, false));
        let composer_ready = cursor_ready && (provider_tui || alternate_screen);
        let recovery_state = if allow_initial_key {
            Ok(())
        } else {
            recovery_dialog_guard(provider, &snapshot, composer_ready, pty_id)
        };
        Ok((
            provider_tui,
            cursor_ready,
            alternate_screen,
            composer_ready,
            recovery_state,
        ))
    });
    let Some(readiness) = readiness else {
        log::info!(
            "[submit-audit] key-guard pty={} provider_available=false session_bound={} turn_started_at_claim={} allowed=false",
            pty_id,
            session_bound,
            turn_was_started
        );
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    };
    let (provider_tui, cursor_ready, alternate_screen, composer_ready, recovery_state) = readiness?;
    if !allow_initial_key {
        recovery_state?;
        return Ok(());
    }
    let turn_started_now = transport.turn_started();
    let allowed = submit_key_readiness(turn_was_started, turn_started_now, composer_ready);
    if allow_initial_key || !allowed {
        log::info!(
            "[submit-audit] key-guard pty={} session_bound={} turn_started_at_claim={} turn_started_now={} provider_available=true provider_tui={} cursor_ready={} alternate_screen={} composer_ready={} allowed={}",
            pty_id,
            session_bound,
            turn_was_started,
            turn_started_now,
            provider_tui,
            cursor_ready,
            alternate_screen,
            composer_ready,
            allowed
        );
    }
    if allowed {
        Ok(())
    } else {
        Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        })
    }
}

pub(super) fn recovery_dialog_guard(
    provider: &dyn codeterm_plugin::provider::AgentProvider,
    snapshot: &crate::agent::tab_state::TabStateSnapshot,
    composer_ready: bool,
    pty_id: &str,
) -> Result<(), InjectionError> {
    if !composer_ready && provider.delivery_modal_visible(snapshot) {
        return Err(InjectionError::AgentTuiPresent {
            pty_id: pty_id.to_string(),
        });
    }
    Ok(())
}

pub(super) fn recovery_session_authorized(session_bound: bool, provider_capability: bool) -> bool {
    session_bound || provider_capability
}
