use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::Notify;

use super::*;

struct InjectedClock(AtomicU64);

impl InjectedClock {
    fn new(start_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start_ms)))
    }

    fn advance_ms(&self, delta_ms: u64) {
        self.0.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

impl Clock for InjectedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Deterministic waiter: advances the injected clock by each scheduled `delay_ms`
/// once the test releases one step and waits for the driver to complete that tick,
/// with zero wall-clock sleeps.
struct WaiterState {
    parked_count: usize,
    entered_seq: u64,
    released_seq: u64,
}

struct ParkedGuard<'a>(&'a Mutex<WaiterState>);

impl Drop for ParkedGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.parked_count = state.parked_count.saturating_sub(1);
    }
}

struct SteppingWaiter {
    clock: Arc<InjectedClock>,
    recorded_delays: Mutex<Vec<u64>>,
    state: Mutex<WaiterState>,
    entered: Notify,
    release: Notify,
}

impl SteppingWaiter {
    fn new(clock: Arc<InjectedClock>) -> Arc<Self> {
        Arc::new(Self {
            clock,
            recorded_delays: Mutex::new(Vec::new()),
            state: Mutex::new(WaiterState {
                parked_count: 0,
                entered_seq: 0,
                released_seq: 0,
            }),
            entered: Notify::new(),
            release: Notify::new(),
        })
    }

    async fn wait_until_parked(&self, expected_count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = self.entered.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.state.lock().parked_count >= expected_count {
                    return;
                }
                notified.await;
            }
        })
        .await
        .expect("timed out waiting for idle driver to park on waiter");
    }

    /// Release one parked tick, advance the injected clock, and wait until the driver finishes
    /// the resulting tick (either parking for the next cadence or satisfying `done`).
    async fn advance_step_until(&self, done: impl Fn() -> bool) {
        let target_seq = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = self.entered.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if done() {
                    return None;
                }
                let state = self.state.lock();
                if state.entered_seq > state.released_seq && state.parked_count >= 1 {
                    return Some(state.entered_seq);
                }
                drop(state);
                notified.await;
            }
        })
        .await
        .expect("timed out waiting for driver to park before step");

        let Some(target_seq) = target_seq else {
            return;
        };

        {
            let mut state = self.state.lock();
            state.released_seq = state.released_seq.max(target_seq);
        }
        self.release.notify_waiters();

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = self.entered.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if done() || self.state.lock().entered_seq > target_seq {
                    return;
                }
                tokio::select! {
                    _ = notified => {}
                    _ = tokio::task::yield_now() => {}
                }
            }
        })
        .await
        .expect("timed out waiting for driver to finish stepped tick");
    }

    async fn advance_step(&self) {
        self.advance_step_until(|| false).await;
    }
}

#[async_trait]
impl IdleTickWaiter for SteppingWaiter {
    async fn wait_ms(&self, delay_ms: u64) {
        self.recorded_delays.lock().push(delay_ms);
        let my_seq = {
            let mut state = self.state.lock();
            state.entered_seq += 1;
            state.parked_count += 1;
            state.entered_seq
        };
        let _parked_guard = ParkedGuard(&self.state);
        self.entered.notify_waiters();
        loop {
            let notified = self.release.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.state.lock().released_seq >= my_seq {
                break;
            }
            notified.await;
        }
        self.clock.advance_ms(delay_ms);
    }
}

struct DriverProvisioning {
    clock: Arc<InjectedClock>,
    lease_duration_ms: u64,
    renew_calls: AtomicUsize,
    sleep_calls: AtomicUsize,
    delete_calls: AtomicUsize,
    renew_outcomes: Mutex<VecDeque<Result<(), CloudError>>>,
}

impl DriverProvisioning {
    fn new(clock: Arc<InjectedClock>, lease_duration_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            clock,
            lease_duration_ms,
            renew_calls: AtomicUsize::new(0),
            sleep_calls: AtomicUsize::new(0),
            delete_calls: AtomicUsize::new(0),
            renew_outcomes: Mutex::new(VecDeque::new()),
        })
    }
}

#[async_trait]
impl CloudProvisioningPort for DriverProvisioning {
    async fn create(
        &self,
        _request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        Err(CloudError::Internal("unused".into()))
    }

    async fn wake(
        &self,
        _machine_id: &str,
        _request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError> {
        Err(CloudError::Internal("unused".into()))
    }

    async fn sleep(
        &self,
        _machine_id: &str,
        _request_id: &str,
    ) -> Result<CloudSleepMode, CloudError> {
        self.sleep_calls.fetch_add(1, Ordering::SeqCst);
        Ok(CloudSleepMode::DeleteRecreate)
    }

    async fn delete(&self, _machine_id: &str, _request_id: &str) -> Result<(), CloudError> {
        self.delete_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        _request_id: &str,
    ) -> Result<ProvisionedLease, CloudError> {
        let seq = self.renew_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(outcome) = self.renew_outcomes.lock().pop_front() {
            outcome?;
        }
        Ok(ProvisionedLease {
            machine_id: machine_id.to_owned(),
            lease_id: format!("{lease_id}-r{seq}"),
            lease_expires_at_ms: self.clock.now_ms().saturating_add(self.lease_duration_ms),
        })
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(CloudUsageSummary::default())
    }
}

struct DriverPeerControl {
    busy: Mutex<PeerBusySnapshot>,
    read_idle_error: Mutex<Option<String>>,
    read_idle_calls: AtomicUsize,
    drain_calls: AtomicUsize,
}

impl DriverPeerControl {
    fn new(busy: PeerBusySnapshot) -> Arc<Self> {
        Arc::new(Self {
            busy: Mutex::new(busy),
            read_idle_error: Mutex::new(None),
            read_idle_calls: AtomicUsize::new(0),
            drain_calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl CloudPeerControlPort for DriverPeerControl {
    async fn read_idle(
        &self,
        _record: &CloudMachineRecord,
    ) -> Result<PeerBusySnapshot, CloudError> {
        self.read_idle_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(err) = self.read_idle_error.lock().clone() {
            return Err(CloudError::ProviderUnavailable(err));
        }
        Ok(*self.busy.lock())
    }

    async fn enter_drain(&self, _record: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        self.drain_calls.fetch_add(1, Ordering::SeqCst);
        Ok(DrainToken {
            token: "drain-tok".into(),
            entered_at_ms: 0,
        })
    }

    async fn reopen_drain(&self, _record: &CloudMachineRecord) -> Result<(), CloudError> {
        Ok(())
    }

    async fn update_endpoint(
        &self,
        _record: &CloudMachineRecord,
        _endpoint: SocketAddrV4,
    ) -> Result<(), CloudError> {
        Ok(())
    }

    async fn publish_direct_endpoint(
        &self,
        _record: &CloudMachineRecord,
        _endpoint: SocketAddrV4,
    ) -> Result<(), CloudError> {
        Ok(())
    }

    async fn await_peer_ready(
        &self,
        _record: &CloudMachineRecord,
        _deadline_ms: u64,
    ) -> Result<(), CloudError> {
        Ok(())
    }

    async fn flush_send(
        &self,
        _record: &CloudMachineRecord,
        _send: &HeldCloudSend,
    ) -> FlushWriteOutcome {
        FlushWriteOutcome::Delivered
    }
}

fn awake_record(id: &str, now_ms: u64, lease_expires_at_ms: u64) -> CloudMachineRecord {
    CloudMachineRecord {
        machine_id: id.into(),
        name: id.into(),
        provider: "timeweb".into(),
        size: "small".into(),
        state: CloudLifecycleState::Awake,
        sleep_mode: CloudSleepMode::DeleteRecreate,
        installation_id: Some(format!("inst-{id}")),
        peer_instance_id: Some(format!("peer-{id}")),
        endpoint_addr: Some(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 10),
            CLOUD_INGRESS_PORT,
        )),
        rate_per_hour: None,
        created_at_ms: now_ms,
        last_awake_at_ms: Some(now_ms),
        last_owner_interaction_at_ms: Some(now_ms),
        accumulated_awake_secs: 0,
        period_start_ms: now_ms,
        lease_id: Some("lease-0".into()),
        lease_expires_at_ms: Some(lease_expires_at_ms),
        exhaustion_pending: false,
        idle_since_ms: None,
        apply_unconfirmed: false,
        pending_request: None,
        last_error: None,
        ownership: CloudOwnershipReceipt {
            provider_machine_id: format!("prov-{id}"),
            floating_ip_id: None,
            auxiliary_resource_ids: BTreeMap::new(),
            controller_id: "home-1".into(),
            labels: BTreeMap::new(),
        },
    }
}

async fn wait_for_active_ids(handle: &CloudIdleDriverHandle, expected: &[&str]) {
    let want: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if handle.active_machine_ids() == want {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out waiting for active_machine_ids={want:?}, got={:?}",
            handle.active_machine_ids()
        )
    });
}

#[tokio::test]
async fn idle_driver_renews_while_busy_across_all_signals_then_sleeps_after_idle_window() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    let start_ms = 1_000_000;
    let lease_duration_ms = 600_000; // 10 minutes
    let idle_window_ms = 15 * 60_000; // 15 minutes

    // Persist an awake machine to disk before controller construction (proves restart rebuild).
    let store = CloudMachineStore::new(store_path.clone());
    store
        .save(&[awake_record(
            "cloud-1",
            start_ms,
            start_ms + lease_duration_ms,
        )])
        .unwrap();

    let clock = InjectedClock::new(start_ms);
    let ctrl = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "home-1".into(),
        clock.clone(),
    ));
    let prov = DriverProvisioning::new(clock.clone(), lease_duration_ms);
    let peer = DriverPeerControl::new(PeerBusySnapshot {
        active_agent_turns: 1,
        ..Default::default()
    });
    let waiter = SteppingWaiter::new(clock.clone());

    let handle = spawn_cloud_idle_driver(
        ctrl.clone(),
        prov.clone(),
        peer.clone(),
        waiter.clone(),
        CloudIdleDriverConfig::with_idle_window_reader(Arc::new(move || idle_window_ms)),
    );
    wait_for_active_ids(&handle, &["cloud-1"]).await;

    // Cycle through all 4 busy signals for 18 virtual minutes (> 15 min idle window):
    // must renew lease before expiry (at 8m and 16m), call read_idle on 60s cadence, and NEVER sleep.
    let busy_states = [
        PeerBusySnapshot {
            active_agent_turns: 1,
            ..Default::default()
        },
        PeerBusySnapshot {
            running_background_jobs: 1,
            ..Default::default()
        },
        PeerBusySnapshot {
            held_sends: 1,
            ..Default::default()
        },
        PeerBusySnapshot {
            attached_viewers: 1,
            ..Default::default()
        },
    ];
    for step in 0..18 {
        *peer.busy.lock() = busy_states[step % busy_states.len()];
        waiter.advance_step().await;
    }
    waiter.wait_until_parked(1).await;

    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 0);
    assert_eq!(peer.drain_calls.load(Ordering::SeqCst), 0);
    assert!(
        prov.renew_calls.load(Ordering::SeqCst) >= 2,
        "expected at least 2 renewals across 18 busy minutes, got {}",
        prov.renew_calls.load(Ordering::SeqCst)
    );
    assert!(
        waiter
            .recorded_delays
            .lock()
            .iter()
            .all(|&d| d <= IDLE_DRIVER_CADENCE_MS),
        "driver cadence must stay bounded at 60s"
    );
    assert_eq!(
        ctrl.find_by_peer("cloud-1").unwrap().state,
        CloudLifecycleState::Awake
    );

    // Switch to idle: after 15 virtual minutes of idle ticks, driver drains, sleeps, and stops.
    *peer.busy.lock() = PeerBusySnapshot::default();
    for _ in 0..17 {
        if ctrl.find_by_peer("cloud-1").unwrap().state == CloudLifecycleState::Asleep {
            break;
        }
        waiter
            .advance_step_until(|| {
                ctrl.find_by_peer("cloud-1").unwrap().state == CloudLifecycleState::Asleep
            })
            .await;
    }
    wait_for_active_ids(&handle, &[]).await;

    assert_eq!(
        ctrl.find_by_peer("cloud-1").unwrap().state,
        CloudLifecycleState::Asleep
    );
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 1);
    assert_eq!(peer.drain_calls.load(Ordering::SeqCst), 1);
    assert!(peer.read_idle_calls.load(Ordering::SeqCst) >= 18);
}

#[tokio::test]
async fn idle_driver_rebuilds_on_restart_and_cancels_immediately_on_sleep_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    let start_ms = 500_000;
    CloudMachineStore::new(store_path.clone())
        .save(&[
            awake_record("cloud-a", start_ms, start_ms + 600_000),
            awake_record("cloud-b", start_ms, start_ms + 600_000),
        ])
        .unwrap();

    let clock = InjectedClock::new(start_ms);
    let ctrl = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "home-1".into(),
        clock.clone(),
    ));
    let prov = DriverProvisioning::new(clock.clone(), 600_000);
    let peer = DriverPeerControl::new(PeerBusySnapshot::default());
    let waiter = SteppingWaiter::new(clock);

    let handle = spawn_cloud_idle_driver(
        ctrl.clone(),
        prov.clone(),
        peer.clone(),
        waiter.clone(),
        CloudIdleDriverConfig::with_idle_window_reader(Arc::new(|| 15 * 60_000)),
    );
    // Both persisted awake machines immediately have active drivers and park on the waiter.
    wait_for_active_ids(&handle, &["cloud-a", "cloud-b"]).await;
    waiter.wait_until_parked(2).await;

    // Sleep cloud-a while its driver is parked: must event-cancel without advancing the timer.
    ctrl.sleep("cloud-a", prov.as_ref(), peer.as_ref(), true)
        .await
        .unwrap();
    wait_for_active_ids(&handle, &["cloud-b"]).await;

    // Delete cloud-b while its driver is parked: must event-cancel immediately too.
    ctrl.delete("cloud-b", true, prov.as_ref()).await.unwrap();
    wait_for_active_ids(&handle, &[]).await;
}

#[tokio::test]
async fn offline_home_lapsed_lease_conflict_reconciles_local_state_to_asleep_and_stops_driver() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    // Lease already expired while home was offline; Firebase reconciliation put machine asleep.
    let expired_at = 200_000;
    let now_after_offline = 900_000;
    CloudMachineStore::new(store_path.clone())
        .save(&[awake_record("cloud-offline", 100_000, expired_at)])
        .unwrap();

    let clock = InjectedClock::new(now_after_offline);
    let ctrl = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "home-1".into(),
        clock.clone(),
    ));
    let prov = DriverProvisioning::new(clock.clone(), 600_000);
    prov.renew_outcomes
        .lock()
        .push_back(Err(CloudError::Conflict("Machine is not awake".into())));
    let peer = DriverPeerControl::new(PeerBusySnapshot {
        active_agent_turns: 1,
        ..Default::default()
    });
    let waiter = SteppingWaiter::new(clock);

    let handle = spawn_cloud_idle_driver(
        ctrl.clone(),
        prov,
        peer,
        waiter,
        CloudIdleDriverConfig::with_idle_window_reader(Arc::new(|| 15 * 60_000)),
    );
    wait_for_active_ids(&handle, &[]).await;
    assert_eq!(
        ctrl.find_by_peer("cloud-offline").unwrap().state,
        CloudLifecycleState::Asleep
    );
}

#[tokio::test]
async fn idle_machine_renews_lease_before_expiry_without_resetting_idle_window_then_drains_and_sleeps(
) {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    // 600 s lease (expires at t=610_000, renew lead 120 s -> renew at t=490_000) vs 15 min idle window (900_000 ms).
    CloudMachineStore::new(store_path.clone())
        .save(&[awake_record("cloud-idle-renew", 10_000, 610_000)])
        .unwrap();

    let clock = InjectedClock::new(10_000);
    let ctrl = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "home-1".into(),
        clock.clone(),
    ));
    let prov = DriverProvisioning::new(clock.clone(), 600_000);
    let peer = DriverPeerControl::new(PeerBusySnapshot::default());
    let waiter = SteppingWaiter::new(clock.clone());

    let handle = spawn_cloud_idle_driver(
        ctrl.clone(),
        prov.clone(),
        peer.clone(),
        waiter.clone(),
        CloudIdleDriverConfig::with_idle_window_reader(Arc::new(|| 15 * 60_000)),
    );
    waiter.wait_until_parked(1).await;
    assert_eq!(
        ctrl.find_by_peer("cloud-idle-renew").unwrap().idle_since_ms,
        Some(10_000)
    );
    assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 0);

    // Advance minute-by-minute past the minute-8 renewal deadline (t=490_000 = 8 * 60_000 + 10_000).
    for _ in 0..8 {
        waiter.advance_step().await;
    }
    assert_eq!(clock.now_ms(), 490_000);
    assert_eq!(
        prov.renew_calls.load(Ordering::SeqCst),
        1,
        "idle machine must renew lease before minute 10 so backend dead-man switch does not kill it"
    );
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        ctrl.find_by_peer("cloud-idle-renew").unwrap().idle_since_ms,
        Some(10_000),
        "renewing an idle machine's lease must preserve idle_since_ms instead of resetting the 15 min countdown"
    );

    // Advance the remaining 7 minutes to t=910_000 (15 min idle): must cleanly two-phase drain and sleep.
    for _ in 0..7 {
        waiter
            .advance_step_until(|| {
                ctrl.find_by_peer("cloud-idle-renew")
                    .is_some_and(|r| r.state == CloudLifecycleState::Asleep)
            })
            .await;
    }
    wait_for_active_ids(&handle, &[]).await;
    assert_eq!(clock.now_ms(), 910_000);
    assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 1);
    assert_eq!(peer.drain_calls.load(Ordering::SeqCst), 1);
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        ctrl.find_by_peer("cloud-idle-renew").unwrap().state,
        CloudLifecycleState::Asleep
    );
}

#[tokio::test]
async fn unreachable_peer_reconciles_lease_past_renew_window_to_asleep_and_out_of_credits() {
    // Case 1: Peer is unreachable on mesh; backend already slept it (Conflict) -> transitions to Asleep.
    {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("cloud_machines.json");
        CloudMachineStore::new(store_path.clone())
            .save(&[awake_record("cloud-unreach-sleep", 10_000, 610_000)])
            .unwrap();

        let clock = InjectedClock::new(10_000);
        let ctrl = Arc::new(CloudMachineController::new(
            CloudMachineStore::new(store_path),
            "home-1".into(),
            clock.clone(),
        ));
        let prov = DriverProvisioning::new(clock.clone(), 600_000);
        prov.renew_outcomes
            .lock()
            .push_back(Err(CloudError::Conflict("Machine is not awake".into())));
        let peer = DriverPeerControl::new(PeerBusySnapshot::default());
        *peer.read_idle_error.lock() = Some("mesh transport unreachable".into());
        let waiter = SteppingWaiter::new(clock.clone());

        let handle = spawn_cloud_idle_driver(
            ctrl.clone(),
            prov.clone(),
            peer.clone(),
            waiter.clone(),
            CloudIdleDriverConfig::with_idle_window_reader(Arc::new(|| 15 * 60_000)),
        );
        waiter.wait_until_parked(1).await;
        // Before the renew window (t=10_000 < 490_000), unreachable read_idle must not hit backend yet.
        assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            ctrl.find_by_peer("cloud-unreach-sleep").unwrap().state,
            CloudLifecycleState::Awake
        );

        // Advance to renew window (t=490_000): reconciles against backend, gets Conflict -> Asleep & stops driver.
        for _ in 0..8 {
            waiter
                .advance_step_until(|| {
                    ctrl.find_by_peer("cloud-unreach-sleep")
                        .is_some_and(|r| r.state == CloudLifecycleState::Asleep)
                })
                .await;
        }
        wait_for_active_ids(&handle, &[]).await;
        assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            ctrl.find_by_peer("cloud-unreach-sleep").unwrap().state,
            CloudLifecycleState::Asleep
        );
    }

    // Case 2: Peer is unreachable past lease expiry and credits are exhausted -> transitions to OutOfCredits.
    {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("cloud_machines.json");
        CloudMachineStore::new(store_path.clone())
            .save(&[awake_record("cloud-unreach-ooc", 10_000, 610_000)])
            .unwrap();

        let clock = InjectedClock::new(620_000);
        let ctrl = Arc::new(CloudMachineController::new(
            CloudMachineStore::new(store_path),
            "home-1".into(),
            clock.clone(),
        ));
        let prov = DriverProvisioning::new(clock.clone(), 600_000);
        prov.renew_outcomes
            .lock()
            .push_back(Err(CloudError::CreditsExhausted));
        let peer = DriverPeerControl::new(PeerBusySnapshot::default());
        *peer.read_idle_error.lock() = Some("mesh transport unreachable".into());
        let waiter = SteppingWaiter::new(clock);

        let handle = spawn_cloud_idle_driver(
            ctrl.clone(),
            prov.clone(),
            peer,
            waiter,
            CloudIdleDriverConfig::with_idle_window_reader(Arc::new(|| 15 * 60_000)),
        );
        wait_for_active_ids(&handle, &[]).await;
        assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            ctrl.find_by_peer("cloud-unreach-ooc").unwrap().state,
            CloudLifecycleState::OutOfCredits
        );
    }
}

#[tokio::test]
async fn max_awake_without_owner_ceiling_sleeps_busy_machine_and_owner_interaction_extends_window()
{
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    // Awake at t=10_000 with 10 min ceiling (600_000 ms -> fires at 610_000 if no owner interaction).
    CloudMachineStore::new(store_path.clone())
        .save(&[awake_record("cloud-ceiling", 10_000, 3_600_000)])
        .unwrap();

    let clock = InjectedClock::new(10_000);
    let ctrl = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "home-1".into(),
        clock.clone(),
    ));
    let prov = DriverProvisioning::new(clock.clone(), 3_600_000);
    // Keep machine continuously busy (viewer + agent turn) so normal idle sleep never fires.
    let peer = DriverPeerControl::new(PeerBusySnapshot {
        active_agent_turns: 1,
        attached_viewers: 1,
        ..Default::default()
    });
    let waiter = SteppingWaiter::new(clock.clone());

    let handle = spawn_cloud_idle_driver(
        ctrl.clone(),
        prov.clone(),
        peer.clone(),
        waiter.clone(),
        CloudIdleDriverConfig::with_idle_window_reader(Arc::new(|| 15 * 60_000))
            .with_max_awake_without_owner_reader(Arc::new(|| Some(10 * 60_000))),
    );
    waiter.wait_until_parked(1).await;

    // Advance 5 minutes (t=310_000) and record an owner interaction -> ceiling shifts from 610_000 to 910_000.
    for _ in 0..5 {
        waiter.advance_step().await;
    }
    assert_eq!(clock.now_ms(), 310_000);
    ctrl.record_owner_interaction("cloud-ceiling");

    // At t=610_000 (original ceiling), machine must STILL be awake because owner interacted at t=310_000.
    for _ in 0..5 {
        waiter.advance_step().await;
    }
    assert_eq!(clock.now_ms(), 610_000);
    assert_eq!(
        ctrl.find_by_peer("cloud-ceiling").unwrap().state,
        CloudLifecycleState::Awake
    );
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 0);

    // Advance 5 more minutes to t=910_000 (10 min since last owner interaction): ceiling sleeps the busy machine.
    for _ in 0..5 {
        waiter
            .advance_step_until(|| {
                ctrl.find_by_peer("cloud-ceiling")
                    .is_some_and(|r| r.state == CloudLifecycleState::Asleep)
            })
            .await;
    }
    wait_for_active_ids(&handle, &[]).await;
    assert_eq!(clock.now_ms(), 910_000);
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        ctrl.find_by_peer("cloud-ceiling").unwrap().state,
        CloudLifecycleState::Asleep
    );
}
