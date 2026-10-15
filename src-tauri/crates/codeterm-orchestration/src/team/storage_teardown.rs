use codeterm_service_core::multiplexer::types::TabCloseReason;
use codeterm_service_core::service::ServiceContext;

use super::close::PreparedTeamClose;
use super::TeamMutationCaller;

#[cfg(test)]
pub(super) struct StorageTeardownSubmitted(pub std::sync::mpsc::Sender<()>);

pub(super) struct PreparedStorageTeamTeardown {
    pub(super) storage: codeterm_service_core::storage::Storage,
    pub(super) cron: codeterm_service_core::CronState,
    wake: codeterm_service_core::CronWake,
    pub(super) sentinel: codeterm_service_core::ports::sentinel::SentinelPorts,
    pub(super) request: codeterm_service_core::storage::TeamTeardown,
    pub(super) cron_changes: Vec<codeterm_service_core::cron::store::CronChange>,
    cron_tabs: Vec<String>,
    cron_logs: Vec<std::path::PathBuf>,
}

pub(super) async fn prepare_storage_team_teardown(
    svc: &ServiceContext,
    team_id: &str,
    caller: &TeamMutationCaller,
) -> Result<(PreparedTeamClose, Option<PreparedStorageTeamTeardown>), String> {
    let Some(storage) = svc
        .try_ext::<codeterm_service_core::startup::StorageAuthority>()
        .and_then(|authority| authority.storage())
        .cloned()
    else {
        let mux = svc.mux.read();
        return Ok((super::prepare_team_close(&mux, team_id, caller)?, None));
    };
    let desk = storage
        .desk()
        .snapshot()
        .await
        .map_err(|error| format!("Team desk snapshot failed: {error}"))?;
    let expected_desk_team_revision = *desk
        .value
        .team_revisions
        .get(team_id)
        .ok_or_else(|| format!("Team {team_id} has no committed desk row"))?;
    let cron = svc
        .try_ext::<codeterm_service_core::CronState>()
        .ok_or("CronState not registered")?
        .clone();
    let wake = svc
        .try_ext::<codeterm_service_core::CronWake>()
        .ok_or("CronWake not registered")?
        .clone();
    let sentinel = svc
        .try_ext::<codeterm_service_core::ports::sentinel::SentinelPorts>()
        .ok_or("Sentinel ports not registered")?
        .clone();
    let (prepared, teardown) = {
        let mux = svc.mux.read();
        let prepared = super::prepare_team_close(&mux, team_id, caller)?;
        let cron_store = cron.lock();
        let _revisions = cron_store.team_teardown_revisions(team_id)?;
        let jobs = cron_store.list_for_team(team_id);
        let mut cron_tabs = std::collections::BTreeSet::new();
        let mut cron_logs = Vec::new();
        for job in jobs {
            cron_tabs.extend(cron_store.tab_owners_for_job(&job.id));
            cron_tabs.extend(
                cron_store
                    .get_history(&job.id)
                    .into_iter()
                    .filter_map(|run| run.tab_id.clone()),
            );
            if let Some(logs) = cron_store.job_logs_dir(&job.id) {
                cron_logs.push(logs);
            }
        }
        (
            prepared,
            PreparedStorageTeamTeardown {
                storage,
                cron: cron.clone(),
                wake,
                sentinel,
                request: codeterm_service_core::storage::TeamTeardown {
                    team_id: team_id.to_string(),
                    expected_desk_team_revision,
                },
                cron_changes: vec![codeterm_service_core::cron::store::CronChange::RemoveScope(
                    codeterm_service_core::cron::store::CronScope::Team(team_id.to_string()),
                )],
                cron_tabs: cron_tabs.into_iter().collect(),
                cron_logs,
            },
        )
    };
    // Last check, after the mux and cron-store guards are dropped.
    teardown.sentinel.store.ensure_external_commit_ready()?;
    Ok((prepared, Some(teardown)))
}

pub(super) async fn finish_storage_team_teardown(
    svc: &ServiceContext,
    team_id: &str,
    teardown: &PreparedStorageTeamTeardown,
) {
    let service = svc.clone();
    let tabs = teardown.cron_tabs.clone();
    let logs = teardown.cron_logs.clone();
    let team_id = team_id.to_string();
    if let Err(error) = tokio::task::spawn_blocking(move || {
        for tab_id in tabs {
            let _ = service.close_tab_with_reason(&tab_id, TabCloseReason::Cancelled);
        }
        for directory in logs {
            let _ = std::fs::remove_dir_all(directory);
        }
        if let Err(error) =
            codeterm_service_core::delayed_send::delete_team_series(&service, &team_id)
        {
            log::error!("Failed to delete Team delayed series {team_id}: {error}");
        }
    })
    .await
    {
        log::error!("Team cron cleanup task failed: {error}");
    }
    teardown.wake.notify_all();
    teardown.sentinel.wake.notify();
    svc.emit_cron_changed();
    svc.emit_state_changed_after_bus_publish();
}
