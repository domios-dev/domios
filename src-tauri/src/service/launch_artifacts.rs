use codeterm_plugin::provider::{
    LaunchArtifactSweepReport, LaunchArtifactSweepRequest,
};

use super::ServiceContext;

fn live_tab_ids(service: &ServiceContext) -> Vec<String> {
    let mut tab_ids: Vec<_> = service
        .mux
        .read()
        .snapshot()
        .tabs
        .into_keys()
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
            .with_provider(&provider_id, |provider| provider.sweep_launch_artifacts(&request))
        {
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
        .with_provider(provider_id, |provider| provider.sweep_launch_artifacts(&request))
        .ok_or_else(|| format!("Unknown provider: {provider_id}"))?
        .and_then(|report| report.ok_or_else(|| "Provider does not support launch artifact cleanup".into()))
}
