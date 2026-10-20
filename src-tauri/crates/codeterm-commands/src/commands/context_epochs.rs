use codeterm_service_core::{
    agent::chat_types::ChatMsg,
    multiplexer::types::ContextEpochArchive,
    service::{tab_chat::session_segments, ServiceState},
};

pub(crate) fn materialize_archived_context_epochs(
    svc: &ServiceState,
    archives: &[ContextEpochArchive],
    current_session_id: &str,
) -> Vec<ChatMsg> {
    session_segments::archived_provider_sessions(svc, archives, current_session_id)
}
