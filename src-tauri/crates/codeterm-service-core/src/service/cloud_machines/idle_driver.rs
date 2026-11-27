use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::controller::CloudMachineController;
use super::idle_drain::IdleDecision;
use super::ports::{CloudPeerControlPort, CloudProvisioningPort};
use super::state::DrainingReconcileIntent;
use super::types::{CloudLifecycleState, DEFAULT_LEASE_RENEW_LEAD_MS};

/// Maximum cadence between `read_idle` checks for an awake cloud machine (60 s).
/// The timer acts only as a bound; sleep, delete, and shutdown cancel tasks by event.
pub const IDLE_DRIVER_CADENCE_MS: u64 = 60_000;

#[async_trait]
pub trait IdleTickWaiter: Send + Sync {
    async fn wait_ms(&self, delay_ms: u64);
}

pub struct TokioIdleTickWaiter;

#[async_trait]
impl IdleTickWaiter for TokioIdleTickWaiter {
    async fn wait_ms(&self, delay_ms: u64) {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
}

pub struct CloudIdleDriverConfig {
    pub idle_window_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub max_awake_without_owner_ms: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
    pub lease_renew_lead_ms: u64,
    pub delete_on_stop_billed: bool,
    pub cadence_ms: u64,
}

impl CloudIdleDriverConfig {
    pub fn with_idle_window_reader(idle_window_ms: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self {
            idle_window_ms,
            max_awake_without_owner_ms: Arc::new(|| None),
            lease_renew_lead_ms: DEFAULT_LEASE_RENEW_LEAD_MS,
            delete_on_stop_billed: true,
            cadence_ms: IDLE_DRIVER_CADENCE_MS,
        }
    }

    pub fn with_max_awake_without_owner_reader(
        mut self,
        max_awake_without_owner_ms: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
    ) -> Self {
        self.max_awake_without_owner_ms = max_awake_without_owner_ms;
        self
    }
}

struct MachineTask {
    cancel_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MachineTask {
    fn cancel(self) {
        let _ = self.cancel_tx.send(true);
        self.task.abort();
    }
}

struct ActiveMachineGuard {
    machine_id: String,
    active: Arc<Mutex<HashSet<String>>>,
    task_exit_tx: watch::Sender<u64>,
}

impl Drop for ActiveMachineGuard {
    fn drop(&mut self) {
        self.active.lock().remove(&self.machine_id);
        self.task_exit_tx
            .send_modify(|rev| *rev = rev.wrapping_add(1));
    }
}

pub struct CloudIdleDriverHandle {
    shutdown_tx: watch::Sender<bool>,
    active_machines: Arc<Mutex<HashSet<String>>>,
    supervisor: JoinHandle<()>,
}

impl CloudIdleDriverHandle {
    pub fn active_machine_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.active_machines.lock().iter().cloned().collect();
        ids.sort();
        ids
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl Drop for CloudIdleDriverHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.supervisor.abort();
    }
}

/// Start the home-side per-machine driver for every awake cloud machine owned by `controller`.
/// Rebuilds immediately from persisted records on startup, tracks new wakes/creates via
/// lifecycle events, and event-cancels a machine's task as soon as it sleeps or is deleted.
pub fn spawn_cloud_idle_driver(
    controller: Arc<CloudMachineController>,
    provisioning: Arc<dyn CloudProvisioningPort>,
    peer_control: Arc<dyn CloudPeerControlPort>,
    waiter: Arc<dyn IdleTickWaiter>,
    config: CloudIdleDriverConfig,
) -> CloudIdleDriverHandle {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    // Seed active machines synchronously from persisted Awake and unowned Draining records so
    // callers observing `active_machine_ids()` right after startup never race the first supervisor pass.
    let initial_active: HashSet<String> = controller
        .list_records()
        .into_iter()
        .filter(|rec| {
            rec.state == CloudLifecycleState::Awake
                || (rec.state == CloudLifecycleState::Draining
                    && !controller.has_active_drain(&rec.machine_id))
        })
        .map(|rec| rec.machine_id)
        .collect();
    let active_machines = Arc::new(Mutex::new(initial_active));
    let active_for_supervisor = Arc::clone(&active_machines);
    let config = Arc::new(config);

    let supervisor = tokio::spawn(async move {
        let mut lifecycle_rx = controller.subscribe_lifecycle();
        let (task_exit_tx, mut task_exit_rx) = watch::channel(0u64);
        let mut tasks: HashMap<String, MachineTask> = HashMap::new();

        loop {
            if *shutdown_rx.borrow() {
                break;
            }
            let records = controller.list_records();
            let awake_ids: HashSet<String> = records
                .iter()
                .filter(|rec| rec.state == CloudLifecycleState::Awake)
                .map(|rec| rec.machine_id.clone())
                .collect();
            // A draining machine is mid-sleep inside its own loop; cancelling it would abandon the drain.
            let draining_ids: HashSet<String> = records
                .iter()
                .filter(|rec| rec.state == CloudLifecycleState::Draining)
                .map(|rec| rec.machine_id.clone())
                .collect();

            // Event-cancel any machine that slept, ran out of credits, or was deleted.
            let stale_ids: Vec<String> = tasks
                .iter()
                .filter(|(id, entry)| {
                    (!awake_ids.contains(*id) && !draining_ids.contains(*id))
                        || entry.task.is_finished()
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in stale_ids {
                if let Some(entry) = tasks.remove(&id) {
                    entry.cancel();
                }
                active_for_supervisor.lock().remove(&id);
            }

            // Spawn one per-machine loop for every awake machine and every unowned draining machine
            // so no Draining record is ever left wedged after a restart or lost loop.
            let needed_ids: HashSet<String> = awake_ids
                .iter()
                .cloned()
                .chain(
                    draining_ids
                        .iter()
                        .filter(|id| !controller.has_active_drain(id))
                        .cloned(),
                )
                .collect();
            for machine_id in needed_ids {
                if tasks.contains_key(&machine_id) {
                    continue;
                }
                let (cancel_tx, cancel_rx) = watch::channel(false);
                active_for_supervisor.lock().insert(machine_id.clone());
                let task = tokio::spawn(run_machine_idle_loop(
                    machine_id.clone(),
                    Arc::clone(&controller),
                    Arc::clone(&provisioning),
                    Arc::clone(&peer_control),
                    Arc::clone(&waiter),
                    Arc::clone(&config),
                    Arc::clone(&active_for_supervisor),
                    task_exit_tx.clone(),
                    cancel_rx,
                ));
                tasks.insert(machine_id, MachineTask { cancel_tx, task });
            }
            active_for_supervisor
                .lock()
                .retain(|id| awake_ids.contains(id) || tasks.contains_key(id));

            tokio::select! {
                biased;
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                changed = lifecycle_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                changed = task_exit_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }

        for (_, entry) in tasks.drain() {
            entry.cancel();
        }
    });

    CloudIdleDriverHandle {
        shutdown_tx,
        active_machines,
        supervisor,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_machine_idle_loop(
    machine_id: String,
    controller: Arc<CloudMachineController>,
    provisioning: Arc<dyn CloudProvisioningPort>,
    peer_control: Arc<dyn CloudPeerControlPort>,
    waiter: Arc<dyn IdleTickWaiter>,
    config: Arc<CloudIdleDriverConfig>,
    active_machines: Arc<Mutex<HashSet<String>>>,
    task_exit_tx: watch::Sender<u64>,
    mut cancel_rx: watch::Receiver<bool>,
) {
    let _active_guard = ActiveMachineGuard {
        machine_id: machine_id.clone(),
        active: active_machines,
        task_exit_tx,
    };
    let cadence_ms = config.cadence_ms.max(1);

    loop {
        if *cancel_rx.borrow() {
            break;
        }
        let Some(record) = controller.find_by_peer(&machine_id) else {
            break;
        };
        if record.state == CloudLifecycleState::Draining {
            match controller
                .reconcile_draining(
                    &machine_id,
                    DrainingReconcileIntent::FinishDrainOrRecover,
                    config.delete_on_stop_billed,
                    provisioning.as_ref(),
                    peer_control.as_ref(),
                )
                .await
            {
                Ok(reconciled) => {
                    if reconciled.state != CloudLifecycleState::Awake {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    log::warn!(
                        "[cloud/idle-driver] draining reconciliation failed for {machine_id}: {error}"
                    );
                    tokio::select! {
                        biased;
                        changed = cancel_rx.changed() => {
                            if changed.is_err() || *cancel_rx.borrow() {
                                break;
                            }
                        }
                        _ = waiter.wait_ms(cadence_ms) => {}
                    }
                    continue;
                }
            }
        }
        if record.state != CloudLifecycleState::Awake {
            break;
        }

        // Probe busy snapshot over mesh (never Firebase) and evaluate lease renewal / idle sleep.
        let max_awake_without_owner_ms = (config.max_awake_without_owner_ms)();
        let delay_ms = match peer_control.read_idle(&record).await {
            Ok(busy) => {
                let idle_window_ms = (config.idle_window_ms)();
                match controller
                    .evaluate_idle_tick(
                        &machine_id,
                        busy,
                        idle_window_ms,
                        config.lease_renew_lead_ms,
                        max_awake_without_owner_ms,
                        config.delete_on_stop_billed,
                        provisioning.as_ref(),
                        peer_control.as_ref(),
                    )
                    .await
                {
                    Ok(IdleDecision::BeginTwoPhaseDrain { .. }) => break,
                    Ok(IdleDecision::ScheduleAt { at_ms, .. }) => {
                        let now_ms = controller.now_ms();
                        at_ms.saturating_sub(now_ms).clamp(1, cadence_ms)
                    }
                    Ok(IdleDecision::None | IdleDecision::RenewLeaseNow { .. }) => cadence_ms,
                    Err(error) => {
                        log::warn!(
                            "[cloud/idle-driver] evaluate tick failed for {machine_id}: {error}"
                        );
                        cadence_ms
                    }
                }
            }
            Err(error) => {
                log::debug!(
                    "[cloud/idle-driver] mesh read_idle unavailable for {machine_id}: {error}"
                );
                match controller
                    .reconcile_unreachable_lease(
                        &machine_id,
                        config.lease_renew_lead_ms,
                        max_awake_without_owner_ms,
                        provisioning.as_ref(),
                        peer_control.as_ref(),
                    )
                    .await
                {
                    Ok(IdleDecision::BeginTwoPhaseDrain { .. }) => break,
                    Ok(IdleDecision::ScheduleAt { at_ms, .. }) => {
                        let now_ms = controller.now_ms();
                        at_ms.saturating_sub(now_ms).clamp(1, cadence_ms)
                    }
                    Ok(IdleDecision::None | IdleDecision::RenewLeaseNow { .. }) => cadence_ms,
                    Err(reconcile_error) => {
                        log::warn!(
                            "[cloud/idle-driver] unreachable lease reconciliation failed for {machine_id}: {reconcile_error}"
                        );
                        cadence_ms
                    }
                }
            }
        };

        if controller
            .find_by_peer(&machine_id)
            .is_none_or(|rec| rec.state != CloudLifecycleState::Awake)
        {
            break;
        }

        tokio::select! {
            biased;
            changed = cancel_rx.changed() => {
                if changed.is_err() || *cancel_rx.borrow() {
                    break;
                }
            }
            _ = waiter.wait_ms(delay_ms) => {}
        }
    }
}
