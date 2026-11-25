use super::ServiceContext;
use crate::service::MuxEmitCause;
use codeterm_plugin::provider::CapabilityProbe;
use codeterm_provider_ops::capability_probe::{CapabilityProbeHost, ProbeTarget};
use codeterm_types::agent::capability_probe::{ProbeFailure, ProbeSnapshot};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub use codeterm_provider_ops::capability_probe::invalidate_and_rebuild;

pub fn refresh(svc: &ServiceContext, tab: &str, force: bool) -> ProbeSnapshot {
    match host(svc, tab) {
        Some(host) => codeterm_provider_ops::capability_probe::refresh(host, tab, force),
        None => ProbeSnapshot::Unavailable {
            reason: ProbeFailure::ProjectUnavailable,
        },
    }
}

pub(crate) fn refresh_tab(svc: &ServiceContext, tab: &str) {
    if let Some(host) = host(svc, tab) {
        codeterm_provider_ops::capability_probe::refresh_tab(host, tab);
    }
}

struct AccountProbeHost {
    svc: ServiceContext,
    account: String,
}

fn host(svc: &ServiceContext, tab: &str) -> Option<Arc<dyn CapabilityProbeHost>> {
    let account = svc
        .mux
        .read()
        .get_tab(tab)
        .map(|tab| super::provider_accounts::selected_account(tab).to_owned())?;
    Some(Arc::new(AccountProbeHost {
        svc: svc.clone(),
        account,
    }))
}

impl CapabilityProbeHost for AccountProbeHost {
    fn validate(&self, tab: &str) -> Result<ProbeTarget, ProbeFailure> {
        let (provider, project, account) = {
            let mux = self.svc.mux.read();
            let tab = mux.get_tab(tab).ok_or(ProbeFailure::ProjectUnavailable)?;
            (
                tab.agent_type.clone().ok_or(ProbeFailure::Unsupported)?,
                tab.cwd
                    .clone()
                    .map(PathBuf::from)
                    .ok_or(ProbeFailure::ProjectUnavailable)?,
                super::provider_accounts::selected_account(tab).to_owned(),
            )
        };
        if account != self.account {
            return Err(ProbeFailure::Superseded);
        }
        let enabled = self
            .svc
            .try_ext::<crate::config::ConfigState>()
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
            .svc
            .agent_registry
            .with_provider(&provider, |provider| provider.capability_probe())
            .flatten()
            != Some(CapabilityProbe::StreamJsonInitialize)
        {
            return Err(ProbeFailure::Unsupported);
        }
        let invocation = super::provider_accounts::for_tab(&self.svc, tab)
            .map_err(|_| ProbeFailure::Superseded)?;
        if !super::provider_accounts::invocation_matches(invocation.as_deref(), &provider, &account)
        {
            return Err(ProbeFailure::Superseded);
        }
        Ok(ProbeTarget {
            provider,
            project,
            invocation,
        })
    }

    fn cli_path(&self, provider: &str) -> Option<PathBuf> {
        self.svc
            .agent_registry
            .provider_arc(provider)?
            .resolved_cli_path()
    }

    fn matching_tabs(&self, provider: &str, project: Option<&Path>) -> Vec<String> {
        self.svc
            .mux
            .read()
            .all_tabs()
            .filter(|tab| {
                super::provider_accounts::selected_account(tab) == self.account
                    && tab.agent_type.as_deref() == Some(provider)
                    && project
                        .is_none_or(|project| tab.cwd.as_deref().map(Path::new) == Some(project))
            })
            .map(|tab| tab.id.clone())
            .collect()
    }

    fn publish(&self, provider: &str, project: Option<&Path>, snapshot: Option<&ProbeSnapshot>) {
        let mut mux = self.svc.mux.write();
        let tabs: Vec<_> = mux
            .all_tabs()
            .filter(|tab| {
                super::provider_accounts::selected_account(tab) == self.account
                    && tab.agent_type.as_deref() == Some(provider)
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
        self.svc.emit_state_changed(MuxEmitCause::Capability);
    }
}
