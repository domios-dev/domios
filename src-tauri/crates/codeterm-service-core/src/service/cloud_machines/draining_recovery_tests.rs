use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use super::*;
use crate::mesh::crown::AuthenticatedOrigin;
use crate::service::chat_send_transaction::ChatSendState;

struct FakeClock(AtomicU64);

impl FakeClock {
    fn new(start_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start_ms)))
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct RecoveryProvisioning {
    clock: Arc<FakeClock>,
    sleep_mode: CloudSleepMode,
    usage: Mutex<CloudUsageSummary>,
    renew_outcomes: Mutex<VecDeque<Result<ProvisionedLease, CloudError>>>,
    sleep_outcomes: Mutex<VecDeque<Result<CloudSleepMode, CloudError>>>,
    wake_outcomes: Mutex<VecDeque<Result<(), CloudError>>>,
    renew_calls: AtomicUsize,
    sleep_calls: AtomicUsize,
    wake_calls: AtomicUsize,
}

impl RecoveryProvisioning {
    fn new(clock: Arc<FakeClock>, sleep_mode: CloudSleepMode) -> Arc<Self> {
        Arc::new(Self {
            clock,
            sleep_mode,
            usage: Mutex::new(CloudUsageSummary::default()),
            renew_outcomes: Mutex::new(VecDeque::new()),
            sleep_outcomes: Mutex::new(VecDeque::new()),
            wake_outcomes: Mutex::new(VecDeque::new()),
            renew_calls: AtomicUsize::new(0),
            sleep_calls: AtomicUsize::new(0),
            wake_calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl CloudProvisioningPort for RecoveryProvisioning {
    async fn create(
        &self,
        _request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        Err(CloudError::Internal("unused".into()))
    }

    async fn wake(
        &self,
        machine_id: &str,
        _request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError> {
        let seq = self.wake_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(outcome) = self.wake_outcomes.lock().pop_front() {
            outcome?;
        }
        Ok(ProvisionedMachine {
            machine_id: machine_id.to_owned(),
            provider: "fly_machines".into(),
            endpoint: CloudEndpoint::ProviderProxy {
                host: "domios-recovered.fly.dev".into(),
            },
            size: "small".into(),
            rate: CloudRate {
                amount: 0.20,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: self.sleep_mode,
            lease_id: format!("lease-woken-{seq}"),
            lease_expires_at_ms: self.clock.now_ms().saturating_add(600_000),
        })
    }

    async fn sleep(
        &self,
        _machine_id: &str,
        _request_id: &str,
    ) -> Result<CloudSleepMode, CloudError> {
        self.sleep_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(outcome) = self.sleep_outcomes.lock().pop_front() {
            return outcome;
        }
        Ok(self.sleep_mode)
    }

    async fn delete(&self, _machine_id: &str, _request_id: &str) -> Result<(), CloudError> {
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
            return outcome;
        }
        Ok(ProvisionedLease {
            machine_id: machine_id.to_owned(),
            lease_id: format!("{lease_id}-r{seq}"),
            lease_expires_at_ms: self.clock.now_ms().saturating_add(600_000),
        })
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(self.usage.lock().clone())
    }
}

struct RecoveryPeerControl {
    drain_gate: Arc<LocalPeerDrainGate>,
    reopen_calls: AtomicUsize,
    enter_drain_calls: AtomicUsize,
    reopen_error: Mutex<Option<CloudError>>,
    flushed: Mutex<Vec<String>>,
}

impl RecoveryPeerControl {
    fn new(initially_draining: bool) -> Arc<Self> {
        let gate = Arc::new(LocalPeerDrainGate::default());
        if initially_draining {
            let _ = gate.try_enter_drain("cloud-draining", PeerBusySnapshot::default(), 1_000);
        }
        Arc::new(Self {
            drain_gate: gate,
            reopen_calls: AtomicUsize::new(0),
            enter_drain_calls: AtomicUsize::new(0),
            reopen_error: Mutex::new(None),
            flushed: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl CloudPeerControlPort for RecoveryPeerControl {
    async fn read_idle(
        &self,
        _record: &CloudMachineRecord,
    ) -> Result<PeerBusySnapshot, CloudError> {
        Ok(PeerBusySnapshot::default())
    }

    async fn enter_drain(&self, record: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        self.enter_drain_calls.fetch_add(1, Ordering::SeqCst);
        self.drain_gate
            .try_enter_drain(&record.machine_id, PeerBusySnapshot::default(), 1_000)
    }

    async fn reopen_drain(&self, _record: &CloudMachineRecord) -> Result<(), CloudError> {
        self.reopen_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(err) = self.reopen_error.lock().clone() {
            return Err(err);
        }
        self.drain_gate.reopen();
        Ok(())
    }

    async fn update_endpoint(
        &self,
        _record: &CloudMachineRecord,
        _endpoint: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        Ok(())
    }

    async fn publish_direct_endpoint(
        &self,
        _record: &CloudMachineRecord,
        _endpoint: &CloudEndpoint,
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
        send: &HeldCloudSend,
    ) -> FlushWriteOutcome {
        self.flushed
            .lock()
            .push(send.body["text"].as_str().unwrap_or("").to_owned());
        FlushWriteOutcome::Written
    }
}

fn draining_record(id: &str, awake_at_ms: u64, lease_expires_at_ms: u64) -> CloudMachineRecord {
    CloudMachineRecord {
        machine_id: id.into(),
        name: id.into(),
        provider: "fly_machines".into(),
        size: "small".into(),
        state: CloudLifecycleState::Draining,
        sleep_mode: CloudSleepMode::StopBilled,
        installation_id: Some(format!("inst-{id}")),
        peer_instance_id: Some(format!("peer-{id}")),
        endpoint: Some(CloudEndpoint::Direct(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 10),
            CLOUD_INGRESS_PORT,
        ))),
        rate_per_hour: None,
        created_at_ms: awake_at_ms,
        last_awake_at_ms: Some(awake_at_ms),
        last_owner_interaction_at_ms: Some(awake_at_ms),
        accumulated_awake_secs: 30,
        period_start_ms: awake_at_ms,
        lease_id: Some("lease-stale".into()),
        lease_expires_at_ms: Some(lease_expires_at_ms),
        exhaustion_pending: false,
        idle_since_ms: Some(awake_at_ms + 1_000),
        apply_unconfirmed: false,
        pending_request: Some(PendingProvisioningRequest {
            op: PendingProvisioningOp::Sleep,
            request_id: "stale-req-id".into(),
            started_at_ms: awake_at_ms + 5_000,
            unconfirmed: true,
        }),
        last_error: Some("prior drain interrupted".into()),
        ownership: CloudOwnershipReceipt {
            provider_machine_id: format!("cm_{id}"),
            floating_ip_id: None,
            auxiliary_resource_ids: BTreeMap::new(),
            controller_id: "home-1".into(),
            labels: BTreeMap::new(),
        },
    }
}

fn storage_usage_for(
    machine_id: &str,
    stopped_at: Option<u64>,
    tier: CloudStorageTier,
    remaining: f64,
    allowance: f64,
) -> CloudUsageSummary {
    CloudUsageSummary {
        used: (allowance - remaining).max(0.0),
        allowance,
        purchased: 0.0,
        remaining,
        cap: allowance,
        storage: Some(CloudStorageUsageSummary {
            hot_gb_month_rate_cents: 15,
            hot_days: 14.0,
            purge_days: 90.0,
            purge_warning_days: [14.0, 3.0],
            cold_archive_included: true,
            total_allocated_gb: 10,
            total_used_bytes: 1_024,
            total_hot_monthly_cost_cents: 150,
            machines: vec![CloudMachineStorageUsage {
                machine_id: machine_id.into(),
                create_request_id: None,
                name: None,
                created_at_ms: None,
                tier,
                allocated_gb: 10,
                used_bytes: Some(1_024),
                used_gb: Some(0.001),
                hot_monthly_cost_cents: 150,
                hot_accrued_cents: 1.0,
                cold_included: true,
                stopped_at,
                hot_until_at: None,
                cold_since_at: None,
                purge_at: None,
                archive_id: None,
                archive_bytes: None,
                archive_generation: 0,
                disk_full: false,
                volume_upgrade_max_gb: 500,
                snapshot_residual: None,
            }],
        }),
        ..Default::default()
    }
}

#[test]
fn pure_plan_and_apply_draining_recovery_transitions() {
    // Non-draining states never trigger draining recovery.
    assert_eq!(
        plan_draining_recovery(
            CloudLifecycleState::Awake,
            BackendDrainingTruth::Asleep,
            DrainingReconcileIntent::FinishDrainOrRecover,
            IdleSleepAction::StopProvider,
        ),
        DrainingRecoveryPlan::NotDraining
    );

    // Backend asleep / out_of_credits always settle locally regardless of intent.
    assert_eq!(
        plan_draining_recovery(
            CloudLifecycleState::Draining,
            BackendDrainingTruth::Asleep,
            DrainingReconcileIntent::PrepareForWake,
            IdleSleepAction::DeleteForRecreate,
        ),
        DrainingRecoveryPlan::FinishAsleep
    );
    assert_eq!(
        plan_draining_recovery(
            CloudLifecycleState::Draining,
            BackendDrainingTruth::OutOfCredits,
            DrainingReconcileIntent::FinishDrainOrRecover,
            IdleSleepAction::StopProvider,
        ),
        DrainingRecoveryPlan::FinishOutOfCredits
    );

    // Backend awake resumes sleep during background recovery unless KeepUnderLease,
    // and reopens awake during PrepareForWake.
    assert_eq!(
        plan_draining_recovery(
            CloudLifecycleState::Draining,
            BackendDrainingTruth::Awake,
            DrainingReconcileIntent::FinishDrainOrRecover,
            IdleSleepAction::DeleteForRecreate,
        ),
        DrainingRecoveryPlan::ResumeSleep
    );
    assert_eq!(
        plan_draining_recovery(
            CloudLifecycleState::Draining,
            BackendDrainingTruth::Awake,
            DrainingReconcileIntent::FinishDrainOrRecover,
            IdleSleepAction::KeepUnderLease,
        ),
        DrainingRecoveryPlan::ReopenAwake
    );
    assert_eq!(
        plan_draining_recovery(
            CloudLifecycleState::Draining,
            BackendDrainingTruth::Awake,
            DrainingReconcileIntent::PrepareForWake,
            IdleSleepAction::DeleteForRecreate,
        ),
        DrainingRecoveryPlan::ReopenAwake
    );

    let base = draining_record("cloud-pure", 100_000, 700_000);

    // Asleep settles awake interval, releases lease, and clears pending request.
    let asleep = apply_draining_truth(&base, BackendDrainingTruth::Asleep, 160_000).unwrap();
    assert_eq!(asleep.state, CloudLifecycleState::Asleep);
    assert_eq!(asleep.accumulated_awake_secs, 90);
    assert_eq!(asleep.last_awake_at_ms, None);
    assert_eq!(asleep.lease_id, None);
    assert_eq!(asleep.lease_expires_at_ms, None);
    assert_eq!(asleep.pending_request, None);
    assert_eq!(asleep.last_error, None);

    // OutOfCredits settles awake interval, releases lease, and marks credits error.
    let ooc = apply_draining_truth(&base, BackendDrainingTruth::OutOfCredits, 160_000).unwrap();
    assert_eq!(ooc.state, CloudLifecycleState::OutOfCredits);
    assert_eq!(ooc.accumulated_awake_secs, 90);
    assert_eq!(ooc.lease_id, None);
    assert_eq!(ooc.lease_expires_at_ms, None);
    assert_eq!(ooc.pending_request, None);
    assert!(ooc.last_error.as_deref().unwrap_or("").contains("credits"));

    // Awake restores Awake, preserves awake timestamp, and clears idle/pending state.
    let awake = apply_draining_truth(&base, BackendDrainingTruth::Awake, 160_000).unwrap();
    assert_eq!(awake.state, CloudLifecycleState::Awake);
    assert_eq!(awake.last_awake_at_ms, Some(100_000));
    assert_eq!(awake.idle_since_ms, None);
    assert_eq!(awake.pending_request, None);
    assert_eq!(awake.last_error, None);

    // Applying draining truth to a non-Draining record is refused cleanly.
    let err = apply_draining_truth(&asleep, BackendDrainingTruth::Awake, 200_000).unwrap_err();
    assert_eq!(err.code(), "invalid_transition");

    // Usage inference distinguishes stopped+credits, stopped+exhausted, and still-hot/running.
    assert_eq!(
        infer_backend_truth_from_usage(
            &base,
            &storage_usage_for(
                "cm_cloud-pure",
                Some(150_000),
                CloudStorageTier::Hot,
                5.0,
                10.0
            )
        ),
        Some(BackendDrainingTruth::Asleep)
    );
    assert_eq!(
        infer_backend_truth_from_usage(
            &base,
            &storage_usage_for(
                "cm_cloud-pure",
                Some(150_000),
                CloudStorageTier::Hot,
                0.0,
                10.0
            )
        ),
        Some(BackendDrainingTruth::OutOfCredits)
    );
    assert_eq!(
        infer_backend_truth_from_usage(
            &base,
            &storage_usage_for("cm_cloud-pure", None, CloudStorageTier::Hot, 5.0, 10.0)
        ),
        None
    );
}

#[tokio::test]
async fn wake_from_stale_draining_reconciles_backend_asleep_then_wakes_and_flushes_sends() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    let rec = draining_record("cloud-DPGKwwYH", 100_000, 200_000);
    CloudMachineStore::new(store_path.clone())
        .save(&[rec])
        .unwrap();

    let clock = FakeClock::new(900_000);
    let ctrl = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "home-1".into(),
        clock.clone(),
    ));
    let prov = RecoveryProvisioning::new(clock, CloudSleepMode::StopBilled);
    // Simulate backend having moved the machine to asleep after lapsed lease.
    prov.renew_outcomes
        .lock()
        .push_back(Err(CloudError::Conflict("Machine is not awake".into())));
    let peer = RecoveryPeerControl::new(true);

    let held = ctrl
        .hold_send_for_wake(
            "cloud-DPGKwwYH",
            "tab-1",
            "deliv-wake-draining".into(),
            "POST".into(),
            "/api/tabs/tab-1/send".into(),
            serde_json::json!({"text": "opaque-after-draining-wake"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();

    let receipt = ctrl
        .wake("cloud-DPGKwwYH", prov.as_ref(), peer.as_ref(), 60_000)
        .await
        .expect(
            "wake from stale Draining must reconcile and succeed instead of invalid_transition",
        );
    assert_eq!(receipt.state, CloudLifecycleState::Awake);
    assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 1);
    assert_eq!(prov.wake_calls.load(Ordering::SeqCst), 1);
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 0);
    assert_eq!(peer.reopen_calls.load(Ordering::SeqCst), 1);
    assert!(!peer.drain_gate.is_draining());
    assert_eq!(
        peer.flushed.lock().as_slice(),
        &["opaque-after-draining-wake".to_owned()]
    );
    assert_eq!(
        ctrl.inspect_held_send(&held.send_id).unwrap().state,
        ChatSendState::Written
    );
}

#[tokio::test]
async fn wake_from_stale_draining_handles_backend_awake_and_out_of_credits() {
    // Case 1: Backend says machine is still awake -> reopen drain gate, transition Awake, flush sends without restart.
    {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("cloud_machines.json");
        CloudMachineStore::new(store_path.clone())
            .save(&[draining_record("cloud-still-awake", 100_000, 700_000)])
            .unwrap();

        let clock = FakeClock::new(200_000);
        let ctrl = Arc::new(CloudMachineController::new(
            CloudMachineStore::new(store_path),
            "home-1".into(),
            clock.clone(),
        ));
        let prov = RecoveryProvisioning::new(clock, CloudSleepMode::StopBilled);
        let peer = RecoveryPeerControl::new(true);

        let receipt = ctrl
            .wake("cloud-still-awake", prov.as_ref(), peer.as_ref(), 60_000)
            .await
            .expect("wake from stale Draining when backend is awake must succeed");
        assert_eq!(receipt.state, CloudLifecycleState::Awake);
        assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 1);
        assert_eq!(prov.wake_calls.load(Ordering::SeqCst), 0);
        assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 0);
        assert_eq!(peer.reopen_calls.load(Ordering::SeqCst), 1);
        assert!(!peer.drain_gate.is_draining());
        let saved = ctrl.find_by_peer("cloud-still-awake").unwrap();
        assert_eq!(saved.lease_id.as_deref(), Some("lease-stale-r1"));
        assert_eq!(saved.pending_request, None);
    }

    // Case 2: Backend reports out_of_credits via usage storage -> settles OutOfCredits, returns credits_exhausted.
    {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("cloud_machines.json");
        CloudMachineStore::new(store_path.clone())
            .save(&[draining_record("cloud-ooc-wake", 100_000, 200_000)])
            .unwrap();

        let clock = FakeClock::new(500_000);
        let ctrl = Arc::new(CloudMachineController::new(
            CloudMachineStore::new(store_path),
            "home-1".into(),
            clock.clone(),
        ));
        let prov = RecoveryProvisioning::new(clock, CloudSleepMode::StopBilled);
        *prov.usage.lock() = storage_usage_for(
            "cm_cloud-ooc-wake",
            Some(400_000),
            CloudStorageTier::Hot,
            0.0,
            10.0,
        );
        let peer = RecoveryPeerControl::new(true);
        // Guest unreachable on reopen must not prevent local settlement.
        *peer.reopen_error.lock() = Some(CloudError::ProviderUnavailable("guest offline".into()));

        let err = ctrl
            .wake("cloud-ooc-wake", prov.as_ref(), peer.as_ref(), 60_000)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "credits_exhausted");
        assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 0);
        assert_eq!(prov.wake_calls.load(Ordering::SeqCst), 0);
        assert_eq!(peer.reopen_calls.load(Ordering::SeqCst), 1);
        let saved = ctrl.find_by_peer("cloud-ooc-wake").unwrap();
        assert_eq!(saved.state, CloudLifecycleState::OutOfCredits);
        assert_eq!(saved.lease_id, None);
        assert_eq!(saved.lease_expires_at_ms, None);
    }
}

#[tokio::test]
async fn controller_startup_reconcile_and_explicit_sleep_recover_persisted_draining() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    CloudMachineStore::new(store_path.clone())
        .save(&[
            draining_record("cloud-reconcile-asleep", 100_000, 200_000),
            draining_record("cloud-reconcile-awake", 100_000, 700_000),
        ])
        .unwrap();

    let clock = FakeClock::new(300_000);
    let ctrl = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "home-1".into(),
        clock.clone(),
    ));
    let prov = RecoveryProvisioning::new(clock, CloudSleepMode::StopBilled);
    // First machine is asleep on backend; second machine is still awake on backend.
    prov.renew_outcomes
        .lock()
        .push_back(Err(CloudError::Conflict("Machine is not awake".into())));
    let peer = RecoveryPeerControl::new(true);

    let outcomes = ctrl
        .reconcile_draining_machines(true, prov.as_ref(), peer.as_ref())
        .await;
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(Result::is_ok));

    let m1 = ctrl.find_by_peer("cloud-reconcile-asleep").unwrap();
    assert_eq!(m1.state, CloudLifecycleState::Asleep);
    assert_eq!(m1.lease_id, None);

    let m2 = ctrl.find_by_peer("cloud-reconcile-awake").unwrap();
    assert_eq!(m2.state, CloudLifecycleState::Asleep);
    assert_eq!(m2.lease_id, None);
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 1);
}
