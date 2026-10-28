use super::ChatMsg;
use crate::{
    multiplexer::types::{Tab, TabBacking},
    service::{agent_runtime::engine, ServiceContext},
};
use codeterm_plugin::agent_runtime::RuntimeContext;
use std::sync::atomic::Ordering;

fn owns(tab: &Tab, provider: &str, context: &RuntimeContext) -> bool {
    tab.id == context.tab_id
        && tab.agent_type.as_deref() == Some(provider)
        && tab.launch_session_authenticated()
        && tab.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
        && tab.agent_session_id == context.thread_id
        && tab.cwd.as_deref().unwrap_or_default() == context.cwd.as_str()
        && matches!(&tab.backing, TabBacking::ChatBackend { plugin_id, session_id }
            if plugin_id == provider && Some(session_id) == context.thread_id.as_ref())
}

pub(super) fn for_tab(svc: &ServiceContext, tab_id: &str) -> Result<Option<Vec<ChatMsg>>, String> {
    cached(svc, tab_id, None)
}

fn cached(
    svc: &ServiceContext,
    tab_id: &str,
    expected: Option<(&str, &str, &str)>,
) -> Result<Option<Vec<ChatMsg>>, String> {
    if !crate::service::agent_runtime::requested_for_tab(svc, tab_id) {
        return Ok(None);
    }
    let Some(session) = engine::for_tab(tab_id) else {
        return Ok(None);
    };
    let context = session.context.lock().clone();
    if expected.is_some_and(|(provider, cwd, thread)| {
        session.provider != provider
            || context.cwd != cwd
            || context.thread_id.as_deref() != Some(thread)
    }) || !session.thread_loaded.load(Ordering::Acquire)
    {
        return Ok(None);
    }
    let current = svc
        .mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| owns(tab, &session.provider, &context));
    if !current || engine::validate_binding(svc, &session).is_err() {
        return Err("Runtime transcript binding is not current".into());
    }
    let messages = session.cache.lock().snapshot();
    let current_context = session.context.lock().clone();
    let current = svc
        .mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| owns(tab, &session.provider, &context));
    if !current
        || current_context.epoch != context.epoch
        || current_context.thread_id != context.thread_id
        || current_context.binding_nonce != context.binding_nonce
        || current_context.cwd != context.cwd
        || engine::validate_binding(svc, &session).is_err()
    {
        return Err("Runtime transcript binding changed during snapshot".into());
    }
    Ok(Some(messages))
}

pub(super) fn for_session(
    svc: &ServiceContext,
    provider_id: &str,
    cwd: &str,
    session_id: &str,
) -> Result<Option<(Vec<ChatMsg>, u64)>, String> {
    if !svc
        .agent_registry
        .with_provider(provider_id, |provider| provider.agent_runtime().is_some())
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let tabs = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| {
            tab.agent_type.as_deref() == Some(provider_id)
                && tab.agent_session_id.as_deref() == Some(session_id)
                && tab.cwd.as_deref().unwrap_or_default() == cwd
                && matches!(&tab.backing, TabBacking::ChatBackend { plugin_id, session_id: bound }
                if plugin_id == provider_id && bound == session_id)
        })
        .map(|tab| tab.id.clone())
        .collect::<Vec<_>>();
    let mut snapshot = None;
    for tab in tabs {
        if let Some(messages) = cached(svc, &tab, Some((provider_id, cwd, session_id)))? {
            if snapshot.is_some() {
                return Err("Runtime transcript has multiple current tab owners".into());
            }
            snapshot = Some((messages, 0));
        }
    }
    match snapshot {
        Some(snapshot) => Ok(Some(snapshot)),
        None => super::runtime_stored::archived_window(svc, provider_id, cwd, session_id),
    }
}

#[cfg(test)]
mod tests;
