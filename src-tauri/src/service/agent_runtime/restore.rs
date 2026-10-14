use crate::{
    multiplexer::types::{Tab, TabBacking},
    service::ServiceContext,
};

fn materialize(tab: &mut Tab, runtime_declared: bool) -> bool {
    if !runtime_declared
        || !tab.restored_from_snapshot
        || tab.agent_type.is_none()
        || matches!(tab.backing, TabBacking::Surface(_))
    {
        return false;
    }
    let provider = tab.agent_type.clone().expect("Checked provider");
    tab.backing = TabBacking::ChatBackend {
        plugin_id: provider,
        session_id: tab.agent_session_id.clone().unwrap_or_default(),
    };
    tab.pty_id = None;
    tab.chat_view = true;
    tab.agent_booting = false;
    true
}

pub(crate) fn tab(svc: &ServiceContext, tab_id: &str) -> Result<bool, String> {
    let (provider, pty, restored) = {
        let mux = svc.mux.read();
        let Some(tab) = mux.get_tab(tab_id) else {
            return Ok(false);
        };
        (
            tab.agent_type.clone(),
            tab.pty_id.clone(),
            tab.restored_from_snapshot,
        )
    };
    let Some(provider) =
        provider.filter(|provider| restored && super::engine::requested(svc, provider))
    else {
        return Ok(false);
    };
    if pty
        .as_ref()
        .is_some_and(|id| svc.pty.lock().get_session(id).is_some())
    {
        return Err("Restored runtime tab still owns a live terminal process".into());
    }
    let nonce = svc.ensure_session_binding_nonce(tab_id);
    let thread = {
        let mut mux = svc.mux.write();
        let target = mux
            .get_tab_mut(tab_id)
            .ok_or("Restored runtime tab closed")?;
        if target.agent_type.as_deref() != Some(provider.as_str())
            || target.session_binding_nonce.as_deref() != Some(nonce.as_str())
        {
            return Err("Restored runtime binding changed".into());
        }
        if !materialize(target, true) {
            return Ok(false);
        }
        target.agent_session_id.clone()
    };
    if thread.as_deref().is_some_and(|thread| !thread.is_empty()) {
        super::authority::record_spawn(svc, tab_id, &provider, &nonce)?;
    }
    Ok(true)
}

pub(crate) fn all(svc: &ServiceContext) {
    let tabs = svc
        .mux
        .read()
        .snapshot()
        .tabs
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for id in tabs {
        if let Err(reason) = tab(svc, &id) {
            log::warn!("[agent-runtime-restore] tab={id}: {reason}");
        }
    }
}

#[cfg(test)]
mod tests;
