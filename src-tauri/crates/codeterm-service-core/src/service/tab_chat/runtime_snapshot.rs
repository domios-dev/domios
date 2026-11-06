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
    snapshot(svc, tab_id, None)
}

fn snapshot(
    svc: &ServiceContext,
    tab_id: &str,
    expected: Option<(&str, &str, &str)>,
) -> Result<Option<Vec<ChatMsg>>, String> {
    if crate::service::agent_runtime::requested_for_tab(svc, tab_id)
        && svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.agent_session_id.is_none()
                && tab
                    .agent_runtime_state
                    .as_ref()
                    .is_some_and(|state| state["state"] == "degraded")
        })
    {
        return Err(super::runtime_stored::StoredHistoryFailure::Unavailable
            .error("Runtime start failed before a conversation was bound; start a new session"));
    }
    let cached = cached(svc, tab_id, expected)?;
    if cached.as_ref().is_some_and(|rows| !rows.is_empty()) {
        return Ok(cached);
    }
    Ok(persisted(svc, tab_id, expected)?.or(cached))
}

fn persisted(
    svc: &ServiceContext,
    tab_id: &str,
    expected: Option<(&str, &str, &str)>,
) -> Result<Option<Vec<ChatMsg>>, String> {
    if !crate::service::agent_runtime::requested_for_tab(svc, tab_id) {
        return Ok(None);
    }
    let tab = svc
        .mux
        .read()
        .get_tab(tab_id)
        .cloned()
        .ok_or("Tab not found")?;
    let Some((provider, thread)) = tab
        .agent_type
        .as_deref()
        .zip(tab.agent_session_id.as_deref())
        .filter(|(_, thread)| !thread.is_empty())
    else {
        return Ok(None);
    };
    let context = RuntimeContext {
        tab_id: tab.id.clone(),
        cwd: tab.cwd.clone().unwrap_or_default(),
        thread_id: Some(thread.into()),
        binding_nonce: tab.session_binding_nonce.clone().unwrap_or_default(),
        ..Default::default()
    };
    if expected
        .is_some_and(|(id, cwd, session)| id != provider || cwd != context.cwd || session != thread)
    {
        return Ok(None);
    }
    let Some(capability) = svc
        .agent_registry
        .with_provider(provider, |adapter| adapter.agent_runtime())
        .flatten()
    else {
        return Ok(None);
    };
    let Some(rows) =
        engine::persisted_transcript(provider, thread, &capability).map_err(|error| {
            super::runtime_stored::StoredHistoryFailure::Unavailable
                .error(format!("Stored runtime transcript: {error}"))
        })?
    else {
        return Ok(None);
    };
    if !owns(&tab, provider, &context) {
        return Err("Stored runtime transcript binding is not current".into());
    }
    if !svc
        .mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| owns(tab, provider, &context))
    {
        return Err("Stored runtime transcript binding changed during snapshot".into());
    }
    log::debug!(
        "[runtime-transcript] tab={tab_id} snapshot source=persisted provider={provider} rows={}",
        rows.len()
    );
    Ok(Some(rows))
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
    if context.thread_id.is_none()
        && matches!(
            session.state.lock().phase,
            crate::service::agent_runtime::model::RuntimePhase::Degraded { .. }
        )
    {
        return Err(super::runtime_stored::StoredHistoryFailure::Unavailable
            .error("Runtime start failed before a conversation was bound; start a new session"));
    }
    if expected.is_some_and(|(provider, cwd, thread)| {
        session.provider != provider
            || context.cwd != cwd
            || context.thread_id.as_deref() != Some(thread)
    }) {
        return Ok(None);
    }
    let tab = svc.mux.read().get_tab(tab_id).cloned();
    let loading = matches!(
        session.state.lock().phase,
        crate::service::agent_runtime::model::RuntimePhase::Loading
    );
    let process_live = session.process.lock().as_ref().is_some_and(|process| {
        process.ended.borrow().is_none() && process.client.ended().borrow().is_none()
    });
    let fresh_unused = loading
        && process_live
        && tab.as_ref().is_some_and(|tab| {
            context.thread_id.as_deref().is_some_and(|thread| {
                tab.agent_runtime_config
                    .as_ref()
                    .and_then(|config| config.get("unusedThreadId"))
                    .and_then(serde_json::Value::as_str)
                    == Some(thread)
            })
        });
    let current = tab
        .as_ref()
        .is_some_and(|tab| owns(tab, &session.provider, &context));
    if !current || engine::validate_binding(svc, &session).is_err() {
        return Err("Runtime transcript binding is not current".into());
    }
    let messages = session.cache.lock().snapshot();
    if !session.thread_loaded.load(Ordering::Acquire) && !fresh_unused && messages.is_empty() {
        return Ok(None);
    }
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
    let mut found = None;
    for tab in tabs {
        if let Some(messages) = snapshot(svc, &tab, Some((provider_id, cwd, session_id)))? {
            if found.is_some() {
                return Err("Runtime transcript has multiple current tab owners".into());
            }
            found = Some((messages, 0));
        }
    }
    match found {
        Some(snapshot) => Ok(Some(snapshot)),
        None => super::runtime_stored::archived_window(svc, provider_id, cwd, session_id),
    }
}

#[cfg(test)]
mod tests;
