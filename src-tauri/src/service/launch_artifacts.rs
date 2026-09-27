use crate::agent::registry::AgentRegistry;
use codeterm_plugin::provider::{LaunchArtifactSweepReport, LaunchArtifactSweepRequest};

use super::ServiceContext;

fn live_tab_ids(service: &ServiceContext) -> Vec<String> {
    let mut tab_ids: Vec<_> = service
        .mux
        .read()
        .snapshot()
        .tabs
        .into_iter()
        .filter_map(|(tab_id, tab)| {
            (tab.boot_stage != Some(crate::service::boot_stage::BootStage::SpawnFailed))
                .then_some(tab_id)
        })
        .collect();
    tab_ids.sort();
    tab_ids
}

pub(crate) fn sweep_after_restore(service: &ServiceContext) {
    let request = LaunchArtifactSweepRequest {
        instance_id: crate::paths::codeterm_instance_id().to_string(),
        live_tab_ids: live_tab_ids(service),
        confirm: true,
        include_legacy: false,
    };
    for provider_id in service.agent_registry.provider_ids() {
        match service
            .agent_registry
            .with_provider(&provider_id, |provider| {
                provider.sweep_launch_artifacts(&request)
            }) {
            Some(Ok(Some(report))) if !report.removed.is_empty() => {
                log::info!(
                    "[startup] released this instance's dead launch artifacts: {:?}",
                    report.removed
                );
            }
            Some(Err(error)) => {
                log::warn!("[startup] launch artifact sweep failed: {error}");
            }
            _ => {}
        }
    }
}

pub(crate) fn release_closed_tabs(registry: &AgentRegistry, agent_providers: &[(String, String)]) {
    let instance_id = crate::paths::codeterm_instance_id();
    for (tab_id, provider_id) in agent_providers {
        if let Some(Err(error)) = registry.with_provider(provider_id, |provider| {
            provider.release_tab_launch_artifacts(tab_id, instance_id)
        }) {
            log::warn!("[tab-close] launch artifact release failed for {tab_id}: {error}");
        }
    }
}

pub(crate) fn release_tab(
    registry: &AgentRegistry,
    tab_id: &str,
    provider_id: &str,
) -> Result<bool, String> {
    registry
        .with_provider(provider_id, |provider| {
            provider.release_tab_launch_artifacts(tab_id, crate::paths::codeterm_instance_id())
        })
        .ok_or_else(|| format!("Unknown provider: {provider_id}"))?
}

pub(crate) fn sweep_for_owner(
    service: &ServiceContext,
    provider_id: &str,
    confirm: bool,
    include_legacy: bool,
) -> Result<LaunchArtifactSweepReport, String> {
    let request = LaunchArtifactSweepRequest {
        instance_id: crate::paths::codeterm_instance_id().to_string(),
        live_tab_ids: live_tab_ids(service),
        confirm,
        include_legacy,
    };
    service
        .agent_registry
        .with_provider(provider_id, |provider| {
            provider.sweep_launch_artifacts(&request)
        })
        .ok_or_else(|| format!("Unknown provider: {provider_id}"))?
        .and_then(|report| {
            report.ok_or_else(|| "Provider does not support launch artifact cleanup".into())
        })
}
