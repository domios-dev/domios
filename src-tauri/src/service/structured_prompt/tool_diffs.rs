use crate::agent::chat_types::ChatMsg;
use crate::plugin::provider::AgentProvider;
use crate::service::ServiceContext;
use serde_json::Value;

pub(super) fn map(provider: &dyn AgentProvider, part: &Value, row: &mut ChatMsg) {
    if row.kind == "tool_call" {
        row.file_diffs = provider.structured_tool_file_diffs(part);
    }
}

pub(super) fn apply(svc: &ServiceContext, tab_id: &str, part: &Value, row: &mut ChatMsg) {
    if row.kind != "tool_call" {
        return;
    }
    let provider_id = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_type.clone());
    if let Some(provider_id) = provider_id {
        svc.agent_registry
            .with_provider(&provider_id, |provider| map(provider, part, row));
    }
}
