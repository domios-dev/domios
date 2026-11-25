use super::{apply_general_agent_launch_claim, state, terminate_pty_before_relaunch};
use crate::multiplexer::types::{Tab, TabBacking};
use crate::service::{
    agent_runtime::engine, messenger::switch_handoff::same_binding, ServiceContext,
};

pub(super) fn prepare(
    svc: &ServiceContext,
    source: &Tab,
    provider: &str,
    resume: bool,
    cwd: &str,
) -> Result<Option<String>, String> {
    let terminal = svc.agent_registry.with_provider(provider, |p| {
        let descriptor = p.describe();
        descriptor.transport == crate::agent::provider::ProviderTransport::Pty
            && descriptor.agent_runtime.is_none()
    });
    if terminal != Some(true) {
        return Err(format!(
            "General PTY launch requires an available terminal provider: {provider}"
        ));
    }
    let same_provider = source.agent_type.as_deref() == Some(provider);
    let session = if same_provider && resume {
        let id = source
            .agent_session_id
            .as_deref()
            .ok_or("Cannot resume General: this tab has no bound PTY session")?;
        if !state::session_id_is_owned(
            &svc.agent_registry,
            provider,
            cwd,
            id,
            (source.agent_type.as_deref(), Some(id)),
            None,
        ) {
            return Err(
                "Cannot resume General: the PTY provider could not verify this tab's session"
                    .into(),
            );
        }
        Some(id.to_owned())
    } else {
        None
    };

    let tab_id = &source.id;
    let _claim = svc.pty_ownership.acquire(tab_id)?;
    let check_source = |mux: &crate::multiplexer::state::MultiplexerState| -> Result<(), String> {
        if !mux
            .get_tab(tab_id)
            .is_some_and(|tab| same_binding(source, tab))
        {
            return Err("General binding changed during transport retirement".into());
        }
        if session
            .as_ref()
            .is_some_and(|id| mux.other_session_ids(tab_id).contains(id))
        {
            return Err("Cannot resume General: another tab owns its PTY session".into());
        }
        if let Some(id) = source.pty_id.as_deref() {
            if mux.tab_id_for_pty(id).is_some_and(|owner| owner != *tab_id) {
                return Err(
                    "Cannot restart General: its recorded PTY belongs to another tab".into(),
                );
            }
        }
        Ok(())
    };
    check_source(&svc.mux.read())?;
    if let Some(runtime) = engine::for_tab(tab_id) {
        engine::validate_binding(svc, &runtime).map_err(|reason| reason.to_string())?;
        engine::stop_confirmed_blocking(svc, tab_id, runtime)?;
    }
    if let Some(pty) = source.pty_id.as_deref() {
        terminate_pty_before_relaunch(&svc.pty, &svc.screen_state, pty)?;
        svc.pty_ownership.release_pty(pty);
    }
    let cleared = {
        let mut mux = svc.mux.write();
        check_source(&mux)?;
        let tab = mux.get_tab_mut(tab_id).expect("Checked General binding");
        if engine::for_tab(tab_id).is_some() {
            return Err("General binding changed during transport retirement".into());
        }
        let cleared = session
            .is_none()
            .then(|| crate::service::tab::clear_explicit_agent_identity(tab));
        tab.backing = TabBacking::Pty;
        tab.agent_runtime_state = None;
        tab.agent_runtime_config = None;
        tab.agent_runtime_questions = None;
        tab.structured_runtime_mode = None;
        tab.restorable_launch.structured_runtime_mode = None;
        tab.restorable_launch.runtime_launch_options = None;
        tab.restorable_launch.exited_chat_binding = None;
        if !same_provider {
            crate::service::provider_handoff::reset_tab_identity_for_provider_change(tab, false);
        }
        apply_general_agent_launch_claim(tab);
        mux.unlink_pty(tab_id);
        cleared
    };
    if let Some(cleared) = cleared {
        svc.finish_explicit_agent_identity_clear(
            tab_id,
            "General retired its chat backing for a PTY launch",
            cleared,
        );
    }
    Ok(session)
}

#[cfg(test)]
mod tests;
