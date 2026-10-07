use super::host_api::PluginSandbox;
use super::manifest::PluginManifest;
use crate::provider_version::{ProbeStatus, VersionObservation};

pub(super) struct ProviderObservation {
    pub(super) binary: Option<String>,
    pub(super) compatibility: Option<crate::provider_compat::CompatibilityManifest>,
}

impl ProviderObservation {
    pub(super) fn from_manifest(json: Option<&str>) -> Self {
        let manifest = json.and_then(|json| PluginManifest::from_json_str(json).ok());
        Self {
            binary: manifest
                .as_ref()
                .filter(|m| m.capabilities.agent_provider)
                .map(|m| {
                    m.cli_binary
                        .as_ref()
                        .or(m.binary_name.as_ref())
                        .unwrap_or(&m.id)
                        .clone()
                }),
            compatibility: manifest.and_then(|m| m.cli_compatibility),
        }
    }

    pub(super) fn version(&self, sandbox: &PluginSandbox) -> VersionObservation {
        match self
            .binary
            .as_deref()
            .filter(|binary| sandbox.can_exec(binary))
        {
            Some(binary) => crate::provider_version::observe(binary),
            None => VersionObservation {
                version: None,
                status: ProbeStatus::Unreadable,
            },
        }
    }
}
