use std::sync::Arc;

use codeterm_sentinel::store::{Sentinel, Store};
use codeterm_service_core::ports::sentinel::{
    BlueprintRecord, BlueprintStore, SentinelCleanup, SentinelPorts, SentinelWake,
};
use parking_lot::Mutex;

struct FixtureSentinels(Arc<Mutex<Store>>);

pub(super) fn manage(
    svc: &codeterm_service_core::service::ServiceContext,
    store: &Arc<Mutex<Store>>,
) {
    let adapter = Arc::new(FixtureSentinels(store.clone()));
    assert!(svc
        .set_ext(SentinelPorts {
            store: adapter.clone(),
            wake: adapter.clone(),
            cleanup: adapter,
        })
        .is_ok());
}

impl BlueprintStore for FixtureSentinels {
    fn records(&self) -> Vec<BlueprintRecord> {
        self.0
            .lock()
            .list()
            .iter()
            .map(|job| BlueprintRecord {
                id: job.id.clone(),
                workspace_id: job.workspace_id.clone(),
                team_id: job.team_id.clone(),
                scenario_root: job.is_scenario_root(),
                definition: serde_json::to_value(job).expect("fixture sentinel serializes"),
            })
            .collect()
    }

    fn scope_of(&self, id: &str) -> Option<(String, Option<String>)> {
        self.0
            .lock()
            .get(id)
            .map(|job| (job.workspace_id.clone(), job.team_id.clone()))
    }

    fn insert_restored(&self, record: &BlueprintRecord) -> Result<(), String> {
        let job: Sentinel =
            serde_json::from_value(record.definition.clone()).map_err(|error| error.to_string())?;
        codeterm_sentinel::storage::commit(&self.0, |store| store.stage_insert(job.clone()))
            .map(|_| ())
    }

    fn ensure_external_commit_ready(&self) -> Result<(), String> {
        self.0.lock().ensure_external_commit_ready()
    }

    fn adopt_external_commit(
        &self,
        committed: &codeterm_service_core::storage::Committed<
            codeterm_service_core::storage::SentinelCommit,
        >,
    ) -> Result<(), String> {
        self.0.lock().adopt_external_commit(committed)
    }
}

impl SentinelWake for FixtureSentinels {
    fn notify(&self) {}

    fn notify_event_subscription_changed(&self, _pipeline_id: &str) {}

    fn terminal_output(&self, _tab_id: &str, _generation: Option<u64>, _bytes: &[u8]) {}
}

impl SentinelCleanup for FixtureSentinels {
    fn cancel_tab_owned_runs(&self, tab_id: &str) -> usize {
        self.0.lock().cancel_tab_owned_runs(tab_id)
    }

    fn remove_worker_watchers(&self, tab_id: &str) -> Result<(), String> {
        codeterm_sentinel::cleanup::remove_owned(&self.0, tab_id)
    }

    fn remove_team(&self, team_id: &str) -> Result<(), String> {
        codeterm_sentinel::storage::commit(&self.0, |store| store.stage_remove_team(team_id))
    }

    fn remove_workspace(&self, workspace_id: &str) -> Result<(), String> {
        let jobs: Vec<_> = self
            .0
            .lock()
            .list()
            .iter()
            .filter(|job| job.workspace_id == workspace_id)
            .cloned()
            .collect();
        for job in jobs {
            if self.0.lock().get(&job.id).is_none() {
                continue;
            }
            codeterm_sentinel::storage::commit(&self.0, |store| {
                store.stage_delete_scenario(&job.id, workspace_id, job.team_id.as_deref())
            })?;
        }
        Ok(())
    }
}
