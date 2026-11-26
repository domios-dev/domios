use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use zeroize::Zeroizing;

use super::*;

struct FixedClock(AtomicU64);

impl FixedClock {
    fn new(now_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(now_ms)))
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct ProxyProvisioning {
    create_endpoint: Mutex<CloudEndpoint>,
    wake_endpoint: Mutex<CloudEndpoint>,
}

#[async_trait]
impl CloudProvisioningPort for ProxyProvisioning {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        Ok(ProvisionedMachine {
            machine_id: "cm_fly_1".into(),
            provider: "fly_machines".into(),
            endpoint: self.create_endpoint.lock().clone(),
            size: request.size.clone(),
            rate: CloudRate {
                amount: 0.04,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: CloudSleepMode::StopBilled,
            lease_id: "lease-fly-1".into(),
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
            provider: "fly_machines".into(),
            endpoint: self.wake_endpoint.lock().clone(),
            size: "small".into(),
            rate: CloudRate {
                amount: 0.04,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: CloudSleepMode::StopBilled,
            lease_id: "lease-fly-2".into(),
            lease_expires_at_ms: 900_000,
        })
    }

    async fn sleep(&self, _: &str, _: &str) -> Result<CloudSleepMode, CloudError> {
        Ok(CloudSleepMode::StopBilled)
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

struct StubEnrollment {
    seen_routes: Mutex<Vec<CloudRouteSpec>>,
}

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
        route: &CloudRouteSpec,
        _: &EnrollmentBinding,
        _: &str,
        _: &str,
    ) -> Result<EnrollmentOutcome, CloudError> {
        self.seen_routes.lock().push(route.clone());
        Ok(EnrollmentOutcome::Enrolled {
            installation_id: "inst-fly".into(),
            peer_instance_id: Some("peer-fly".into()),
        })
    }
}

struct CountingPeerControl {
    updates: Mutex<Vec<CloudEndpoint>>,
    publishes: Mutex<Vec<CloudEndpoint>>,
    ready_calls: AtomicUsize,
}

impl CountingPeerControl {
    fn new() -> Self {
        Self {
            updates: Mutex::new(Vec::new()),
            publishes: Mutex::new(Vec::new()),
            ready_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl CloudPeerControlPort for CountingPeerControl {
    async fn read_idle(&self, _: &CloudMachineRecord) -> Result<PeerBusySnapshot, CloudError> {
        Ok(PeerBusySnapshot::default())
    }
    async fn enter_drain(&self, _: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        Ok(DrainToken {
            token: "drain".into(),
            entered_at_ms: 1_000,
        })
    }
    async fn reopen_drain(&self, _: &CloudMachineRecord) -> Result<(), CloudError> {
        Ok(())
    }
    async fn update_endpoint(
        &self,
        _: &CloudMachineRecord,
        endpoint: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        self.updates.lock().push(endpoint.clone());
        Ok(())
    }
    async fn publish_direct_endpoint(
        &self,
        _: &CloudMachineRecord,
        endpoint: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        self.publishes.lock().push(endpoint.clone());
        Ok(())
    }
    async fn await_peer_ready(&self, _: &CloudMachineRecord, _: u64) -> Result<(), CloudError> {
        self.ready_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn flush_send(&self, _: &CloudMachineRecord, _: &HeldCloudSend) -> FlushWriteOutcome {
        FlushWriteOutcome::Delivered
    }
}

#[test]
fn legacy_persisted_endpoint_addr_loads_as_direct_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cloud_machines.json");
    let legacy_json = serde_json::json!({
        "schema_version": 1,
        "machines": [{
            "machine_id": "cloud-legacy-1",
            "name": "legacy-box",
            "provider": "timeweb",
            "size": "small",
            "state": "awake",
            "sleep_mode": "delete_recreate",
            "installation_id": "inst-legacy",
            "peer_instance_id": "peer-legacy",
            "endpoint_addr": "198.51.100.10:7687",
            "created_at_ms": 1_000,
            "period_start_ms": 1_000,
            "ownership": {
                "provider_machine_id": "prov-legacy",
                "controller_id": "ctrl-1"
            }
        }]
    });
    std::fs::write(&path, serde_json::to_string_pretty(&legacy_json).unwrap()).unwrap();

    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(path),
        "ctrl-1".into(),
        FixedClock::new(2_000),
    );
    let record = ctrl.find_by_peer("cloud-legacy-1").unwrap();
    assert_eq!(
        record.endpoint,
        Some(CloudEndpoint::Direct(SocketAddrV4::new(
            Ipv4Addr::new(198, 51, 100, 10),
            CLOUD_INGRESS_PORT,
        )))
    );
}

#[tokio::test]
async fn provider_proxy_wake_with_stable_host_is_noop_for_update_and_republication() {
    let dir = tempfile::tempdir().unwrap();
    let clock = FixedClock::new(10_000);
    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "ctrl-fly".into(),
        clock,
    );
    let stable = CloudEndpoint::ProviderProxy {
        host: "domios-a1b2c3d4.fly.dev".into(),
    };
    let prov = ProxyProvisioning {
        create_endpoint: Mutex::new(stable.clone()),
        wake_endpoint: Mutex::new(stable.clone()),
    };
    let enrollment = StubEnrollment {
        seen_routes: Mutex::new(Vec::new()),
    };
    let peer_ctrl = CountingPeerControl::new();

    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("fly-box".into()),
                retry_machine_id: None,
                extra_labels: BTreeMap::new(),
            },
            &prov,
            &enrollment,
            &peer_ctrl,
        )
        .await
        .unwrap();
    assert_eq!(created.endpoint, Some(stable.clone()));
    assert_eq!(enrollment.seen_routes.lock().as_slice(), &[stable.clone()]);
    assert_eq!(peer_ctrl.updates.lock().as_slice(), &[stable.clone()]);
    assert_eq!(peer_ctrl.publishes.lock().as_slice(), &[stable.clone()]);

    peer_ctrl.updates.lock().clear();
    peer_ctrl.publishes.lock().clear();

    ctrl.sleep(&created.machine_id, &prov, &peer_ctrl, true)
        .await
        .unwrap();
    let woken = ctrl
        .wake(&created.machine_id, &prov, &peer_ctrl, 60_000)
        .await
        .unwrap();
    assert_eq!(woken.endpoint, Some(stable));
    assert!(
        peer_ctrl.updates.lock().is_empty(),
        "stable provider_proxy host wake must not re-update endpoint"
    );
    assert!(
        peer_ctrl.publishes.lock().is_empty(),
        "stable provider_proxy host wake must not re-publish endpoint"
    );
    assert_eq!(peer_ctrl.ready_calls.load(Ordering::SeqCst), 1);

    // If the host actually changes on a later wake, it updates and re-publishes the new host.
    let changed = CloudEndpoint::ProviderProxy {
        host: "domios-e5f6a7b8.fly.dev".into(),
    };
    *prov.wake_endpoint.lock() = changed.clone();
    ctrl.sleep(&created.machine_id, &prov, &peer_ctrl, true)
        .await
        .unwrap();
    let woken2 = ctrl
        .wake(&created.machine_id, &prov, &peer_ctrl, 60_000)
        .await
        .unwrap();
    assert_eq!(woken2.endpoint, Some(changed.clone()));
    assert_eq!(peer_ctrl.updates.lock().as_slice(), &[changed.clone()]);
    assert_eq!(peer_ctrl.publishes.lock().as_slice(), &[changed]);
}
