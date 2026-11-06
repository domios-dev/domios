use crate::multiplexer::types::{Tab, TabBacking};
use codeterm_plugin::agent_runtime::RuntimeFailureCode;
use serde_json::Value;
use std::sync::atomic::Ordering;

fn unbound(value: Option<&str>) -> bool {
    value.is_none_or(str::is_empty)
}

pub(super) fn is_failed_unbound_state(state: &Value) -> bool {
    state["state"] == "degraded"
        && serde_json::from_value::<RuntimeFailureCode>(state["failureCode"].clone()).ok()
            == Some(RuntimeFailureCode::RuntimeStartUnbound)
}

pub(crate) fn matches(tab: &Tab, state: &Value) -> bool {
    if !is_failed_unbound_state(state)
        || !unbound(tab.agent_session_id.as_deref())
        || tab.reported_session_binding_nonce.is_some()
        || tab
            .session_binding_nonce
            .as_deref()
            .is_none_or(str::is_empty)
        || !matches!(&tab.backing, TabBacking::ChatBackend { plugin_id, session_id }
            if Some(plugin_id.as_str()) == tab.agent_type.as_deref() && session_id.is_empty())
    {
        return false;
    }
    let Some(session) = super::engine::for_tab(&tab.id) else {
        return true;
    };
    let context = session.context.lock().clone();
    let runtime = session.state.lock();
    session.provider == tab.agent_type.as_deref().unwrap_or_default()
        && context.tab_id == tab.id
        && Some(context.binding_nonce.as_str()) == tab.session_binding_nonce.as_deref()
        && unbound(context.thread_id.as_deref())
        && unbound(runtime.thread_id.as_deref())
        && matches!(runtime.phase, super::model::RuntimePhase::Degraded { .. })
        && matches!(
            runtime.failure_code,
            Some(RuntimeFailureCode::RuntimeStartUnbound)
        )
        && !session.thread_loaded.load(Ordering::Acquire)
}

pub(crate) fn for_tab(tab: &Tab) -> bool {
    tab.agent_runtime_state
        .as_ref()
        .is_some_and(|state| matches(tab, state))
}
