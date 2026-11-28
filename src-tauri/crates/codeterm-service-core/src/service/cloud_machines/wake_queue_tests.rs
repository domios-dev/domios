use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use zeroize::Zeroizing;

use super::*;
use crate::mesh::crown::{AuthenticatedAgent, AuthenticatedOrigin};
use crate::service::chat_send_transaction::ChatSendState;

struct StepClock(AtomicU64);

impl StepClock {
    fn new(start_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start_ms)))
    }

    fn advance(&self, delta_ms: u64) {
        self.0.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

impl Clock for StepClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct StubProvisioning {
    create_addr: CloudEndpoint,
    wake_addr: CloudEndpoint,
}

#[async_trait]
impl CloudProvisioningPort for StubProvisioning {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        Ok(ProvisionedMachine {
            ingress_ready_at: None,
            machine_id: "cm_stub_1".into(),
            provider: "timeweb".into(),
            endpoint: self.create_addr.clone(),
            size: request.size.clone(),
            rate: CloudRate {
                amount: 0.20,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: CloudSleepMode::Stop,
            lease_id: "lease-1".into(),
            lease_expires_at_ms: 900_000,
        })
    }

    async fn wake(
        &self,
        machine_id: &str,
        _request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError> {
        Ok(ProvisionedMachine {
            ingress_ready_at: None,
            machine_id: machine_id.to_owned(),
            provider: "timeweb".into(),
            endpoint: self.wake_addr.clone(),
            size: "small".into(),
            rate: CloudRate {
                amount: 0.20,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: CloudSleepMode::Stop,
            lease_id: "lease-2".into(),
            lease_expires_at_ms: 900_000,
        })
    }

    async fn sleep(
        &self,
        _machine_id: &str,
        _request_id: &str,
    ) -> Result<CloudSleepMode, CloudError> {
        Ok(CloudSleepMode::Stop)
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
        Ok(ProvisionedLease {
            machine_id: machine_id.to_owned(),
            lease_id: lease_id.to_owned(),
            lease_expires_at_ms: 900_000,
        })
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(CloudUsageSummary {
            used: 0.0,
            allowance: 10.0,
            purchased: 0.0,
            remaining: 10.0,
            cap: 50.0,
            burn_per_size: BTreeMap::new(),
            currency: Some("USD".into()),
            period_end_at: None,
            ..Default::default()
        })
    }
}

struct StubEnrollment;

#[async_trait]
impl CloudEnrollmentPort for StubEnrollment {
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
        Ok(EnrollmentOutcome::Enrolled {
            installation_id: "inst-q".into(),
            peer_instance_id: Some("peer-q".into()),
        })
    }
}

struct InstrumentedPeerControl {
    publish_outcomes: Mutex<VecDeque<Result<(), CloudError>>>,
    flush_outcomes: Mutex<VecDeque<FlushWriteOutcome>>,
    published: Mutex<Vec<(String, CloudEndpoint)>>,
    events: Mutex<Vec<String>>,
    flushed_ids: Mutex<Vec<String>>,
    link_generation: Mutex<Option<String>>,
    asleep_notes: AtomicUsize,
    flush_in_flight: AtomicUsize,
    max_flush_in_flight: AtomicUsize,
    first_flush_entered: Arc<tokio::sync::Notify>,
    first_flush_release: Arc<tokio::sync::Notify>,
    gate_first_flush: Mutex<bool>,
}

impl InstrumentedPeerControl {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            publish_outcomes: Mutex::new(VecDeque::new()),
            flush_outcomes: Mutex::new(VecDeque::new()),
            published: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            flushed_ids: Mutex::new(Vec::new()),
            link_generation: Mutex::new(None),
            asleep_notes: AtomicUsize::new(0),
            flush_in_flight: AtomicUsize::new(0),
            max_flush_in_flight: AtomicUsize::new(0),
            first_flush_entered: Arc::new(tokio::sync::Notify::new()),
            first_flush_release: Arc::new(tokio::sync::Notify::new()),
            gate_first_flush: Mutex::new(false),
        })
    }
}

#[async_trait]
impl CloudPeerControlPort for InstrumentedPeerControl {
    async fn read_idle(
        &self,
        _record: &CloudMachineRecord,
    ) -> Result<PeerBusySnapshot, CloudError> {
        Ok(PeerBusySnapshot::default())
    }

    async fn enter_drain(&self, _record: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        Ok(DrainToken {
            token: "drain-1".into(),
            entered_at_ms: 1_000,
        })
    }

    async fn reopen_drain(&self, _record: &CloudMachineRecord) -> Result<(), CloudError> {
        Ok(())
    }

    async fn update_endpoint(
        &self,
        _record: &CloudMachineRecord,
        endpoint: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        self.events.lock().push(format!("update:{endpoint}"));
        Ok(())
    }

    async fn publish_direct_endpoint(
        &self,
        record: &CloudMachineRecord,
        endpoint: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        self.events.lock().push(format!("publish:{endpoint}"));
        if let Some(outcome) = self.publish_outcomes.lock().pop_front() {
            outcome?;
        }
        self.published.lock().push((
            record.installation_id.clone().unwrap_or_default(),
            endpoint.clone(),
        ));
        Ok(())
    }

    async fn await_peer_ready(
        &self,
        _record: &CloudMachineRecord,
        _deadline_ms: u64,
    ) -> Result<(), CloudError> {
        self.events.lock().push("ready".into());
        Ok(())
    }

    async fn flush_send(
        &self,
        _record: &CloudMachineRecord,
        send: &HeldCloudSend,
    ) -> FlushWriteOutcome {
        let active = self.flush_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_flush_in_flight.fetch_max(active, Ordering::SeqCst);
        let should_gate = {
            let mut gate = self.gate_first_flush.lock();
            if *gate {
                *gate = false;
                true
            } else {
                false
            }
        };
        if should_gate {
            self.first_flush_entered.notify_waiters();
            self.first_flush_release.notified().await;
        }
        self.flushed_ids.lock().push(send.send_id.clone());
        self.events.lock().push(format!("flush:{}", send.send_id));
        self.flush_in_flight.fetch_sub(1, Ordering::SeqCst);
        self.flush_outcomes
            .lock()
            .pop_front()
            .unwrap_or(FlushWriteOutcome::Delivered)
    }

    fn current_link_generation(&self, _record: &CloudMachineRecord) -> Option<String> {
        self.link_generation.lock().clone()
    }

    fn note_machine_asleep(&self, _record: &CloudMachineRecord) {
        self.asleep_notes.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn persisted_claimed_send_becomes_submit_unconfirmed_on_restart_and_never_replays() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    let clock = StepClock::new(10_000);
    let prov = StubProvisioning {
        create_addr: CloudEndpoint::Direct(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 10),
            CLOUD_INGRESS_PORT,
        )),
        wake_addr: CloudEndpoint::Direct(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 20),
            CLOUD_INGRESS_PORT,
        )),
    };
    let peer_ctrl = InstrumentedPeerControl::new();

    let ctrl1 = CloudMachineController::new(
        CloudMachineStore::new(store_path.clone()),
        "controller-1".into(),
        clock.clone(),
    );
    let created = ctrl1
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("persist-box".into()),
                ..Default::default()
            },
            &prov,
            &StubEnrollment,
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    // Hold two sends: s1 will be persisted as Claimed (simulating crash mid-flush), s2 stays Held.
    let agent = AuthenticatedAgent {
        instance_id: "controller-1".into(),
        tab_id: "tab-agent".into(),
        can_spawn: false,
    };
    let s1 = ctrl1
        .hold_send_for_wake(
            &created.machine_id,
            "remote-tab",
            "deliv-claimed".into(),
            "POST".into(),
            "/api/tabs/remote-tab/send".into(),
            serde_json::json!({"text": "crash-mid-flush"}),
            AuthenticatedOrigin::Agent,
            Some(agent.clone()),
            60_000,
        )
        .unwrap();
    let s2 = ctrl1
        .hold_send_for_wake(
            &created.machine_id,
            "remote-tab",
            "deliv-held".into(),
            "POST".into(),
            "/api/tabs/remote-tab/send".into(),
            serde_json::json!({"text": "still-held"}),
            AuthenticatedOrigin::Agent,
            Some(agent),
            60_000,
        )
        .unwrap();

    // Simulate process crash after s1 was persisted as Claimed right before peer write settled.
    let queue_store = CloudMachineStore::new(store_path.clone()).wake_queue_store();
    let (seq, mut persisted) = queue_store.load();
    let s1_row = persisted
        .iter_mut()
        .find(|row| row.send_id == s1.send_id)
        .expect("s1 persisted");
    s1_row.state = HeldCloudSendState::Claimed;
    queue_store.save(seq, &persisted).unwrap();

    // Restart controller from disk: Claimed must promote to SubmitUnconfirmed, Held stays Held.
    let ctrl2 = CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "controller-1".into(),
        clock,
    );
    assert_eq!(
        ctrl2.inspect_held_send(&s1.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    assert_eq!(
        ctrl2.inspect_held_send(&s2.send_id).unwrap().state,
        ChatSendState::Pending
    );

    // Flushing after restart delivers ONLY s2 and NEVER replays s1.
    ctrl2
        .flush_held_sends(&created.machine_id, peer_ctrl.as_ref())
        .await;
    assert_eq!(
        peer_ctrl.flushed_ids.lock().as_slice(),
        &[s2.send_id.clone()]
    );
    assert_eq!(
        ctrl2.inspect_held_send(&s1.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    assert_eq!(
        ctrl2.inspect_held_send(&s2.send_id).unwrap().state,
        ChatSendState::Sent
    );
}

#[tokio::test]
async fn flush_held_sends_serializes_concurrent_flushes_per_machine() {
    let dir = tempfile::tempdir().unwrap();
    let clock = StepClock::new(20_000);
    let prov = StubProvisioning {
        create_addr: CloudEndpoint::Direct(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 10),
            CLOUD_INGRESS_PORT,
        )),
        wake_addr: CloudEndpoint::Direct(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 20),
            CLOUD_INGRESS_PORT,
        )),
    };
    let peer_ctrl = InstrumentedPeerControl::new();
    let ctrl = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "controller-1".into(),
        clock,
    ));
    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("mutex-box".into()),
                ..Default::default()
            },
            &prov,
            &StubEnrollment,
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    let s1 = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "remote-tab",
            "deliv-m1".into(),
            "POST".into(),
            "/api/tabs/remote-tab/send".into(),
            serde_json::json!({"text": "one"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();
    let s2 = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "remote-tab",
            "deliv-m2".into(),
            "POST".into(),
            "/api/tabs/remote-tab/send".into(),
            serde_json::json!({"text": "two"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();

    *peer_ctrl.gate_first_flush.lock() = true;
    let c1 = ctrl.clone();
    let p1 = peer_ctrl.clone();
    let m1 = created.machine_id.clone();
    let f1 = tokio::spawn(async move { c1.flush_held_sends(&m1, p1.as_ref()).await });
    peer_ctrl.first_flush_entered.notified().await;

    let c2 = ctrl.clone();
    let p2 = peer_ctrl.clone();
    let m2 = created.machine_id.clone();
    let f2 = tokio::spawn(async move { c2.flush_held_sends(&m2, p2.as_ref()).await });
    tokio::task::yield_now().await;
    assert_eq!(peer_ctrl.max_flush_in_flight.load(Ordering::SeqCst), 1);

    peer_ctrl.first_flush_release.notify_waiters();
    let _ = f1.await.unwrap();
    let _ = f2.await.unwrap();

    assert_eq!(peer_ctrl.max_flush_in_flight.load(Ordering::SeqCst), 1);
    assert_eq!(
        peer_ctrl.flushed_ids.lock().as_slice(),
        &[s1.send_id, s2.send_id]
    );
}

#[tokio::test]
async fn publish_direct_endpoint_failure_blocks_wake_readiness_and_releases_no_held_sends() {
    let dir = tempfile::tempdir().unwrap();
    let clock = StepClock::new(30_000);
    let create_addr = CloudEndpoint::Direct(SocketAddrV4::new(
        Ipv4Addr::new(198, 51, 100, 10),
        CLOUD_INGRESS_PORT,
    ));
    let wake_addr = CloudEndpoint::Direct(SocketAddrV4::new(
        Ipv4Addr::new(198, 51, 100, 99),
        CLOUD_INGRESS_PORT,
    ));
    let prov = StubProvisioning {
        create_addr: create_addr.clone(),
        wake_addr: wake_addr.clone(),
    };
    let peer_ctrl = InstrumentedPeerControl::new();
    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "controller-1".into(),
        clock,
    );

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("publish-box".into()),
                ..Default::default()
            },
            &prov,
            &StubEnrollment,
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert_eq!(
        peer_ctrl.published.lock().as_slice(),
        &[("inst-q".into(), create_addr)]
    );

    ctrl.sleep(&created.machine_id, &prov, peer_ctrl.as_ref(), true)
        .await
        .unwrap();

    let held = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "remote-tab",
            "deliv-pub-fail".into(),
            "POST".into(),
            "/api/tabs/remote-tab/send".into(),
            serde_json::json!({"text": "must-stay-held"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();

    peer_ctrl.events.lock().clear();
    peer_ctrl
        .publish_outcomes
        .lock()
        .push_back(Err(CloudError::ProviderUnavailable(
            "endpoint publication refused".into(),
        )));

    let err = ctrl
        .wake(&created.machine_id, &prov, peer_ctrl.as_ref(), 60_000)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "provider_unavailable");
    assert!(
        peer_ctrl.flushed_ids.lock().is_empty(),
        "failed publish_direct_endpoint must release zero held sends"
    );
    assert_eq!(
        ctrl.inspect_held_send(&held.send_id).unwrap().state,
        ChatSendState::Pending
    );
    assert_eq!(
        peer_ctrl.events.lock().as_slice(),
        &[
            format!("update:{wake_addr}"),
            format!("publish:{wake_addr}"),
        ]
    );
    assert_eq!(
        ctrl.find_by_peer(&created.machine_id).unwrap().state,
        CloudLifecycleState::Asleep
    );

    // Subsequent wake with successful publish_direct_endpoint publishes before ready & flush.
    peer_ctrl.events.lock().clear();
    ctrl.wake(&created.machine_id, &prov, peer_ctrl.as_ref(), 60_000)
        .await
        .unwrap();
    assert_eq!(
        peer_ctrl.events.lock().as_slice(),
        &[
            format!("update:{wake_addr}"),
            format!("publish:{wake_addr}"),
            "ready".into(),
            format!("flush:{}", held.send_id),
        ]
    );
    assert_eq!(
        ctrl.inspect_held_send(&held.send_id).unwrap().state,
        ChatSendState::Sent
    );
}

struct ScriptedWakeProvisioning {
    endpoint: CloudEndpoint,
    clock: Arc<StepClock>,
    advance_per_wake_ms: u64,
    hang_wake: Mutex<bool>,
    wake_outcomes: Mutex<VecDeque<Result<(), CloudError>>>,
    seen_wake_request_ids: Mutex<Vec<String>>,
}

impl ScriptedWakeProvisioning {
    fn new(endpoint: CloudEndpoint, clock: Arc<StepClock>, advance_per_wake_ms: u64) -> Self {
        Self {
            endpoint,
            clock,
            advance_per_wake_ms,
            hang_wake: Mutex::new(false),
            wake_outcomes: Mutex::new(VecDeque::new()),
            seen_wake_request_ids: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl CloudProvisioningPort for ScriptedWakeProvisioning {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        Ok(ProvisionedMachine {
            ingress_ready_at: None,
            machine_id: "cm_retry_1".into(),
            provider: "fly".into(),
            endpoint: self.endpoint.clone(),
            size: request.size.clone(),
            rate: CloudRate {
                amount: 0.20,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: CloudSleepMode::Stop,
            lease_id: "lease-1".into(),
            lease_expires_at_ms: 900_000,
        })
    }

    async fn wake(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError> {
        self.seen_wake_request_ids
            .lock()
            .push(request_id.to_owned());
        if *self.hang_wake.lock() {
            std::future::pending::<()>().await;
        }
        if self.advance_per_wake_ms > 0 {
            self.clock.advance(self.advance_per_wake_ms);
        }
        if let Some(Err(error)) = self.wake_outcomes.lock().pop_front() {
            return Err(error);
        }
        Ok(ProvisionedMachine {
            ingress_ready_at: None,
            machine_id: machine_id.to_owned(),
            provider: "fly".into(),
            endpoint: self.endpoint.clone(),
            size: "small".into(),
            rate: CloudRate {
                amount: 0.20,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: CloudSleepMode::Stop,
            lease_id: "lease-2".into(),
            lease_expires_at_ms: 900_000,
        })
    }

    async fn sleep(&self, _: &str, _: &str) -> Result<CloudSleepMode, CloudError> {
        Ok(CloudSleepMode::Stop)
    }

    async fn delete(&self, _: &str, _: &str) -> Result<(), CloudError> {
        Ok(())
    }

    async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        _: &str,
    ) -> Result<ProvisionedLease, CloudError> {
        Ok(ProvisionedLease {
            machine_id: machine_id.to_owned(),
            lease_id: lease_id.to_owned(),
            lease_expires_at_ms: 900_000,
        })
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(CloudUsageSummary::default())
    }
}

#[test]
fn endpoint_known_without_guest_link_is_not_ready() {
    let endpoint_only = evaluate_guest_link_readiness(&GuestLinkReadinessInput {
        traffic_open: true,
        auth_refused: false,
        installation_unavailable: false,
        stream_attached: false,
        stream_binding_token: None,
        peer_online: false,
        link_nonce: None,
        has_persisted_endpoint: true,
    });
    assert!(!endpoint_only.ready);
    assert_eq!(endpoint_only.link_generation, None);

    let stream_linked = evaluate_guest_link_readiness(&GuestLinkReadinessInput {
        traffic_open: true,
        auth_refused: false,
        installation_unavailable: false,
        stream_attached: true,
        stream_binding_token: Some("binding-a".into()),
        peer_online: false,
        link_nonce: None,
        has_persisted_endpoint: true,
    });
    assert!(stream_linked.ready);
    assert_eq!(
        stream_linked.link_generation.as_deref(),
        Some("stream:binding-a")
    );

    let account_linked = evaluate_guest_link_readiness(&GuestLinkReadinessInput {
        traffic_open: true,
        auth_refused: false,
        installation_unavailable: false,
        stream_attached: false,
        stream_binding_token: None,
        peer_online: true,
        link_nonce: Some("nonce-1".into()),
        has_persisted_endpoint: true,
    });
    assert!(account_linked.ready);
    assert_eq!(
        account_linked.link_generation.as_deref(),
        Some("link:nonce-1")
    );

    let blocked = evaluate_guest_link_readiness(&GuestLinkReadinessInput {
        traffic_open: false,
        auth_refused: false,
        installation_unavailable: false,
        stream_attached: true,
        stream_binding_token: Some("binding-a".into()),
        peer_online: true,
        link_nonce: Some("nonce-1".into()),
        has_persisted_endpoint: true,
    });
    assert!(!blocked.ready);
    assert_eq!(blocked.link_generation, None);
}

#[tokio::test]
async fn link_event_flushes_once_per_generation_and_never_replays_written_or_uncertain() {
    let dir = tempfile::tempdir().unwrap();
    let clock = StepClock::new(10_000);
    let addr = CloudEndpoint::Direct(SocketAddrV4::new(
        Ipv4Addr::new(198, 51, 100, 10),
        CLOUD_INGRESS_PORT,
    ));
    let prov = StubProvisioning {
        create_addr: addr.clone(),
        wake_addr: addr,
    };
    let peer_ctrl = InstrumentedPeerControl::new();
    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "controller-1".into(),
        clock,
    );
    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("gen-box".into()),
                ..Default::default()
            },
            &prov,
            &StubEnrollment,
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    let held = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "tab-1",
            "deliv-gen-1".into(),
            "POST".into(),
            "/api/tabs/tab-1/send".into(),
            serde_json::json!({"text": "once-only", "client_token": "tok-1"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();

    // First link generation hits proven pre-write RefusedMachineSleeping -> stays Held.
    peer_ctrl
        .flush_outcomes
        .lock()
        .push_back(FlushWriteOutcome::RefusedMachineSleeping);
    ctrl.flush_held_sends_for_link_generation(
        &created.machine_id,
        "stream:gen-1",
        peer_ctrl.as_ref(),
    )
    .await;
    assert_eq!(peer_ctrl.flushed_ids.lock().len(), 1);
    assert_eq!(
        ctrl.inspect_held_send(&held.send_id).unwrap().state,
        ChatSendState::Pending
    );

    // Duplicate event for the SAME link generation must not re-attempt flush.
    ctrl.flush_held_sends_for_link_generation(
        &created.machine_id,
        "stream:gen-1",
        peer_ctrl.as_ref(),
    )
    .await;
    assert_eq!(peer_ctrl.flushed_ids.lock().len(), 1);

    // New link generation flushes exactly once and delivers.
    peer_ctrl
        .flush_outcomes
        .lock()
        .push_back(FlushWriteOutcome::Delivered);
    ctrl.flush_held_sends_for_link_generation(
        &created.machine_id,
        "stream:gen-2",
        peer_ctrl.as_ref(),
    )
    .await;
    assert_eq!(peer_ctrl.flushed_ids.lock().len(), 2);
    let status = ctrl.inspect_held_send(&held.send_id).unwrap();
    assert_eq!(status.state, ChatSendState::Sent);
    assert_eq!(status.correlation_id.as_deref(), Some("tok-1"));

    // Subsequent link generation never replays delivered send.
    ctrl.flush_held_sends_for_link_generation(
        &created.machine_id,
        "stream:gen-3",
        peer_ctrl.as_ref(),
    )
    .await;
    assert_eq!(peer_ctrl.flushed_ids.lock().len(), 2);

    // Uncertain write becomes submit_unconfirmed and is NEVER replayed on a newer link generation.
    let uncertain = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "tab-1",
            "deliv-gen-2".into(),
            "POST".into(),
            "/api/tabs/tab-1/send".into(),
            serde_json::json!({"text": "uncertain-once"}),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();
    peer_ctrl
        .flush_outcomes
        .lock()
        .push_back(FlushWriteOutcome::Uncertain("reset after write".into()));
    ctrl.flush_held_sends_for_link_generation(
        &created.machine_id,
        "stream:gen-4",
        peer_ctrl.as_ref(),
    )
    .await;
    assert_eq!(peer_ctrl.flushed_ids.lock().len(), 3);
    assert_eq!(
        ctrl.inspect_held_send(&uncertain.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    ctrl.flush_held_sends_for_link_generation(
        &created.machine_id,
        "stream:gen-5",
        peer_ctrl.as_ref(),
    )
    .await;
    assert_eq!(peer_ctrl.flushed_ids.lock().len(), 3);
}

#[tokio::test]
async fn deadline_expiry_reaches_expired_not_delivered_and_later_link_does_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    let clock = StepClock::new(10_000);
    let addr = CloudEndpoint::Direct(SocketAddrV4::new(
        Ipv4Addr::new(198, 51, 100, 10),
        CLOUD_INGRESS_PORT,
    ));
    let prov = StubProvisioning {
        create_addr: addr.clone(),
        wake_addr: addr,
    };
    let peer_ctrl = InstrumentedPeerControl::new();
    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(store_path.clone()),
        "controller-1".into(),
        clock.clone(),
    );
    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("expire-box".into()),
                ..Default::default()
            },
            &prov,
            &StubEnrollment,
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    let held = ctrl
        .hold_send_for_wake(
            &created.machine_id,
            "tab-1",
            "deliv-expire-1".into(),
            "POST".into(),
            "/api/tabs/tab-1/send".into(),
            serde_json::json!({"text": "keep-for-resend", "client_token": "tok-exp"}),
            AuthenticatedOrigin::Owner,
            None,
            5_000,
        )
        .unwrap();

    clock.advance(4_999);
    assert_eq!(
        ctrl.inspect_held_send(&held.send_id).unwrap().state,
        ChatSendState::Pending
    );

    // Reaching the deadline settles honestly to expired_not_delivered, never stranded held.
    clock.advance(1);
    let expired_status = ctrl.inspect_held_send(&held.send_id).unwrap();
    assert_eq!(expired_status.state, ChatSendState::Failed);
    assert!(expired_status.pending_reason.is_none());
    assert!(expired_status
        .error
        .as_deref()
        .is_some_and(|e| e.starts_with(EXPIRED_NOT_DELIVERED_CODE)));
    assert_eq!(expired_status.correlation_id.as_deref(), Some("tok-exp"));

    // A later link event after expiry must never write the expired send.
    ctrl.flush_held_sends_for_link_generation(
        &created.machine_id,
        "stream:late-link",
        peer_ctrl.as_ref(),
    )
    .await;
    assert!(peer_ctrl.flushed_ids.lock().is_empty());

    // Reloading from disk preserves the terminal expired_not_delivered state.
    let reloaded = CloudMachineController::new(
        CloudMachineStore::new(store_path),
        "controller-1".into(),
        clock,
    );
    let persisted_status = reloaded.inspect_held_send(&held.send_id).unwrap();
    assert_eq!(persisted_status.state, ChatSendState::Failed);
    assert!(persisted_status
        .error
        .as_deref()
        .is_some_and(|e| e.starts_with(EXPIRED_NOT_DELIVERED_CODE)));
}

#[tokio::test]
async fn unconfirmed_and_starting_wake_retry_once_with_same_request_id_inside_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let clock = StepClock::new(50_000);
    let endpoint = CloudEndpoint::Direct(SocketAddrV4::new(
        Ipv4Addr::new(198, 51, 100, 77),
        CLOUD_INGRESS_PORT,
    ));
    let prov = ScriptedWakeProvisioning::new(endpoint.clone(), clock.clone(), 1_000);
    let peer_ctrl = InstrumentedPeerControl::new();
    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "controller-1".into(),
        clock.clone(),
    );
    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("retry-wake-box".into()),
                ..Default::default()
            },
            &prov,
            &StubEnrollment,
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    // Case 1: provider_unconfirmed retries ONCE with the exact same request_id and succeeds.
    ctrl.sleep(&created.machine_id, &prov, peer_ctrl.as_ref(), true)
        .await
        .unwrap();
    prov.wake_outcomes
        .lock()
        .push_back(Err(CloudError::ProviderUnconfirmed(
            "adopt in flight".into(),
        )));
    prov.wake_outcomes.lock().push_back(Ok(()));
    let receipt = ctrl
        .wake(&created.machine_id, &prov, peer_ctrl.as_ref(), 60_000)
        .await
        .unwrap();
    assert_eq!(receipt.state, CloudLifecycleState::Awake);
    {
        let ids = prov.seen_wake_request_ids.lock().clone();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1], "retry must reuse the exact same request_id");
    }

    // Case 2: bounded to ONE retry (two total attempts) if both fail.
    prov.seen_wake_request_ids.lock().clear();
    ctrl.sleep(&created.machine_id, &prov, peer_ctrl.as_ref(), true)
        .await
        .unwrap();
    prov.wake_outcomes
        .lock()
        .push_back(Err(CloudError::ProviderUnconfirmed("first fail".into())));
    prov.wake_outcomes
        .lock()
        .push_back(Err(CloudError::ProviderUnconfirmed("second fail".into())));
    let err = ctrl
        .wake(&created.machine_id, &prov, peer_ctrl.as_ref(), 60_000)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "provider_unconfirmed");
    {
        let ids = prov.seen_wake_request_ids.lock().clone();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1]);
    }

    // Case 3: skip retry when remaining time is below MIN_WAKE_RETRY_READINESS_BUDGET_MS.
    let slow_prov = ScriptedWakeProvisioning::new(endpoint.clone(), clock.clone(), 50_000);
    slow_prov
        .wake_outcomes
        .lock()
        .push_back(Err(CloudError::ProviderUnconfirmed("slow timeout".into())));
    let err = ctrl
        .wake(
            &created.machine_id,
            &slow_prov,
            peer_ctrl.as_ref(),
            50_000 + MIN_WAKE_RETRY_READINESS_BUDGET_MS - 1,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "provider_unconfirmed");
    assert_eq!(slow_prov.seen_wake_request_ids.lock().len(), 1);

    // Case 4: hanging provisioning.wake times out against the deadline as pending unconfirmed.
    let hanging_prov = ScriptedWakeProvisioning::new(endpoint, clock, 0);
    *hanging_prov.hang_wake.lock() = true;
    let err = ctrl
        .wake(&created.machine_id, &hanging_prov, peer_ctrl.as_ref(), 25)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "provider_unconfirmed");
    assert_eq!(hanging_prov.seen_wake_request_ids.lock().len(), 1);
    assert!(
        ctrl.find_by_peer(&created.machine_id)
            .unwrap()
            .pending_request
            .is_some_and(|p| p.op == PendingProvisioningOp::Wake && p.unconfirmed),
        "timed-out wake must leave pending_request marked unconfirmed"
    );
}

#[tokio::test]
async fn repeated_wakes_with_persistent_link_generation_deliver_each_held_send() {
    let dir = tempfile::tempdir().unwrap();
    let clock = StepClock::new(10_000);
    let addr = CloudEndpoint::Direct(SocketAddrV4::new(
        Ipv4Addr::new(198, 51, 100, 88),
        CLOUD_INGRESS_PORT,
    ));
    let prov = StubProvisioning {
        create_addr: addr.clone(),
        wake_addr: addr,
    };
    let peer_ctrl = InstrumentedPeerControl::new();
    // Persistent account-mesh link_id stays identical across sleep/wake cycles.
    *peer_ctrl.link_generation.lock() = Some("link:persistent-nonce".into());

    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "controller-1".into(),
        clock,
    );
    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("multi-wake-box".into()),
                ..Default::default()
            },
            &prov,
            &StubEnrollment,
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();

    for (idx, delivery_id) in [(1, "deliv-cycle-1"), (2, "deliv-cycle-2")] {
        ctrl.sleep(&created.machine_id, &prov, peer_ctrl.as_ref(), true)
            .await
            .unwrap();
        let held = ctrl
            .hold_send_for_wake(
                &created.machine_id,
                "tab-1",
                delivery_id.into(),
                "POST".into(),
                "/api/tabs/tab-1/send".into(),
                serde_json::json!({ "text": format!("wake-cycle-{idx}") }),
                AuthenticatedOrigin::Owner,
                None,
                60_000,
            )
            .unwrap();
        ctrl.wake(&created.machine_id, &prov, peer_ctrl.as_ref(), 60_000)
            .await
            .unwrap();
        assert_eq!(
            ctrl.inspect_held_send(&held.send_id).unwrap().state,
            ChatSendState::Sent,
            "held send from wake cycle {idx} must be delivered even with the same link generation"
        );
    }
    assert_eq!(peer_ctrl.flushed_ids.lock().len(), 2);
    assert_eq!(peer_ctrl.asleep_notes.load(Ordering::SeqCst), 2);
}
