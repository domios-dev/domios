use std::sync::Arc;

use codeterm_plugin::host_traits::PluginProviderRegistrar;

use super::registry::AgentRegistry;

impl PluginProviderRegistrar for AgentRegistry {
    fn register_provider(&self, provider: Box<dyn codeterm_plugin::AgentProvider>) {
        // After shimming src/agent/provider.rs, crate::agent::provider::AgentProvider
        // and codeterm_plugin::provider::AgentProvider are the same trait — direct pass.
        AgentRegistry::register_provider(self, provider);
    }

    fn unregister_provider(&self, id: &str) -> bool {
        AgentRegistry::unregister_provider(self, id)
    }

    fn native_fallback(&self, id: &str) -> Option<Arc<dyn codeterm_plugin::AgentProvider>> {
        AgentRegistry::native_fallback(self, id)
    }

    // The one consented reconciliation, reached from the upgrade side. It is
    // the same operation the daemon-start pass runs and refuses identically
    // without a recorded consent.
    fn provider_reloaded(&self, id: &str) {
        crate::commands::ensure_integration_for_provider(self, id);
    }
}
