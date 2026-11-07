use serde_json::{json, Value};

pub(super) fn sender_position(svc: &crate::service::ServiceContext, tab_id: &str) -> Value {
    let Some(session) = crate::service::agent_runtime::engine::for_tab(tab_id) else {
        return Value::Null;
    };
    let context = session.context.lock().clone();
    let mux = svc.mux.read();
    let Some(tab) = mux.get_tab(tab_id) else {
        return Value::Null;
    };
    if tab.agent_type.as_deref() != Some(session.provider.as_str())
        || tab.cwd.as_deref().unwrap_or_default() != context.cwd
        || tab.agent_session_id != context.thread_id
        || tab.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str())
    {
        return Value::Null;
    }
    drop(mux);
    let Some(source_session_id) = context.thread_id else {
        return Value::Null;
    };
    let cache = session.cache.lock();
    let Some(message_id) = cache.last_message_id() else {
        return Value::Null;
    };
    json!({ "source_session_id": source_session_id, "message_id": message_id })
}

#[cfg(test)]
mod tests;
