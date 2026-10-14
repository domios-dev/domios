use tauri::Manager;

use crate::multiplexer::types::TabCloseReason;
use crate::service::ServiceContext;

use super::close::PreparedTeamClose;
use super::TeamMutationCaller;

#[cfg(test)]
pub(super) struct StorageTeardownSubmitted(pub std::sync::mpsc::Sender<()>);

pub(super) struct PreparedStorageTeamTeardown {
    pub(super) storage: crate::storage::Storage,
    pub(super) cron: crate::CronState,
    wake: crate::CronWake,
    pub(super) sentinel: crate::ports::sentinel::SentinelPorts,
    pub(super) request: crate::storage::TeamTeardown,
    pub(super) cron_changes: Vec<crate::cron::store::CronChange>,
    cron_tabs: Vec<String>,
    cron_logs: Vec<std::path::PathBuf>,
}

impl ServiceContext {
    pub(super) async fn prepare_storage_team_teardown(
        &self,
        team_id: &str,
        caller: &TeamMutationCaller,
    ) -> Result<(PreparedTeamClose, Option<PreparedStorageTeamTeardown>), String> {
        let Some(storage) = self
            .app_handle
            .try_state::<crate::storage::Storage>()
            .map(|state| state.inner().clone())
        else {
            let mux = self.mux.read();
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
        let cron = self
            .app_handle
            .try_state::<crate::CronState>()
            .ok_or("CronState not registered")?
            .inner()
            .clone();
        let wake = self
            .app_handle
            .try_state::<crate::CronWake>()
            .ok_or("CronWake not registered")?
            .inner()
            .clone();
        let sentinel = self
            .app_handle
            .try_state::<crate::ports::sentinel::SentinelPorts>()
            .ok_or("Sentinel ports not registered")?
            .inner()
            .clone();
        let (prepared, teardown) = {
            let mux = self.mux.read();
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
                    request: crate::storage::TeamTeardown {
                        team_id: team_id.to_string(),
                        expected_desk_team_revision,
                    },
                    cron_changes: vec![crate::cron::store::CronChange::RemoveScope(
                        crate::cron::store::CronScope::Team(team_id.to_string()),
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
        &self,
        team_id: &str,
        teardown: &PreparedStorageTeamTeardown,
    ) {
        let service = self.clone();
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
            if let Err(error) = crate::delayed_send::delete_team_series(&service, &team_id) {
                log::error!("Failed to delete Team delayed series {team_id}: {error}");
            }
        })
        .await
        {
            log::error!("Team cron cleanup task failed: {error}");
        }
        teardown.wake.notify_all();
        teardown.sentinel.wake.notify();
        self.emit_cron_changed();
        self.emit_state_changed_after_bus_publish();
    }
}
