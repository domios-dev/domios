use crate::service::MuxEmitCause;
use codeterm_types::agent::context::TurnContext;

pub(crate) fn merge(
    previous: Option<&TurnContext>,
    mut incoming: TurnContext,
    configured_window: Option<u64>,
) -> Option<TurnContext> {
    if !incoming.valid() {
        return None;
    }
    if incoming.window_tokens == 0 {
        incoming.window_tokens = configured_window
            .filter(|window| *window > 0 && *window <= 10_000_000)
            .or_else(|| previous.map(|context| context.window_tokens))
            .unwrap_or(0);
    }
    if incoming.cost_usd.is_none() {
        incoming.cost_usd = previous.and_then(|context| context.cost_usd);
    }
    Some(incoming)
}

pub fn apply(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    incoming: TurnContext,
) -> bool {
    update(svc, tab_id, session_id, nonce, Some(incoming))
}

pub fn update(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    incoming: Option<TurnContext>,
) -> bool {
    if session_id.is_empty() || nonce.is_empty() {
        return false;
    }
    let Some(config) = svc.try_ext::<crate::config::ConfigState>() else {
        return false;
    };
    let provider_id = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_type.clone());
    let (enabled, window) = {
        let config = config.lock();
        let provider = provider_id
            .as_ref()
            .and_then(|provider| config.agents.get(provider));
        (
            provider
                .and_then(|provider| provider.extra.get("context_feed"))
                .and_then(toml::Value::as_bool)
                == Some(true),
            provider
                .and_then(|provider| provider.extra.get("context_window_tokens"))
                .and_then(toml::Value::as_integer)
                .and_then(|value| u64::try_from(value).ok()),
        )
    };
    if !enabled {
        return false;
    }
    let mut mux = svc.mux.write();
    let Some(tab) = mux.get_tab_mut(tab_id) else {
        return false;
    };
    if tab.agent_session_id.as_deref() != Some(session_id)
        || tab.session_binding_nonce.as_deref() != Some(nonce)
        || tab.agent_type != provider_id
    {
        return false;
    }
    let context = match incoming {
        Some(incoming) => match merge(tab.context.as_ref(), incoming, window) {
            Some(context) => Some(context),
            None => return false,
        },
        None => None,
    };
    if tab.context == context {
        return false;
    }
    tab.context = context;
    drop(mux);
    svc.emit_state_changed(MuxEmitCause::ContextFeed);
    true
}

pub(crate) fn apply_runtime(
    svc: &crate::service::ServiceContext,
    provider: &str,
    context: &codeterm_plugin::agent_runtime::RuntimeContext,
    incoming: TurnContext,
) -> bool {
    let Some(thread) = context
        .thread_id
        .as_deref()
        .filter(|thread| !thread.is_empty())
    else {
        return false;
    };
    if context.binding_nonce.is_empty() || !incoming.valid() {
        return false;
    }
    let mut mux = svc.mux.write();
    let Some(tab) = mux.get_tab_mut(&context.tab_id) else {
        return false;
    };
    if tab.agent_type.as_deref() != Some(provider)
        || tab.agent_session_id.as_deref() != Some(thread)
        || tab.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str())
        || !tab.launch_session_authenticated()
        || tab.cwd.as_deref().unwrap_or_default() != context.cwd
        || !matches!(&tab.backing, crate::multiplexer::types::TabBacking::ChatBackend { plugin_id, session_id }
            if plugin_id == provider && session_id == thread)
        || tab.context.as_ref() == Some(&incoming)
    {
        return false;
    }
    tab.context = Some(incoming);
    drop(mux);
    svc.emit_state_changed(MuxEmitCause::ContextFeed);
    true
}

pub fn apply_snapshot(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    session_id: &str,
    nonce: Option<&str>,
    context: Option<&TurnContext>,
) {
    if let (Some(nonce), Some(context)) = (nonce, context) {
        apply(svc, tab_id, session_id, nonce, context.clone());
    }
}

#[cfg(test)]
mod tests;
