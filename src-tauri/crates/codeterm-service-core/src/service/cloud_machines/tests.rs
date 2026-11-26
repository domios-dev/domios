use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use zeroize::Zeroizing;

use super::*;
use crate::mesh::crown::AuthenticatedOrigin;
use crate::service::chat_send_transaction::ChatSendState;

struct FakeClock(AtomicU64);

impl FakeClock {
    fn new(start_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start_ms)))
    }

    fn advance_ms(&self, delta: u64) {
        self.0.fetch_add(delta, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct FakeProvisioningPort {
    sleep_mode: Mutex<CloudSleepMode>,
    rate: Mutex<CloudRate>,
    create_ip: Mutex<Ipv4Addr>,
    wake_ip: Mutex<Ipv4Addr>,
    lease_duration_ms: AtomicU64,
    clock: Arc<FakeClock>,
    create_calls: AtomicUsize,
    wake_calls: AtomicUsize,
    sleep_calls: AtomicUsize,
    delete_calls: AtomicUsize,
    renew_calls: AtomicUsize,
    seen_create_request_ids: Mutex<Vec<String>>,
    create_outcomes: Mutex<VecDeque<Result<(), CloudError>>>,
    wake_outcomes: Mutex<VecDeque<Result<(), CloudError>>>,
    renew_outcomes: Mutex<VecDeque<Result<(), CloudError>>>,
    wake_gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
    usage: Mutex<CloudUsageSummary>,
    created_by_req_id: Mutex<BTreeMap<String, ProvisionedMachine>>,
    next_id: AtomicUsize,
}

impl FakeProvisioningPort {
    fn new(sleep_mode: CloudSleepMode, clock: Arc<FakeClock>) -> Arc<Self> {
        let mut burn = BTreeMap::new();
        burn.insert("small".into(), 1.0);
        burn.insert("medium".into(), 2.0);
        Arc::new(Self {
            sleep_mode: Mutex::new(sleep_mode),
            rate: Mutex::new(CloudRate {
                amount: 0.20,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            }),
            create_ip: Mutex::new(Ipv4Addr::new(198, 51, 100, 10)),
            wake_ip: Mutex::new(Ipv4Addr::new(198, 51, 100, 42)),
            lease_duration_ms: AtomicU64::new(600_000),
            clock,
            create_calls: AtomicUsize::new(0),
            wake_calls: AtomicUsize::new(0),
            sleep_calls: AtomicUsize::new(0),
            delete_calls: AtomicUsize::new(0),
            renew_calls: AtomicUsize::new(0),
            seen_create_request_ids: Mutex::new(Vec::new()),
            create_outcomes: Mutex::new(VecDeque::new()),
            wake_outcomes: Mutex::new(VecDeque::new()),
            renew_outcomes: Mutex::new(VecDeque::new()),
            wake_gate: Mutex::new(None),
            usage: Mutex::new(CloudUsageSummary {
                used: 2.5,
                allowance: 10.0,
                purchased: 25.0,
                remaining: 32.5,
                cap: 50.0,
                burn_per_size: burn,
                currency: Some("USD".into()),
                period_end_at: Some(1_800_000_000),
            }),
            created_by_req_id: Mutex::new(BTreeMap::new()),
            next_id: AtomicUsize::new(1),
        })
    }
}

#[async_trait]
impl CloudProvisioningPort for FakeProvisioningPort {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        self.create_calls.fetch_add(1, Ordering::SeqCst);
        self.seen_create_request_ids
            .lock()
            .push(request.request_id.clone());
        if let Some(err) = self.create_outcomes.lock().pop_front() {
            err?;
        }
        let mut by_req = self.created_by_req_id.lock();
        if let Some(existing) = by_req.get(&request.request_id) {
            return Ok(existing.clone());
        }
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let machine = ProvisionedMachine {
            machine_id: format!("cm_prov_{seq}"),
            provider: "timeweb".into(),
            endpoint: SocketAddrV4::new(*self.create_ip.lock(), CLOUD_INGRESS_PORT),
            size: request.size.clone(),
            rate: self.rate.lock().clone(),
            sleep_mode: *self.sleep_mode.lock(),
            lease_id: format!("lease_{seq}"),
            lease_expires_at_ms: self
                .clock
                .now_ms()
                .saturating_add(self.lease_duration_ms.load(Ordering::SeqCst)),
        };
        by_req.insert(request.request_id.clone(), machine.clone());
        Ok(machine)
    }

    async fn wake(
        &self,
        machine_id: &str,
        _request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError> {
        self.wake_calls.fetch_add(1, Ordering::SeqCst);
        let gate = self.wake_gate.lock().clone();
        if let Some(notify) = gate {
            notify.notified().await;
        }
        if let Some(err) = self.wake_outcomes.lock().pop_front() {
            err?;
        }
        Ok(ProvisionedMachine {
            machine_id: machine_id.to_owned(),
            provider: "timeweb".into(),
            endpoint: SocketAddrV4::new(*self.wake_ip.lock(), CLOUD_INGRESS_PORT),
            size: "small".into(),
            rate: self.rate.lock().clone(),
            sleep_mode: *self.sleep_mode.lock(),
            lease_id: format!("lease_wake_{}", self.wake_calls.load(Ordering::SeqCst)),
            lease_expires_at_ms: self
                .clock
                .now_ms()
                .saturating_add(self.lease_duration_ms.load(Ordering::SeqCst)),
        })
    }

    async fn sleep(
        &self,
        _machine_id: &str,
        _request_id: &str,
    ) -> Result<CloudSleepMode, CloudError> {
        self.sleep_calls.fetch_add(1, Ordering::SeqCst);
        Ok(*self.sleep_mode.lock())
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
        self.renew_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(err) = self.renew_outcomes.lock().pop_front() {
            err?;
        }
        Ok(ProvisionedLease {
            machine_id: machine_id.to_owned(),
            lease_id: format!("{lease_id}-r{}", self.renew_calls.load(Ordering::SeqCst)),
            lease_expires_at_ms: self
                .clock
                .now_ms()
                .saturating_add(self.lease_duration_ms.load(Ordering::SeqCst)),
        })
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(self.usage.lock().clone())
    }
}

struct SecretDropProbe(Arc<AtomicBool>);

impl Drop for SecretDropProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct FakeEnrollment {
    outcome: Mutex<EnrollmentOutcome>,
    apply_calls: AtomicUsize,
    last_secret_dropped: Mutex<Option<Arc<AtomicBool>>>,
    saw_live_secret_during_enroll: AtomicBool,
}

impl FakeEnrollment {
    fn ok(installation_id: &str, peer_id: &str) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(EnrollmentOutcome::Enrolled {
                installation_id: installation_id.into(),
                peer_instance_id: Some(peer_id.into()),
            }),
            apply_calls: AtomicUsize::new(0),
            last_secret_dropped: Mutex::new(None),
            saw_live_secret_during_enroll: AtomicBool::new(false),
        })
    }

    fn unconfirmed(installation_id: &str) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(EnrollmentOutcome::ApplyUnconfirmed {
                installation_id: installation_id.into(),
            }),
            apply_calls: AtomicUsize::new(0),
            last_secret_dropped: Mutex::new(None),
            saw_live_secret_during_enroll: AtomicBool::new(false),
        })
    }

    fn secret_was_dropped(&self) -> bool {
        self.last_secret_dropped
            .lock()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
    }
}

#[async_trait]
impl CloudEnrollmentPort for FakeEnrollment {
    async fn prepare_binding(&self) -> Result<EnrollmentBinding, CloudError> {
        let dropped = Arc::new(AtomicBool::new(false));
        *self.last_secret_dropped.lock() = Some(dropped.clone());
        Ok(EnrollmentBinding {
            controller_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA0".into(),
            provider_creation_nonce: Zeroizing::new(
                "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB0".into(),
            ),
            opaque: Arc::new(SecretDropProbe(dropped)),
        })
    }

    async fn enroll(
        &self,
        _route: &CloudRouteSpec,
        binding: &EnrollmentBinding,
        _provider: &str,
        _size: &str,
    ) -> Result<EnrollmentOutcome, CloudError> {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(probe) = binding.opaque.downcast_ref::<SecretDropProbe>() {
            self.saw_live_secret_during_enroll
                .store(!probe.0.load(Ordering::SeqCst), Ordering::SeqCst);
        }
        Ok(self.outcome.lock().clone())
    }
}

struct FakePeerControl {
    busy: Mutex<PeerBusySnapshot>,
    drain_gate: Arc<LocalPeerDrainGate>,
    flush_outcomes: Mutex<VecDeque<FlushWriteOutcome>>,
    flushed_payloads: Mutex<Vec<(String, String, Option<SocketAddrV4>)>>,
    events: Mutex<Vec<String>>,
    endpoint_updates: Mutex<Vec<SocketAddrV4>>,
    published_endpoints: Mutex<Vec<(String, SocketAddrV4)>>,
}

impl FakePeerControl {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            busy: Mutex::new(PeerBusySnapshot::default()),
            drain_gate: Arc::new(LocalPeerDrainGate::default()),
            flush_outcomes: Mutex::new(VecDeque::new()),
            flushed_payloads: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            endpoint_updates: Mutex::new(Vec::new()),
            published_endpoints: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl CloudPeerControlPort for FakePeerControl {
    async fn read_idle(
        &self,
        _record: &CloudMachineRecord,
    ) -> Result<PeerBusySnapshot, CloudError> {
        Ok(*self.busy.lock())
    }

    async fn enter_drain(&self, record: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        self.drain_gate
            .try_enter_drain(&record.machine_id, *self.busy.lock(), 1000)
    }

    async fn reopen_drain(&self, _record: &CloudMachineRecord) -> Result<(), CloudError> {
        self.drain_gate.reopen();
        Ok(())
    }

    async fn update_endpoint(
        &self,
        _record: &CloudMachineRecord,
        endpoint: SocketAddrV4,
    ) -> Result<(), CloudError> {
        self.endpoint_updates.lock().push(endpoint);
        self.events.lock().push(format!("endpoint:{endpoint}"));
        Ok(())
    }

    async fn publish_direct_endpoint(
        &self,
        record: &CloudMachineRecord,
        endpoint: SocketAddrV4,
    ) -> Result<(), CloudError> {
        self.published_endpoints
            .lock()
            .push((record.installation_id.clone().unwrap_or_default(), endpoint));
        self.events.lock().push(format!("publish:{endpoint}"));
        Ok(())
    }

    async fn await_peer_ready(
        &self,
        _record: &CloudMachineRecord,
        _deadline_ms: u64,
    ) -> Result<(), CloudError> {
        self.drain_gate.reopen();
        self.events.lock().push("ready".into());
        Ok(())
    }

    async fn flush_send(
        &self,
        record: &CloudMachineRecord,
        send: &HeldCloudSend,
    ) -> FlushWriteOutcome {
        self.events.lock().push(format!(
            "flush:{}",
            send.body["text"].as_str().unwrap_or("")
        ));
        self.flushed_payloads.lock().push((
            send.send_id.clone(),
            send.body["text"].as_str().unwrap_or("").to_owned(),
            record.endpoint_addr,
        ));
        self.flush_outcomes
            .lock()
            .pop_front()
            .unwrap_or(FlushWriteOutcome::Written)
    }
}

fn temp_controller(clock: Arc<dyn Clock>) -> (tempfile::TempDir, Arc<CloudMachineController>) {
    let dir = tempfile::tempdir().unwrap();
    let store = CloudMachineStore::new(dir.path().join("cloud_machines.json"));
    let ctrl = Arc::new(CloudMachineController::new(
        store,
        "controller-1".into(),
        clock,
    ));
    (dir, ctrl)
}

#[tokio::test]
async fn create_reuse_backend_refusals_sleep_wake_and_uptime_accounting() {
    let clock = FakeClock::new(1_000_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::DeleteRecreate, clock.clone());
    let enrollment = FakeEnrollment::ok("inst-1", "peer-1");
    let peer_ctrl = FakePeerControl::new();

    // Auto placement refuses implicit creation when no machine exists.
    let err = ctrl
        .resolve_placement(
            &CloudTarget::Auto,
            None,
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "implicit_create_refused");

    // Backend typed refusals surface cleanly without leaving phantom local records.
    prov.create_outcomes
        .lock()
        .push_back(Err(CloudError::LeaseRequired));
    let lease_err = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("worker-a".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap_err();
    assert_eq!(lease_err.code(), "lease_required");
    assert!(enrollment.secret_was_dropped());
    assert!(ctrl.list().is_empty());

    prov.create_outcomes
        .lock()
        .push_back(Err(CloudError::CreditsExhausted));
    let credit_err = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("worker-a".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap_err();
    assert_eq!(credit_err.code(), "credits_exhausted");
    assert!(ctrl.list().is_empty());

    // Explicit creation creates and enrolls with managed cost fields, then drops creation secret.
    let receipt = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("worker-a".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert_eq!(receipt.kind, "cloud");
    assert_eq!(receipt.provider, "timeweb");
    assert_eq!(receipt.state, CloudLifecycleState::Awake);
    assert!(receipt.billing_notice.contains("costs money"));
    assert_eq!(
        receipt.rate_per_hour.as_ref().unwrap().credits_per_hour,
        Some(1.0)
    );
    assert!(enrollment
        .saw_live_secret_during_enroll
        .load(Ordering::SeqCst));
    assert!(enrollment.secret_was_dropped());

    // Backend cloud.machines limit surfaces as limit_reached without leaking existing names/ids.
    prov.create_outcomes
        .lock()
        .push_back(Err(CloudError::LimitReached {
            limit: 1,
            active_count: 1,
        }));
    let limit_err = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("worker-b".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap_err();
    assert_eq!(limit_err.code(), "limit_reached");
    assert!(!limit_err.to_string().contains("worker-a"));
    assert!(!limit_err.to_string().contains(&receipt.machine_id));

    // Auto placement reuses the awake machine without creating another.
    let reused = ctrl
        .resolve_placement(
            &CloudTarget::Auto,
            None,
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert_eq!(reused.machine_id, receipt.machine_id);
    assert_eq!(ctrl.list().len(), 1);

    // Advance 120 seconds awake, sleep, advance 600 seconds asleep -> uptime stays 120s.
    clock.advance_ms(120_000);
    let slept = ctrl
        .sleep(&receipt.machine_id, prov.as_ref(), peer_ctrl.as_ref(), true)
        .await
        .unwrap();
    assert_eq!(slept.state, CloudLifecycleState::Asleep);
    assert_eq!(slept.uptime_secs, 120);
    clock.advance_ms(600_000);
    assert_eq!(ctrl.list()[0].uptime_secs, 120);

    // Auto placement wakes the sleeping machine and never creates a second one.
    let woken = ctrl
        .resolve_placement(
            &CloudTarget::Auto,
            None,
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert_eq!(woken.machine_id, receipt.machine_id);
    assert_eq!(woken.state, CloudLifecycleState::Awake);
    assert_eq!(prov.wake_calls.load(Ordering::SeqCst), 1);

    // Usage delegates to provisioning port.
    let usage = ctrl.usage(prov.as_ref()).await.unwrap();
    assert_eq!(usage.allowance, 10.0);
    assert_eq!(usage.remaining, 32.5);

    // Delete requires explicit confirmation warning about unpushed work.
    let del_err = ctrl
        .delete(&receipt.machine_id, false, prov.as_ref())
        .await
        .unwrap_err();
    assert_eq!(del_err.code(), "delete_confirmation_required");
    assert!(del_err.to_string().contains("unpushed"));

    let deleted = ctrl
        .delete(&receipt.machine_id, true, prov.as_ref())
        .await
        .unwrap();
    assert_eq!(deleted.state, CloudLifecycleState::Deleted);
    assert_eq!(prov.delete_calls.load(Ordering::SeqCst), 1);
    assert!(ctrl.list().is_empty());
}

#[tokio::test]
async fn uncertain_create_reuses_persisted_request_id_on_retry_and_creates_once() {
    let clock = FakeClock::new(20_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::DeleteRecreate, clock.clone());
    let enrollment = FakeEnrollment::ok("inst-retry", "peer-retry");
    let peer_ctrl = FakePeerControl::new();

    prov.create_outcomes
        .lock()
        .push_back(Err(CloudError::ProviderUnconfirmed(
            "backend timeout after create".into(),
        )));

    let err = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("idempotent-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "provider_unconfirmed");
    assert!(enrollment.secret_was_dropped());

    // Pending request id is persisted on the Creating record before retry.
    let pending_records = ctrl.list_records();
    assert_eq!(pending_records.len(), 1);
    let persisted = pending_records[0]
        .pending_request
        .as_ref()
        .expect("persisted pending create request");
    assert_eq!(persisted.op, PendingProvisioningOp::Create);
    assert!(persisted.unconfirmed);
    let first_req_id = persisted.request_id.clone();

    // Retry reuses the EXACT persisted request id and yields one enrolled machine.
    let receipt = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("idempotent-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert_eq!(receipt.state, CloudLifecycleState::Awake);
    assert_eq!(ctrl.list().len(), 1);
    assert!(ctrl.list_records()[0].pending_request.is_none());

    let seen = prov.seen_create_request_ids.lock().clone();
    assert_eq!(seen, vec![first_req_id.clone(), first_req_id]);
    assert_eq!(prov.created_by_req_id.lock().len(), 1);
    assert!(enrollment.secret_was_dropped());
}

#[tokio::test]
async fn busy_guard_renews_credit_lease_by_deadline_and_refused_renewal_finishes_lease_then_sleeps()
{
    let clock = FakeClock::new(10_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::DeleteRecreate, clock.clone());
    let enrollment = FakeEnrollment::ok("inst-busy", "peer-busy");
    let peer_ctrl = FakePeerControl::new();

    // Created at t=10_000 with 600_000ms lease -> expires at t=610_000.
    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("busy-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    // Busy before lease renewal lead (120_000ms before expiry -> renew at 490_000): schedules deadline, zero polling.
    let busy_turn = PeerBusySnapshot {
        active_agent_turns: 1,
        running_background_jobs: 0,
        held_sends: 0,
        attached_viewers: 0,
    };
    *peer_ctrl.busy.lock() = busy_turn;
    let d0 = ctrl
        .evaluate_idle_tick(
            &created.machine_id,
            busy_turn,
            15 * 60_000,
            DEFAULT_LEASE_RENEW_LEAD_MS,
            true,
            prov.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert_eq!(
        d0,
        IdleDecision::ScheduleAt {
            at_ms: 490_000,
            idle_since_ms: None
        }
    );
    assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 0);

    // At renewal deadline while busy -> renews credit lease, never sleeps.
    clock.advance_ms(480_000); // now = 490_000
    let d1 = ctrl
        .evaluate_idle_tick(
            &created.machine_id,
            busy_turn,
            15 * 60_000,
            DEFAULT_LEASE_RENEW_LEAD_MS,
            true,
            prov.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert!(matches!(
        d1,
        IdleDecision::ScheduleAt { at_ms: 970_000, .. }
    ));
    assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 1);
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 0);

    // Direct sleep call while background job is running is refused by peer drain gate.
    *peer_ctrl.busy.lock() = PeerBusySnapshot {
        active_agent_turns: 0,
        running_background_jobs: 1,
        held_sends: 0,
        attached_viewers: 0,
    };
    let busy_err = ctrl
        .sleep(&created.machine_id, prov.as_ref(), peer_ctrl.as_ref(), true)
        .await
        .unwrap_err();
    assert_eq!(busy_err.code(), "machine_busy");
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 0);

    // Next renewal at t=970_000 is refused with credits_exhausted -> finishes current lease until 1_090_000, then sleeps.
    *peer_ctrl.busy.lock() = busy_turn;
    prov.renew_outcomes
        .lock()
        .push_back(Err(CloudError::CreditsExhausted));
    clock.advance_ms(480_000); // now = 970_000; lease expires at 1_090_000
    let d2 = ctrl
        .evaluate_idle_tick(
            &created.machine_id,
            busy_turn,
            15 * 60_000,
            DEFAULT_LEASE_RENEW_LEAD_MS,
            true,
            prov.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert_eq!(
        d2,
        IdleDecision::ScheduleAt {
            at_ms: 1_090_000,
            idle_since_ms: None
        }
    );
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ctrl.list()[0].state, CloudLifecycleState::Awake);

    // When current prepaid lease finishes at t=1_090_000 -> sleeps and transitions to OutOfCredits.
    clock.advance_ms(120_000); // now = 1_090_000
    let d3 = ctrl
        .evaluate_idle_tick(
            &created.machine_id,
            busy_turn,
            15 * 60_000,
            DEFAULT_LEASE_RENEW_LEAD_MS,
            true,
            prov.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert!(matches!(
        d3,
        IdleDecision::BeginTwoPhaseDrain {
            out_of_credits: true,
            ..
        }
    ));
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ctrl.list()[0].state, CloudLifecycleState::OutOfCredits);

    // Wake and auto placement are refused with credits_exhausted until topped up.
    let wake_err = ctrl
        .wake(
            &created.machine_id,
            prov.as_ref(),
            peer_ctrl.as_ref(),
            60_000,
        )
        .await
        .unwrap_err();
    assert_eq!(wake_err.code(), "credits_exhausted");
}

#[tokio::test]
async fn idle_machine_does_not_renew_lease_and_sleeps_after_idle_window() {
    let clock = FakeClock::new(10_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::Stop, clock.clone());
    let enrollment = FakeEnrollment::ok("inst-idle", "peer-idle");
    let peer_ctrl = FakePeerControl::new();

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("idle-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    // Idle past renewal lead does NOT renew lease; waits until idle window then sleeps.
    *peer_ctrl.busy.lock() = PeerBusySnapshot::default();
    let d1 = ctrl
        .evaluate_idle_tick(
            &created.machine_id,
            PeerBusySnapshot::default(),
            15 * 60_000,
            DEFAULT_LEASE_RENEW_LEAD_MS,
            true,
            prov.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert!(matches!(d1, IdleDecision::ScheduleAt { .. }));
    clock.advance_ms(15 * 60_000);
    let d2 = ctrl
        .evaluate_idle_tick(
            &created.machine_id,
            PeerBusySnapshot::default(),
            15 * 60_000,
            DEFAULT_LEASE_RENEW_LEAD_MS,
            true,
            prov.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert!(matches!(
        d2,
        IdleDecision::BeginTwoPhaseDrain {
            out_of_credits: false,
            ..
        }
    ));
    assert_eq!(prov.renew_calls.load(Ordering::SeqCst), 0);
    assert_eq!(prov.sleep_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ctrl.list()[0].state, CloudLifecycleState::Asleep);
}

#[tokio::test]
async fn delete_recreate_wake_updates_endpoint_before_releasing_held_sends_once_and_joins_wakes() {
    let clock = FakeClock::new(50_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::DeleteRecreate, clock.clone());
    let enrollment = FakeEnrollment::ok("inst-wake", "peer-wake");
    let peer_ctrl = FakePeerControl::new();

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("wake-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    let initial_addr = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 10), CLOUD_INGRESS_PORT);
    assert_eq!(created.endpoint_addr, Some(initial_addr));
    assert_eq!(
        peer_ctrl.published_endpoints.lock().as_slice(),
        &[("inst-wake".into(), initial_addr)]
    );
    peer_ctrl.events.lock().clear();
    peer_ctrl.endpoint_updates.lock().clear();
    ctrl.sleep(&created.machine_id, prov.as_ref(), peer_ctrl.as_ref(), true)
        .await
        .unwrap();

    // Hold two sends while asleep; duplicate delivery_id deduplicates to the first hold.
    let s1 = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "tab-1",
            "deliv-1".into(),
            "POST".into(),
            "/api/tabs/tab-1/send".into(),
            serde_json::json!({"text": "first-opaque"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();
    let s1_dup = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "tab-1",
            "deliv-1".into(),
            "POST".into(),
            "/api/tabs/tab-1/send".into(),
            serde_json::json!({"text": "first-opaque"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();
    assert_eq!(s1.send_id, s1_dup.send_id);

    let s2 = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "tab-1",
            "deliv-2".into(),
            "POST".into(),
            "/api/tabs/tab-1/send".into(),
            serde_json::json!({"text": "second-opaque"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();
    assert_eq!(
        ctrl.inspect_held_send(&s1.send_id).unwrap().state,
        ChatSendState::Pending
    );

    // Two concurrent wakes join one backend wake call.
    let gate = Arc::new(tokio::sync::Notify::new());
    *prov.wake_gate.lock() = Some(gate.clone());
    let c1 = ctrl.clone();
    let p1 = prov.clone();
    let pc1 = peer_ctrl.clone();
    let id1 = created.machine_id.clone();
    let t1 = tokio::spawn(async move { c1.wake(&id1, p1.as_ref(), pc1.as_ref(), 60_000).await });

    let c2 = ctrl.clone();
    let p2 = prov.clone();
    let pc2 = peer_ctrl.clone();
    let id2 = created.machine_id.clone();
    tokio::task::yield_now().await;
    let t2 = tokio::spawn(async move { c2.wake(&id2, p2.as_ref(), pc2.as_ref(), 60_000).await });
    tokio::task::yield_now().await;
    gate.notify_waiters();

    let woken1 = t1.await.unwrap().unwrap();
    assert!(t2.await.unwrap().is_ok());
    assert_eq!(prov.wake_calls.load(Ordering::SeqCst), 1);

    let new_addr = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 42), CLOUD_INGRESS_PORT);
    assert_eq!(woken1.endpoint_addr, Some(new_addr));
    assert_eq!(peer_ctrl.endpoint_updates.lock().as_slice(), &[new_addr]);

    // Endpoint update and direct endpoint publication strictly precede ready and flush.
    let events = peer_ctrl.events.lock().clone();
    assert_eq!(
        events,
        vec![
            format!("endpoint:{new_addr}"),
            format!("publish:{new_addr}"),
            "ready".into(),
            "flush:first-opaque".into(),
            "flush:second-opaque".into(),
        ]
    );
    let flushed = peer_ctrl.flushed_payloads.lock().clone();
    assert_eq!(
        flushed,
        vec![
            (s1.send_id.clone(), "first-opaque".into(), Some(new_addr)),
            (s2.send_id.clone(), "second-opaque".into(), Some(new_addr)),
        ]
    );
    assert_eq!(
        ctrl.inspect_held_send(&s1.send_id).unwrap().state,
        ChatSendState::Written
    );
    assert!(ctrl.cancel_held_send(&s1.send_id).is_err());
}

#[tokio::test]
async fn uncertain_flush_becomes_submit_unconfirmed_and_never_replays_and_drain_race_returns_held()
{
    let clock = FakeClock::new(1_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::Stop, clock.clone());
    let enrollment = FakeEnrollment::ok("inst-race", "peer-race");
    let peer_ctrl = FakePeerControl::new();

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("race-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    // First send hits uncertain transport -> submit_unconfirmed, never replayed on subsequent flush.
    let uncertain = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "tab-r",
            "deliv-u".into(),
            "POST".into(),
            "/api/tabs/tab-r/send".into(),
            serde_json::json!({"text": "uncertain-payload"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();
    peer_ctrl
        .flush_outcomes
        .lock()
        .push_back(FlushWriteOutcome::Uncertain(
            "connection reset after write".into(),
        ));
    ctrl.flush_held_sends(&created.machine_id, peer_ctrl.as_ref())
        .await;
    assert_eq!(
        ctrl.inspect_held_send(&uncertain.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    ctrl.flush_held_sends(&created.machine_id, peer_ctrl.as_ref())
        .await;
    assert_eq!(peer_ctrl.flushed_payloads.lock().len(), 1);

    // Second send races drain (bytes_written=false machine_sleeping) -> stays held and flushes once next wake.
    let racing = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "tab-r",
            "deliv-race".into(),
            "POST".into(),
            "/api/tabs/tab-r/send".into(),
            serde_json::json!({"text": "race-payload"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();
    peer_ctrl
        .flush_outcomes
        .lock()
        .push_back(FlushWriteOutcome::RefusedMachineSleeping);
    ctrl.flush_held_sends(&created.machine_id, peer_ctrl.as_ref())
        .await;
    assert_eq!(
        ctrl.inspect_held_send(&racing.send_id).unwrap().state,
        ChatSendState::Pending
    );

    peer_ctrl
        .flush_outcomes
        .lock()
        .push_back(FlushWriteOutcome::Written);
    ctrl.flush_held_sends(&created.machine_id, peer_ctrl.as_ref())
        .await;
    assert_eq!(
        ctrl.inspect_held_send(&racing.send_id).unwrap().state,
        ChatSendState::Written
    );
}

#[tokio::test]
async fn apply_unconfirmed_prevents_reapply_and_peer_drain_gate_refuses_writes_atomically() {
    let clock = FakeClock::new(1_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::Stop, clock.clone());
    let enrollment = FakeEnrollment::unconfirmed("inst-unconfirmed");
    let peer_ctrl = FakePeerControl::new();

    let err = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("unconfirmed-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "apply_unconfirmed");
    assert!(enrollment.secret_was_dropped());
    let rec = &ctrl.list_records()[0];
    assert!(rec.apply_unconfirmed);
    assert!(!can_issue_enrollment_apply(rec));

    // Startup reconciliation adopts label-matched machines once without duplication.
    let mut labels = BTreeMap::new();
    labels.insert(state::LABEL_MANAGED.into(), "true".into());
    labels.insert(state::LABEL_CONTROLLER.into(), "controller-1".into());
    labels.insert(state::LABEL_MACHINE_ID.into(), "cloud-adopted".into());
    labels.insert(state::LABEL_NAME.into(), "adopted-box".into());
    let observed = vec![ObservedProviderMachine {
        provider_machine_id: "cm_ext_99".into(),
        labels,
        state: ObservedProviderState::Stopped,
    }];
    let r1 = reconcile_provider_machines(
        &ctrl.list_records(),
        &observed,
        "controller-1",
        "timeweb",
        CloudSleepMode::DeleteRecreate,
        None,
        2_000,
    );
    assert_eq!(r1.adopted_ids, vec!["cloud-adopted"]);
    let r2 = reconcile_provider_machines(
        &r1.records,
        &observed,
        "controller-1",
        "timeweb",
        CloudSleepMode::DeleteRecreate,
        None,
        3_000,
    );
    assert!(r2.adopted_ids.is_empty());

    let gate = LocalPeerDrainGate::default();
    assert!(gate.check_write_allowed("cloud-1").is_ok());
    let token = gate
        .try_enter_drain("cloud-1", PeerBusySnapshot::default(), 5_000)
        .unwrap();
    assert!(!token.token.is_empty());
    let write_err = gate.check_write_allowed("cloud-1").unwrap_err();
    assert_eq!(write_err.code(), "machine_sleeping");
    assert_eq!(write_err.bytes_written(), Some(false));
    gate.reopen();
    assert!(gate.check_write_allowed("cloud-1").is_ok());
}
