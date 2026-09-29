use crate::agent::registry::AgentRegistry;
use codeterm_plugin::provider::{LaunchArtifactSweepReport, LaunchArtifactSweepRequest};

use super::ServiceContext;

fn live_tab_ids(service: &ServiceContext) -> Vec<String> {
    let mut tab_ids: Vec<_> = service.mux.read().snapshot().tabs.into_keys().collect();
    tab_ids.sort();
    tab_ids
}

fn startup_live_tab_ids(service: &ServiceContext) -> Vec<String> {
    let tabs = service.mux.read().snapshot().tabs;
    let mut tab_ids = tabs
        .into_iter()
        .filter_map(|(tab_id, tab)| {
            let failed = tab.boot_stage == Some(crate::service::boot_stage::BootStage::SpawnFailed);
            let provider_process_live = failed && has_live_provider_process_evidence(service, &tab);
            is_live_for_startup_sweep(&tab, provider_process_live).then_some(tab_id)
        })
        .collect::<Vec<_>>();
    tab_ids.sort();
    tab_ids
}

fn is_live_for_startup_sweep(
    tab: &crate::multiplexer::types::Tab,
    provider_process_live: bool,
) -> bool {
    tab.boot_stage != Some(crate::service::boot_stage::BootStage::SpawnFailed)
        || crate::service::boot_stage::binding_disproves_latched_failure(tab)
        || provider_process_live
}

fn has_live_provider_process_evidence(
    service: &ServiceContext,
    tab: &crate::multiplexer::types::Tab,
) -> bool {
    let (Some(provider_id), Some(pty_id)) = (tab.agent_type.as_deref(), tab.pty_id.as_deref())
    else {
        return false;
    };
    let Some(binary_name) = service
        .agent_registry
        .with_provider(provider_id, |provider| provider.binary_name().to_string())
    else {
        return false;
    };
    let root_pid = {
        let pty = service.pty.lock();
        let Some(session) = pty.get_session(pty_id) else {
            return false;
        };
        let Ok(child) = session.child.lock() else {
            return false;
        };
        let Some(pid) = child.process_id() else {
            return false;
        };
        pid
    };
    crate::service::process_tree::may_have_named_process(root_pid, &binary_name)
}

pub(crate) fn sweep_after_restore(service: &ServiceContext) {
    let task_files = super::task_files::sweep_after_restore(service);
    if !task_files.is_empty() {
        log::info!("[startup] removed orphaned task files: {:?}", task_files);
    }
    let request = LaunchArtifactSweepRequest {
        instance_id: crate::paths::codeterm_instance_id().to_string(),
        live_tab_ids: startup_live_tab_ids(service),
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

#[cfg(test)]
mod tests {
    use super::is_live_for_startup_sweep;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::boot_stage::BootStage;

    #[test]
    fn startup_sweep_keeps_spawn_failed_tab_with_live_provider_process_evidence() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).expect("tab");
        tab.agent_type = Some("process-fixture".into());
        tab.boot_stage = Some(BootStage::SpawnFailed);

        assert!(is_live_for_startup_sweep(tab, true));
        assert!(!is_live_for_startup_sweep(tab, false));
    }
}
