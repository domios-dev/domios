use super::ServiceContext;
use codeterm_plugin::provider::CapabilityProbe;
use codeterm_provider_ops::capability_probe::{CapabilityProbeHost, ProbeTarget};
use codeterm_types::agent::capability_probe::{ProbeFailure, ProbeSnapshot};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::Manager;

pub use codeterm_provider_ops::capability_probe::invalidate_and_rebuild;

pub fn refresh(svc: &ServiceContext, tab: &str, force: bool) -> ProbeSnapshot {
    codeterm_provider_ops::capability_probe::refresh(Arc::new(svc.clone()), tab, force)
}

pub(crate) fn refresh_tab(svc: &ServiceContext, tab: &str) {
    codeterm_provider_ops::capability_probe::refresh_tab(Arc::new(svc.clone()), tab);
}

impl CapabilityProbeHost for ServiceContext {
    fn validate(&self, tab: &str) -> Result<ProbeTarget, ProbeFailure> {
        let (provider, project) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab).ok_or(ProbeFailure::ProjectUnavailable)?;
            (
                tab.agent_type.clone().ok_or(ProbeFailure::Unsupported)?,
                tab.cwd
                    .clone()
                    .map(PathBuf::from)
                    .ok_or(ProbeFailure::ProjectUnavailable)?,
            )
        };
        let enabled = self
            .app_handle
            .try_state::<crate::config::ConfigState>()
            .is_some_and(|config| {
                config
                    .lock()
                    .agents
                    .get(&provider)
                    .and_then(|config| config.extra.get("capability_probe_enabled"))
                    .and_then(toml::Value::as_bool)
                    == Some(true)
            });
        if !enabled {
            return Err(ProbeFailure::Disabled);
        }
        if self
            .agent_registry
            .with_provider(&provider, |provider| provider.capability_probe())
            .flatten()
            != Some(CapabilityProbe::StreamJsonInitialize)
        {
            return Err(ProbeFailure::Unsupported);
        }
        Ok(ProbeTarget { provider, project })
    }

    fn cli_binary(&self, provider: &str) -> Option<String> {
        self.agent_registry
            .with_provider(provider, |provider| {
                provider.cli_binary().map(str::to_string)
            })
            .flatten()
    }

    fn matching_tabs(&self, provider: &str, project: Option<&Path>) -> Vec<String> {
        self.mux
            .read()
            .all_tabs()
            .filter(|tab| {
                tab.agent_type.as_deref() == Some(provider)
                    && project
                        .is_none_or(|project| tab.cwd.as_deref().map(Path::new) == Some(project))
            })
            .map(|tab| tab.id.clone())
            .collect()
    }

    fn publish(&self, provider: &str, project: Option<&Path>, snapshot: Option<&ProbeSnapshot>) {
        let mut mux = self.mux.write();
        let tabs: Vec<_> = mux
            .all_tabs()
            .filter(|tab| {
                tab.agent_type.as_deref() == Some(provider)
                    && project
                        .is_none_or(|project| tab.cwd.as_deref().map(Path::new) == Some(project))
            })
            .map(|tab| tab.id.clone())
            .collect();
        for id in tabs {
            if let Some(tab) = mux.get_tab_mut(&id) {
                tab.provider_signals.capabilities = snapshot.cloned();
            }
        }
        drop(mux);
        self.emit_state_changed();
    }
}
