use std::sync::Arc;

use codeterm_plugin::{provider::AgentProvider, tab_state::TabStateSnapshot};
use tauri::Manager;

use super::ServiceContext;
pub use codeterm_provider_ops::provider_update::*;

mod idle;

impl ProviderUpdateHost for ServiceContext {
    fn providers(&self) -> Vec<Arc<dyn AgentProvider>> {
        self.agent_registry.all_provider_arcs()
    }

    fn provider(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.agent_registry.provider_arc(id)
    }

    fn auto_update_enabled(&self) -> bool {
        self.app_handle
            .try_state::<crate::config::ConfigState>()
            .is_some_and(|config| config.lock().updater.auto_update_provider_clis)
    }

    fn matches_own_session(&self, snapshot: &TabStateSnapshot) -> bool {
        self.mux
            .read()
            .get_tab(&snapshot.tab_id)
            .is_some_and(|tab| {
                tab.agent_type.as_deref() == Some(snapshot.agent_type.as_str())
                    && tab.agent_session_id == snapshot.session_id
            })
    }

    fn provider_busy(&self, provider: &str) -> bool {
        idle::provider_busy(self, provider)
    }

    fn completed(&self, provider: &str, summary: &str) {
        crate::events::emit_everywhere(
            &self.app_handle,
            "provider-cli-completed",
            serde_json::json!({
                "provider": provider, "operation": "update", "summary": summary,
            }),
        );
    }
}

#[cfg(test)]
mod tests;
