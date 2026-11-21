use crate::agent::reports::ReportSenderPosition;
use crate::service::ServiceContext;

use super::engine;

pub fn sender_position(svc: &ServiceContext, tab_id: &str) -> Option<ReportSenderPosition> {
    let Some(session) = engine::for_tab(tab_id) else {
        return None;
    };
    let context = session.context.lock().clone();
    let mux = svc.mux.read();
    let Some(tab) = mux.get_tab(tab_id) else {
        return None;
    };
    if tab.agent_type.as_deref() != Some(session.provider.as_str())
        || tab.cwd.as_deref().unwrap_or_default() != context.cwd
        || tab.agent_session_id != context.thread_id
        || tab.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str())
    {
        return None;
    }
    drop(mux);
    let Some(source_session_id) = context.thread_id else {
        return None;
    };
    let cache = session.cache.lock();
    let Some(message_id) = cache.last_message_id() else {
        return None;
    };
    Some(ReportSenderPosition {
        source_session_id,
        message_id: message_id.to_owned(),
    })
}

#[cfg(test)]
mod tests;
