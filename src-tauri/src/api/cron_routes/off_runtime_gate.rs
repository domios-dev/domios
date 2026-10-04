//! cron_entry_off_runtime_: a Storage-backed cron store whose next commit parks at an
//! event gate after prepare, with the cron lock released.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use parking_lot::Mutex;

use crate::cron::clock::MockClock;
use crate::cron::store::{commit, CronCreateParams, CronJob, CronStorageOptions, CronStore};
use crate::storage::{Storage, StorageConfig};
use crate::{CronState, CronWake};

/// Failure bound around event waits only; elapsed time never signals readiness.
const DEADLINE: Duration = Duration::from_secs(30);

pub(crate) struct StorageCron {
    pub(crate) cron: CronState,
    pub(crate) wake: CronWake,
    storage: Storage,
    _temp: tempfile::TempDir,
}

impl Drop for StorageCron {
    fn drop(&mut self) {
        let _ = self.storage.shutdown().wait(DEADLINE);
    }
}

impl StorageCron {
    pub(crate) fn open() -> Self {
        let temp = tempfile::tempdir().expect("temp root");
        let storage = Storage::open(temp.path().join("instance"), StorageConfig::default())
            .expect("open storage");
        let store = CronStore::open_storage(
            &storage,
            temp.path().join("crons"),
            MockClock::starting_at(1_700_000_000_000),
            CronStorageOptions::default(),
        )
        .expect("hydrate cron storage");
        Self {
            cron: Arc::new(Mutex::new(store)),
            wake: Arc::new(std::sync::Condvar::new()),
            storage,
            _temp: temp,
        }
    }

    pub(crate) fn create(&self, name: &str, workspace_id: &str) -> CronJob {
        let params: CronCreateParams = serde_json::from_value(serde_json::json!({
            "name": name,
            "workspace_id": workspace_id,
            "schedule": "0 * * * *",
            "task": "task",
            "agent": "claude",
            "jitter_max_ms": 0,
        }))
        .expect("create params");
        commit(&self.cron, |store| store.prepare_create(params.clone()))
            .expect("create")
            .value
    }

    pub(crate) fn enabled(&self, job_id: &str) -> Option<bool> {
        self.cron.lock().get(job_id).map(|job| job.enabled)
    }

    pub(crate) fn gate_next_commit(&self) -> CommitGate {
        let (probe, gate) = commit_gate();
        self.cron.lock().set_stage_probe(probe);
        gate
    }
}

pub(crate) struct CommitGate {
    parked: tokio::sync::oneshot::Receiver<()>,
    release: mpsc::Sender<()>,
    expired: Arc<AtomicBool>,
}

impl CommitGate {
    /// Resolves once the gated commit is parked; awaiting it yields the runtime.
    pub(crate) async fn parked(&mut self) {
        (&mut self.parked)
            .await
            .expect("the gated commit reached its gate");
    }

    /// Lets the parked commit finish; false when it stopped waiting at its failure deadline.
    pub(crate) fn release(self) -> bool {
        let _ = self.release.send(());
        !self.expired.load(Ordering::SeqCst)
    }
}

/// Generic over the stage so the probe type is inferred from `set_stage_probe`.
fn commit_gate<S: 'static>() -> (Arc<dyn Fn(S) + Send + Sync>, CommitGate) {
    let (parked_tx, parked) = tokio::sync::oneshot::channel();
    let (release, release_rx) = mpsc::channel::<()>();
    let armed = Mutex::new(Some((parked_tx, release_rx)));
    let expired = Arc::new(AtomicBool::new(false));
    let timed_out = expired.clone();
    let probe: Arc<dyn Fn(S) + Send + Sync> = Arc::new(move |_stage: S| {
        let Some((parked_tx, release_rx)) = armed.lock().take() else {
            return;
        };
        let _ = parked_tx.send(());
        if release_rx.recv_timeout(DEADLINE).is_err() {
            timed_out.store(true, Ordering::SeqCst);
        }
    });
    (
        probe,
        CommitGate {
            parked,
            release,
            expired,
        },
    )
}
