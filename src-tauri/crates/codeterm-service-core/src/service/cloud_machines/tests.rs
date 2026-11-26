use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use zeroize::Zeroizing;

use super::*;
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

#[derive(Default)]
struct FakeProvider {
    sleep_mode: Mutex<CloudSleepMode>,
    rate: Mutex<Option<CloudRate>>,
    machines: Mutex<Vec<ObservedProviderMachine>>,
    start_calls: AtomicUsize,
    stop_calls: AtomicUsize,
    delete_calls: AtomicUsize,
    refresh_calls: AtomicUsize,
    start_gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
}

impl FakeProvider {
    fn new(sleep_mode: CloudSleepMode) -> Arc<Self> {
        Arc::new(Self {
            sleep_mode: Mutex::new(sleep_mode),
            rate: Mutex::new(Some(CloudRate {
                amount: 0.08,
                currency: "USD".into(),
                basis: CloudRateBasis::Estimate,
            })),
            ..Default::default()
        })
    }
}

#[async_trait]
impl CloudBackendPort for FakeProvider {
    async fn prepare_image(&self, _size: &str) -> Result<String, CloudError> {
        Ok("img-ready-1".into())
    }

    async fn create(
        &self,
        request: &CloudCreateRequest,
    ) -> Result<ObservedProviderMachine, CloudError> {
        let mut machines = self.machines.lock();
        let id = format!("prov-{}", machines.len() + 1);
        let machine = ObservedProviderMachine {
            provider_machine_id: id,
            labels: request.labels.clone(),
            state: ObservedProviderState::Running,
        };
        machines.push(machine.clone());
        Ok(machine)
    }

    async fn start(&self, provider_machine_id: &str, _auto_stop: u32) -> Result<(), CloudError> {
        self.start_calls.fetch_add(1, Ordering::SeqCst);
        let gate = self.start_gate.lock().clone();
        if let Some(notify) = gate {
            notify.notified().await;
        }
        let mut machines = self.machines.lock();
        if let Some(m) = machines
            .iter_mut()
            .find(|m| m.provider_machine_id == provider_machine_id)
        {
            m.state = ObservedProviderState::Running;
        }
        Ok(())
    }

    async fn stop(&self, provider_machine_id: &str) -> Result<(), CloudError> {
        self.stop_calls.fetch_add(1, Ordering::SeqCst);
        let mut machines = self.machines.lock();
        if let Some(m) = machines
            .iter_mut()
            .find(|m| m.provider_machine_id == provider_machine_id)
        {
            m.state = ObservedProviderState::Stopped;
        }
        Ok(())
    }

    async fn delete(&self, ownership: &CloudOwnershipReceipt) -> Result<(), CloudError> {
        self.delete_calls.fetch_add(1, Ordering::SeqCst);
        self.machines
            .lock()
            .retain(|m| m.provider_machine_id != ownership.provider_machine_id);
        Ok(())
    }

    async fn list(
        &self,
        _labels: &BTreeMap<String, String>,
    ) -> Result<Vec<ObservedProviderMachine>, CloudError> {
        Ok(self.machines.lock().clone())
    }

    async fn access(
        &self,
        _provider_machine_id: &str,
        port: u16,
    ) -> Result<CloudRouteSpec, CloudError> {
        Ok(CloudRouteSpec::Direct {
            address: SocketAddrV4::new(Ipv4Addr::new(203, 0, 114, 10), port),
        })
    }

    async fn refresh_activity(
        &self,
        _provider_machine_id: &str,
        _auto_stop: u32,
    ) -> Result<(), CloudError> {
        self.refresh_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn hourly_rate(&self, _size: &str) -> Result<Option<CloudRate>, CloudError> {
        Ok(self.rate.lock().clone())
    }

    async fn sleep_mode(&self) -> Result<CloudSleepMode, CloudError> {
        Ok(*self.sleep_mode.lock())
    }
}

struct FakeEnrollment {
    outcome: Mutex<EnrollmentOutcome>,
    apply_calls: AtomicUsize,
}

impl FakeEnrollment {
    fn ok(installation_id: &str, peer_id: &str) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(EnrollmentOutcome::Enrolled {
                installation_id: installation_id.into(),
                peer_instance_id: Some(peer_id.into()),
            }),
            apply_calls: AtomicUsize::new(0),
        })
    }

    fn unconfirmed(installation_id: &str) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(EnrollmentOutcome::ApplyUnconfirmed {
                installation_id: installation_id.into(),
            }),
            apply_calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl CloudEnrollmentPort for FakeEnrollment {
    async fn prepare_binding(&self) -> Result<EnrollmentBinding, CloudError> {
        Ok(EnrollmentBinding {
            controller_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA0".into(),
            provider_creation_nonce: Zeroizing::new(
                "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB0".into(),
            ),
            opaque: Arc::new(()),
        })
    }

    async fn enroll(
        &self,
        _route: &CloudRouteSpec,
        _binding: &EnrollmentBinding,
        _provider: &str,
        _size: &str,
    ) -> Result<EnrollmentOutcome, CloudError> {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.outcome.lock().clone())
    }
}

struct FakePeerControl {
    busy: Mutex<PeerBusySnapshot>,
    drain_gate: Arc<LocalPeerDrainGate>,
    flush_outcomes: Mutex<VecDeque<FlushWriteOutcome>>,
    flushed_payloads: Mutex<Vec<(String, String)>>,
}

impl FakePeerControl {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            busy: Mutex::new(PeerBusySnapshot::default()),
            drain_gate: Arc::new(LocalPeerDrainGate::default()),
            flush_outcomes: Mutex::new(VecDeque::new()),
            flushed_payloads: Mutex::new(Vec::new()),
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

    async fn await_peer_ready(
        &self,
        _record: &CloudMachineRecord,
        _deadline_ms: u64,
    ) -> Result<(), CloudError> {
        self.drain_gate.reopen();
        Ok(())
    }

    async fn flush_send(
        &self,
        _record: &CloudMachineRecord,
        send: &HeldCloudSend,
    ) -> FlushWriteOutcome {
        self.flushed_payloads.lock().push((
            send.send_id.clone(),
            send.body["text"].as_str().unwrap_or("").to_owned(),
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
async fn create_reuse_limit_sleep_wake_and_uptime_accounting() {
    let clock = FakeClock::new(1_000_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let provider = FakeProvider::new(CloudSleepMode::Stop);
    let enrollment = FakeEnrollment::ok("inst-1", "peer-1");
    let peer_ctrl = FakePeerControl::new();

    // Auto placement refuses implicit creation when no machine exists.
    let err = ctrl
        .resolve_placement(
            &CloudTarget::Auto,
            None,
            provider.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
            CloudLimitPolicy::byo(1),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "implicit_create_refused");

    // Explicit creation creates and enrolls with cost fields.
    let receipt = ctrl
        .create(
            CreateCloudMachineParams {
                provider: "fake".into(),
                size: "small".into(),
                name: Some("worker-a".into()),
                auto_stop_minutes: 5,
                extra_labels: BTreeMap::new(),
            },
            provider.as_ref(),
            enrollment.as_ref(),
            CloudLimitPolicy::byo(1),
        )
        .await
        .unwrap();
    assert_eq!(receipt.kind, "cloud");
    assert_eq!(receipt.state, CloudLifecycleState::Awake);
    assert!(receipt.billing_notice.contains("costs money"));
    assert_eq!(
        receipt.rate_per_hour.as_ref().unwrap().basis,
        CloudRateBasis::Estimate
    );

    // Limit reached refuses second creation without leaking existing names/ids.
    let limit_err = ctrl
        .create(
            CreateCloudMachineParams {
                provider: "fake".into(),
                size: "small".into(),
                name: Some("worker-b".into()),
                auto_stop_minutes: 5,
                extra_labels: BTreeMap::new(),
            },
            provider.as_ref(),
            enrollment.as_ref(),
            CloudLimitPolicy::byo(1),
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
            provider.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
            CloudLimitPolicy::byo(1),
        )
        .await
        .unwrap();
    assert_eq!(reused.machine_id, receipt.machine_id);
    assert_eq!(ctrl.list().len(), 1);

    // Advance 120 seconds awake, sleep, advance 600 seconds asleep -> uptime stays 120s.
    clock.advance_ms(120_000);
    let slept = ctrl
        .sleep(
            &receipt.machine_id,
            provider.as_ref(),
            peer_ctrl.as_ref(),
            false,
        )
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
            provider.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
            CloudLimitPolicy::byo(1),
        )
        .await
        .unwrap();
    assert_eq!(woken.machine_id, receipt.machine_id);
    assert_eq!(woken.state, CloudLifecycleState::Awake);
    assert_eq!(provider.start_calls.load(Ordering::SeqCst), 1);

    // Delete requires explicit confirmation warning about unpushed work.
    let del_err = ctrl
        .delete(&receipt.machine_id, false, provider.as_ref())
        .await
        .unwrap_err();
    assert_eq!(del_err.code(), "delete_confirmation_required");
    assert!(del_err.to_string().contains("unpushed"));

    let deleted = ctrl
        .delete(&receipt.machine_id, true, provider.as_ref())
        .await
        .unwrap();
    assert_eq!(deleted.state, CloudLifecycleState::Deleted);
    assert_eq!(provider.delete_calls.load(Ordering::SeqCst), 1);
    assert!(ctrl.list().is_empty());
}

#[tokio::test]
async fn busy_guard_blocks_sleep_and_renews_provider_activity() {
    let clock = FakeClock::new(10_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let provider = FakeProvider::new(CloudSleepMode::Stop);
    let enrollment = FakeEnrollment::ok("inst-busy", "peer-busy");
    let peer_ctrl = FakePeerControl::new();

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                provider: "fake".into(),
                size: "small".into(),
                name: Some("busy-box".into()),
                auto_stop_minutes: 5,
                extra_labels: BTreeMap::new(),
            },
            provider.as_ref(),
            enrollment.as_ref(),
            CloudLimitPolicy::byo(3),
        )
        .await
        .unwrap();

    // Busy with a running agent turn past auto-stop renewal boundary -> renews, never sleeps.
    let busy_turn = PeerBusySnapshot {
        active_agent_turns: 1,
        running_background_jobs: 0,
        held_sends: 0,
        attached_viewers: 0,
    };
    *peer_ctrl.busy.lock() = busy_turn;
    clock.advance_ms(240_000);
    let decision = ctrl
        .evaluate_idle_tick(
            &created.machine_id,
            busy_turn,
            15 * 60_000,
            false,
            provider.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert!(matches!(decision, IdleDecision::RenewActivityNow { .. }));
    assert_eq!(provider.refresh_calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.stop_calls.load(Ordering::SeqCst), 0);

    // Direct sleep call while background job is running is refused by peer drain gate.
    *peer_ctrl.busy.lock() = PeerBusySnapshot {
        active_agent_turns: 0,
        running_background_jobs: 1,
        held_sends: 0,
        attached_viewers: 0,
    };
    let busy_err = ctrl
        .sleep(
            &created.machine_id,
            provider.as_ref(),
            peer_ctrl.as_ref(),
            false,
        )
        .await
        .unwrap_err();
    assert_eq!(busy_err.code(), "machine_busy");
    assert_eq!(provider.stop_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ctrl.list()[0].state, CloudLifecycleState::Awake);

    // Idle for 15 minutes -> enters two-phase drain and stops provider.
    *peer_ctrl.busy.lock() = PeerBusySnapshot::default();
    let d1 = ctrl
        .evaluate_idle_tick(
            &created.machine_id,
            PeerBusySnapshot::default(),
            15 * 60_000,
            false,
            provider.as_ref(),
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
            false,
            provider.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert!(matches!(d2, IdleDecision::BeginTwoPhaseDrain { .. }));
    assert_eq!(provider.stop_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ctrl.list()[0].state, CloudLifecycleState::Asleep);
}

#[tokio::test]
async fn stop_billed_sleep_mode_is_modeled_as_data() {
    let clock = FakeClock::new(10_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let provider = FakeProvider::new(CloudSleepMode::StopBilled);
    let enrollment = FakeEnrollment::ok("inst-tw", "peer-tw");
    let peer_ctrl = FakePeerControl::new();

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                provider: "timeweb".into(),
                size: "small".into(),
                name: Some("tw-box".into()),
                auto_stop_minutes: 0,
                extra_labels: BTreeMap::new(),
            },
            provider.as_ref(),
            enrollment.as_ref(),
            CloudLimitPolicy::byo(3),
        )
        .await
        .unwrap();

    // Under keep-running lease policy, StopBilled does not call provider.stop().
    let kept = ctrl
        .sleep(
            &created.machine_id,
            provider.as_ref(),
            peer_ctrl.as_ref(),
            false,
        )
        .await
        .unwrap();
    assert_eq!(kept.state, CloudLifecycleState::Awake);
    assert_eq!(provider.stop_calls.load(Ordering::SeqCst), 0);

    // Under DeleteRecreate policy, StopBilled deletes to stop billing.
    let deleted_for_recreate = ctrl
        .sleep(
            &created.machine_id,
            provider.as_ref(),
            peer_ctrl.as_ref(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(deleted_for_recreate.state, CloudLifecycleState::Asleep);
    assert_eq!(provider.delete_calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.stop_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn wake_joins_concurrent_callers_and_flushes_held_sends_exactly_once_in_order() {
    let clock = FakeClock::new(50_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let provider = FakeProvider::new(CloudSleepMode::Stop);
    let enrollment = FakeEnrollment::ok("inst-wake", "peer-wake");
    let peer_ctrl = FakePeerControl::new();

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                provider: "fake".into(),
                size: "small".into(),
                name: Some("wake-box".into()),
                auto_stop_minutes: 10,
                extra_labels: BTreeMap::new(),
            },
            provider.as_ref(),
            enrollment.as_ref(),
            CloudLimitPolicy::byo(3),
        )
        .await
        .unwrap();
    ctrl.sleep(
        &created.machine_id,
        provider.as_ref(),
        peer_ctrl.as_ref(),
        false,
    )
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
            60_000,
        )
        .unwrap();
    assert_eq!(
        ctrl.inspect_held_send(&s1.send_id).unwrap().state,
        ChatSendState::Pending
    );

    // Two concurrent wakes join one provider.start() call.
    let gate = Arc::new(tokio::sync::Notify::new());
    *provider.start_gate.lock() = Some(gate.clone());
    let c1 = ctrl.clone();
    let p1 = provider.clone();
    let pc1 = peer_ctrl.clone();
    let id1 = created.machine_id.clone();
    let t1 = tokio::spawn(async move { c1.wake(&id1, p1.as_ref(), pc1.as_ref(), 60_000).await });

    let c2 = ctrl.clone();
    let p2 = provider.clone();
    let pc2 = peer_ctrl.clone();
    let id2 = created.machine_id.clone();
    tokio::task::yield_now().await;
    let t2 = tokio::spawn(async move { c2.wake(&id2, p2.as_ref(), pc2.as_ref(), 60_000).await });
    tokio::task::yield_now().await;
    gate.notify_waiters();

    assert!(t1.await.unwrap().is_ok());
    assert!(t2.await.unwrap().is_ok());
    assert_eq!(provider.start_calls.load(Ordering::SeqCst), 1);

    // Flushed once each in admission order.
    let flushed = peer_ctrl.flushed_payloads.lock().clone();
    assert_eq!(
        flushed,
        vec![
            (s1.send_id.clone(), "first-opaque".into()),
            (s2.send_id.clone(), "second-opaque".into()),
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
    let provider = FakeProvider::new(CloudSleepMode::Stop);
    let enrollment = FakeEnrollment::ok("inst-race", "peer-race");
    let peer_ctrl = FakePeerControl::new();

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                provider: "fake".into(),
                size: "small".into(),
                name: Some("race-box".into()),
                auto_stop_minutes: 10,
                extra_labels: BTreeMap::new(),
            },
            provider.as_ref(),
            enrollment.as_ref(),
            CloudLimitPolicy::byo(3),
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
async fn apply_unconfirmed_prevents_reapply_and_reconcile_adopts_by_label_without_duplication() {
    let clock = FakeClock::new(1_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let provider = FakeProvider::new(CloudSleepMode::Stop);
    let enrollment = FakeEnrollment::unconfirmed("inst-unconfirmed");

    let err = ctrl
        .create(
            CreateCloudMachineParams {
                provider: "fake".into(),
                size: "small".into(),
                name: Some("unconfirmed-box".into()),
                auto_stop_minutes: 10,
                extra_labels: BTreeMap::new(),
            },
            provider.as_ref(),
            enrollment.as_ref(),
            CloudLimitPolicy::byo(3),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "apply_unconfirmed");
    let rec = &ctrl.list_records()[0];
    assert!(rec.apply_unconfirmed);
    assert!(!can_issue_enrollment_apply(rec));

    // Startup reconciliation adopts foreign-untracked machine with our controller label once, never duplicating.
    let mut labels = BTreeMap::new();
    labels.insert(state::LABEL_MANAGED.into(), "true".into());
    labels.insert(state::LABEL_CONTROLLER.into(), "controller-1".into());
    labels.insert(state::LABEL_MACHINE_ID.into(), "cloud-adopted".into());
    labels.insert(state::LABEL_NAME.into(), "adopted-box".into());
    provider.machines.lock().push(ObservedProviderMachine {
        provider_machine_id: "prov-ext-99".into(),
        labels,
        state: ObservedProviderState::Stopped,
    });

    let r1 = ctrl.reconcile("fake", provider.as_ref()).await.unwrap();
    assert_eq!(r1.adopted_ids, vec!["cloud-adopted"]);
    let r2 = ctrl.reconcile("fake", provider.as_ref()).await.unwrap();
    assert!(r2.adopted_ids.is_empty());
    assert_eq!(ctrl.list().len(), 2);
}

#[test]
fn free_plan_refuses_managed_cloud_and_peer_drain_gate_refuses_writes_atomically() {
    let err = check_machine_limit(&[], CloudLimitPolicy::managed(3, false)).unwrap_err();
    assert_eq!(err.code(), "free_plan_managed_refused");

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
