use crate::agent::registry::AgentRegistry;
use crate::service::tab_chat::{ChatRegistry, DeltaSink, TailConfig};
use std::sync::Arc;

pub(crate) fn ensure_append_stream(
    _cfg: TailConfig,
    _nonce: Option<&str>,
    _registry: Arc<AgentRegistry>,
    _chat_registry: &ChatRegistry,
    _sink: Arc<dyn DeltaSink>,
) -> bool {
    false
}
