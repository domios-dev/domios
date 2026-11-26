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
}

impl Clock for StepClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct StubProvisioning {
    create_addr: SocketAddrV4,
    wake_addr: SocketAddrV4,
}

#[async_trait]
impl CloudProvisioningPort for StubProvisioning {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        Ok(ProvisionedMachine {
            machine_id: "cm_stub_1".into(),
            provider: "timeweb".into(),
            endpoint: self.create_addr,
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
            machine_id: machine_id.to_owned(),
            provider: "timeweb".into(),
            endpoint: self.wake_addr,
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
    published: Mutex<Vec<(String, SocketAddrV4)>>,
    events: Mutex<Vec<String>>,
    flushed_ids: Mutex<Vec<String>>,
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
            published: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            flushed_ids: Mutex::new(Vec::new()),
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
        endpoint: SocketAddrV4,
    ) -> Result<(), CloudError> {
        self.events.lock().push(format!("update:{endpoint}"));
        Ok(())
    }

    async fn publish_direct_endpoint(
        &self,
        record: &CloudMachineRecord,
        endpoint: SocketAddrV4,
    ) -> Result<(), CloudError> {
        self.events.lock().push(format!("publish:{endpoint}"));
        if let Some(outcome) = self.publish_outcomes.lock().pop_front() {
            outcome?;
        }
        self.published
            .lock()
            .push((record.installation_id.clone().unwrap_or_default(), endpoint));
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
        FlushWriteOutcome::Delivered
    }
}

#[tokio::test]
async fn persisted_claimed_send_becomes_submit_unconfirmed_on_restart_and_never_replays() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    let clock = StepClock::new(10_000);
    let prov = StubProvisioning {
        create_addr: SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 10), CLOUD_INGRESS_PORT),
        wake_addr: SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 20), CLOUD_INGRESS_PORT),
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
        create_addr: SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 10), CLOUD_INGRESS_PORT),
        wake_addr: SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 20), CLOUD_INGRESS_PORT),
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
    let create_addr = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 10), CLOUD_INGRESS_PORT);
    let wake_addr = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 99), CLOUD_INGRESS_PORT);
    let prov = StubProvisioning {
        create_addr,
        wake_addr,
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
