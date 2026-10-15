//! `ProviderRegistry` — the single facade fronting both provider stores (PTY
//! agents + JS chat backends) so call-sites stop touching two registries.
//! Routing keys off transport, never an id literal.

use std::sync::Arc;

use super::catalogue_cache::RevisionCache;
use crate::agent::provider::{observe_launch_binary, ProviderDescriptor, ProviderTransport};
use crate::agent::registry::{AgentRegistry, ProviderStartupStatus};
use crate::chatbackend::ChatBackendRegistryHandle;

type CliRevision = (u64, u64, u64);
pub(super) type CatalogueRevision = (
    Vec<crate::agent::model_catalog::ModelCatalogueChanged>,
    u64,
    u64,
);

pub struct ProviderRegistry {
    agents: Arc<AgentRegistry>,
    chat_backends: ChatBackendRegistryHandle,
    cli_status_cache: parking_lot::Mutex<
        RevisionCache<CliRevision, codeterm_plugin::provider_cli::ProviderCliStatus>,
    >,
    pub(super) catalogue_values_cache:
        parking_lot::Mutex<RevisionCache<CatalogueRevision, Vec<serde_json::Value>>>,
}

impl ProviderRegistry {
    pub fn new(agents: Arc<AgentRegistry>, chat_backends: ChatBackendRegistryHandle) -> Self {
        Self {
            agents,
            chat_backends,
            cli_status_cache: Default::default(),
            catalogue_values_cache: Default::default(),
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
                if !out.iter().any(|agent| agent.id == descriptor.id) {
                    out.push(descriptor);
                }
            }
        }
        out
    }

    pub(crate) fn cli_status(
        &self,
        id: &str,
        platform: &str,
    ) -> Result<codeterm_plugin::provider_cli::ProviderCliStatus, String> {
        let revision = self.cli_revision(id);
        let key = format!("{id}:{platform}");
        if let Some(status) = self.cli_status_cache.lock().get(&key, &revision) {
            return Ok(status);
        }
        let status = self.compute_cli_status(id, platform)?;
        if self.cli_revision(id) == revision {
            self.cli_status_cache
                .lock()
                .insert(key, revision, status.clone());
        }
        Ok(status)
    }

    fn cli_revision(&self, id: &str) -> CliRevision {
        (
            crate::agent::model_catalog::current_version(id),
            codeterm_plugin::provider_version::revision(),
            crate::service::provider_update::revision(),
        )
    }

    pub(super) fn catalogue_revision(&self) -> CatalogueRevision {
        (
            crate::agent::model_catalog::version_snapshot(),
            codeterm_plugin::provider_version::revision(),
            crate::service::provider_update::revision(),
        )
    }

    fn compute_cli_status(
        &self,
        id: &str,
        platform: &str,
    ) -> Result<codeterm_plugin::provider_cli::ProviderCliStatus, String> {
        match self.transport_of(id) {
            Some(ProviderTransport::ChatBackend)
                if self
                    .agents
                    .provider_arc(id)
                    .map(|p| p.agent_runtime().is_some())
                    != Some(true) =>
            {
                Ok(codeterm_plugin::provider_cli::no_cli())
            }
            Some(ProviderTransport::Pty | ProviderTransport::ChatBackend) => {
                let (binary, installer, updater, compatibility) = self
                    .agents
                    .provider_arc(id)
                    .map(|p| {
                        (
                            p.cli_binary()
                                .map(str::to_owned)
                                .or_else(|| p.agent_runtime().map(|runtime| runtime.command.binary))
                                .unwrap_or_else(|| p.binary_name().to_owned()),
                            p.install_command(platform),
                            p.update_command(platform),
                            p.cli_compatibility(),
                        )
                    })
                    .ok_or_else(|| format!("Unknown provider: {id}"))?;
                let mut status = codeterm_plugin::provider_cli::observe_cli_with_compatibility(
                    binary,
                    installer,
                    updater,
                    compatibility.as_ref(),
                );
                status.update_summary = crate::service::provider_update::status_summary(id);
                Ok(status)
            }
            None => Err(format!("Unknown provider: {id}")),
        }
    }

    /// Transport routing for a spawn target. `None` ⇒ id is neither a known
    /// agent nor a registered chat backend.
    pub fn transport_of(&self, id: &str) -> Option<ProviderTransport> {
        if self.chat_backends.get(id).is_some() {
            return Some(ProviderTransport::ChatBackend);
        }
        self.agents.provider_arc(id).map(|provider| {
            if provider.agent_runtime().is_some() {
                ProviderTransport::ChatBackend
            } else {
                ProviderTransport::Pty
            }
        })
    }

    /// True when `id` names a registered chat backend that is user-spawnable.
    pub fn is_spawnable_chat_backend(&self, id: &str) -> bool {
        if self
            .agents
            .with_provider(id, |p| p.agent_runtime().is_some())
            == Some(true)
        {
            return true;
        }
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
