//! `ProviderRegistry` — the single facade fronting both provider stores (PTY
//! agents + JS chat backends) so call-sites stop touching two registries.
//! Routing keys off transport, never an id literal.

use std::sync::Arc;

use crate::agent::provider::{observe_launch_binary, ProviderDescriptor, ProviderTransport};
use crate::agent::registry::{AgentRegistry, ProviderStartupStatus};
use crate::chatbackend::ChatBackendRegistryHandle;

pub struct ProviderRegistry {
    agents: Arc<AgentRegistry>,
    chat_backends: ChatBackendRegistryHandle,
}

impl ProviderRegistry {
    pub fn new(agents: Arc<AgentRegistry>, chat_backends: ChatBackendRegistryHandle) -> Self {
        Self {
            agents,
            chat_backends,
        }
    }

    /// Every provider as a transport-tagged descriptor. Chat backends are
    /// filtered to user-spawnable ones (bare `chatBackend: true` is hidden).
    /// Chat backend descriptors are lifecycle snapshots; this path never calls
    /// into a plugin VM.
    pub fn describe_all(&self) -> Vec<ProviderDescriptor> {
        let mut out = self.agents.list_descriptors();
        let mut ids = self.chat_backends.ids();
        ids.sort();
        for id in ids {
            let Some(backend) = self.chat_backends.get(&id) else {
                continue;
            };
            if !backend.spawnable() {
                continue;
            }
            if let Some(mut descriptor) = self.chat_backends.descriptor(&id) {
                descriptor.availability =
                    observe_launch_binary(descriptor.cli_binary.as_deref()).availability;
                out.push(descriptor);
            }
        }
        out
    }

    /// Transport routing for a spawn target. `None` ⇒ id is neither a known
    /// agent nor a registered chat backend.
    pub fn transport_of(&self, id: &str) -> Option<ProviderTransport> {
        if self.chat_backends.get(id).is_some() {
            return Some(ProviderTransport::ChatBackend);
        }
        if self.agents.provider_ids().iter().any(|p| p == id) {
            return Some(ProviderTransport::Pty);
        }
        None
    }

    /// True when `id` names a registered chat backend that is user-spawnable.
    pub fn is_spawnable_chat_backend(&self, id: &str) -> bool {
        self.chat_backends
            .get(id)
            .map(|b| b.spawnable())
            .unwrap_or(false)
    }

    /// Ids offered to spawn callers: all agents plus user-spawnable backends.
    pub fn spawnable_ids(&self) -> Vec<String> {
        let mut ids = self.agents.provider_ids();
        let mut chat_ids = self.chat_backends.ids();
        chat_ids.retain(|id| self.is_spawnable_chat_backend(id));
        ids.extend(chat_ids);
        ids.sort();
        ids.dedup();
        ids
    }

    /// Startup-readiness gate (T2) for a PTY agent provider.
    pub fn wait_for_agent_startup(&self, id: &str) -> ProviderStartupStatus {
        self.agents.wait_for_provider_startup(id)
    }
}

#[cfg(test)]
mod tests;
