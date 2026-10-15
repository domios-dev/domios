use crate::agent::registry::AgentRegistry;
use codeterm_plugin::provider::{LaunchArtifactSweepReport, LaunchArtifactSweepRequest};

use super::ServiceContext;

mod migration;

fn authorize_profile_cleanup(confirm: bool, machine_owner: bool) -> Result<(), String> {
    if confirm && !machine_owner {
        return Err("Only the machine-owner instance may delete provider launch artifacts".into());
    }
    Ok(())
}

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

pub fn sweep_after_restore(service: &ServiceContext) {
    for error in super::codex_rpc::process::sweep(
        &crate::paths::data_dir().join("codex-rpc"),
        crate::paths::codeterm_instance_id(),
    ) {
        log::error!("[codex-rpc] orphan sweep: {error}");
    }
    let task_files = super::task_files::sweep_after_restore(service);
    if !task_files.is_empty() {
        log::info!("[startup] removed orphaned task files: {:?}", task_files);
    }
    if authorize_profile_cleanup(
        true,
        codeterm_plugin::global_writes::machine_global_write_allowed(),
    )
    .is_err()
    {
        return;
    }
    for provider_id in service.agent_registry.provider_ids() {
        if let Err(error) = startup_provider_sweep(service, &provider_id) {
            log::warn!("[startup] launch artifact sweep failed for {provider_id}: {error}");
        }
    }
}

fn startup_provider_sweep(service: &ServiceContext, provider_id: &str) -> Result<(), String> {
    let migration_root = &service.agent_pad_data_dir;
    let prior = migration::load(migration_root, provider_id);
    if let Err(error) = &prior {
        log::warn!("[startup] legacy migration state unavailable for {provider_id}: {error}");
    }
    let migrate = migration::should_migrate(
        prior
            .as_ref()
            .map(|state| state.as_ref())
            .map_err(|error| error.as_str()),
    );
    let mut state = migration::Migration::default();
    loop {
        let gone = crate::instance_lifetime::lock_gone_instances();
        let request = LaunchArtifactSweepRequest {
            instance_id: crate::paths::codeterm_instance_id().to_string(),
            live_tab_ids: startup_live_tab_ids(service),
            gone_instance_ids: gone
                .iter()
                .map(|instance| instance.instance_id.clone())
                .collect(),
            confirm: true,
            include_legacy: migrate,
        };
        let Some(report) = service
            .agent_registry
            .with_provider(provider_id, |provider| {
                provider.sweep_launch_artifacts(&request)
            })
            .transpose()?
            .flatten()
        else {
            return Ok(());
        };
        let removed_legacy = report
            .legacy
            .iter()
            .filter(|name| report.removed.contains(name))
            .count();
        state.removed += removed_legacy;
        state.retained = report.legacy.len().saturating_sub(removed_legacy);
        if !report.removed.is_empty() {
            log::info!(
                "[startup] released dead provider launch artifacts: {:?}",
                report.removed
            );
        }
        let candidates = report.dead.len() + if migrate { report.legacy.len() } else { 0 };
        if !migration::continue_sweep(candidates, report.removed.len()) {
            break;
        }
    }
    let mut state = if migrate {
        migration::save(migration_root, provider_id, &state)?;
        state
    } else if let Ok(Some(state)) = prior {
        state
    } else {
        return Ok(());
    };
    if migration::should_notify(&state) {
        service.log_system_notice("Provider profile cleanup completed".into(), Some(format!("{provider_id}: removed {} unused legacy launch profiles; {} eligible profiles retained because safe removal was not confirmed. Live tabs and other files were kept.", state.removed, state.retained)), Some("info"));
        state.notified = true;
        migration::save(migration_root, provider_id, &state)?;
    }
    Ok(())
}

pub fn release_closed_tabs(registry: &AgentRegistry, agent_providers: &[(String, String)]) {
    if authorize_profile_cleanup(
        true,
        codeterm_plugin::global_writes::machine_global_write_allowed(),
    )
    .is_err()
    {
        return;
    }
    let instance_id = crate::paths::codeterm_instance_id();
    for (tab_id, provider_id) in agent_providers {
        if let Some(Err(error)) = registry.with_provider(provider_id, |provider| {
            provider.release_tab_launch_artifacts(tab_id, instance_id)
        }) {
            log::warn!("[tab-close] launch artifact release failed for {tab_id}: {error}");
        }
    }
}

pub fn release_tab(
    registry: &AgentRegistry,
    tab_id: &str,
    provider_id: &str,
) -> Result<bool, String> {
    authorize_profile_cleanup(
        true,
        codeterm_plugin::global_writes::machine_global_write_allowed(),
    )?;
    registry
        .with_provider(provider_id, |provider| {
            provider.release_tab_launch_artifacts(tab_id, crate::paths::codeterm_instance_id())
        })
        .ok_or_else(|| format!("Unknown provider: {provider_id}"))?
}

pub fn sweep_for_owner(
    service: &ServiceContext,
    provider_id: &str,
    confirm: bool,
    include_legacy: bool,
) -> Result<LaunchArtifactSweepReport, String> {
    authorize_profile_cleanup(
        confirm,
        codeterm_plugin::global_writes::machine_global_write_allowed(),
    )?;
    let gone = crate::instance_lifetime::lock_gone_instances();
    let request = LaunchArtifactSweepRequest {
        instance_id: crate::paths::codeterm_instance_id().to_string(),
        live_tab_ids: live_tab_ids(service),
        gone_instance_ids: gone
            .iter()
            .map(|instance| instance.instance_id.clone())
            .collect(),
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
    use super::{authorize_profile_cleanup, is_live_for_startup_sweep};
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::boot_stage::BootStage;

    #[test]
    fn profile_cleanup_requires_machine_owner_for_deletion() {
        assert!(authorize_profile_cleanup(true, false).is_err());
        assert!(authorize_profile_cleanup(true, true).is_ok());
    }

    #[test]
    fn profile_cleanup_allows_non_owner_dry_run() {
        assert!(authorize_profile_cleanup(false, false).is_ok());
        assert!(authorize_profile_cleanup(false, true).is_ok());
    }

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
