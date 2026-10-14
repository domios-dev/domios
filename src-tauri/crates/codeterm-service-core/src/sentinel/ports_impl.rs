use std::sync::Arc;

use crate::ports::sentinel::{
    BlueprintRecord, BlueprintStore, SentinelCleanup, SentinelPorts, SentinelRuntime, SentinelWake,
};

use super::scheduler::Handle;
use super::storage;
use super::store::Sentinel;

struct HandlePorts(Handle);

pub(super) fn ports(handle: &Handle) -> SentinelPorts {
    let adapter = Arc::new(HandlePorts(handle.clone()));
    SentinelPorts {
        store: adapter.clone(),
        wake: adapter.clone(),
        cleanup: adapter,
    }
}

impl BlueprintStore for HandlePorts {
    fn records(&self) -> Vec<BlueprintRecord> {
        self.0
            .store
            .lock()
            .list()
            .iter()
            .filter_map(|job| {
                let definition = serde_json::to_value(job)
                    .map_err(|error| {
                        log::error!("[blueprint] record {} unserializable: {error}", job.id)
                    })
                    .ok()?;
                Some(BlueprintRecord {
                    id: job.id.clone(),
                    workspace_id: job.workspace_id.clone(),
                    team_id: job.team_id.clone(),
                    scenario_root: job.is_scenario_root(),
                    definition,
                })
            })
            .collect()
    }

    fn scope_of(&self, id: &str) -> Option<(String, Option<String>)> {
        self.0
            .store
            .lock()
            .get(id)
            .map(|job| (job.workspace_id.clone(), job.team_id.clone()))
    }

    fn insert_restored(&self, record: &BlueprintRecord) -> Result<(), String> {
        let job: Sentinel =
            serde_json::from_value(record.definition.clone()).map_err(|error| error.to_string())?;
        storage::commit(&self.0.store, |store| store.stage_insert(job.clone()))
    }

    fn ensure_external_commit_ready(&self) -> Result<(), String> {
        self.0.store.lock().ensure_external_commit_ready()
    }
}

impl SentinelWake for HandlePorts {
    fn notify(&self) {
        self.0.notify();
    }

    fn notify_event_subscription_changed(&self, pipeline_id: &str) {
        self.0.notify_event_subscription_changed(pipeline_id);
    }

    fn terminal_output(&self, tab_id: &str, generation: Option<u64>, bytes: &[u8]) {
        self.0.terminal_output(tab_id, generation, bytes);
    }
}

impl SentinelCleanup for HandlePorts {
    fn cancel_tab_owned_runs(&self, tab_id: &str) -> usize {
        self.0.cancel_tab_owned_runs(tab_id)
    }

    fn remove_worker_watchers(&self, tab_id: &str) -> Result<(), String> {
        self.0.remove_worker_watchers(tab_id)
    }

    fn remove_team(&self, team_id: &str) -> Result<(), String> {
        self.0.remove_team(team_id)
    }

    fn remove_workspace(&self, workspace_id: &str) -> Result<(), String> {
        self.0.remove_workspace(workspace_id)
    }
}

struct HostRuntime;

impl SentinelRuntime for HostRuntime {
    fn start(
        &self,
        svc: crate::service::ServiceState,
        clock: crate::cron::clock::SharedClock,
        storage: &crate::storage::Storage,
    ) -> Option<SentinelPorts> {
        super::scheduler::start(svc, clock, storage)
    }
}

pub fn register_sentinel() {
    crate::ports::sentinel::register_runtime(|| Arc::new(HostRuntime));
}

#[cfg(test)]
pub(crate) fn manage_fixture_ports(app: &crate::app_runtime::AppHandle, handle: &Handle) {
    use tauri::Manager;
    assert!(app.manage(ports(handle)));
}
