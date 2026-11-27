use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use zeroize::Zeroizing;

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

fn local_only_record(
    machine_id: &str,
    name: &str,
    request_id: Option<&str>,
    created_at_ms: u64,
    unconfirmed: bool,
) -> CloudMachineRecord {
    let mut labels = BTreeMap::new();
    if let Some(req) = request_id {
        labels.insert(LABEL_CREATE_REQUEST_ID.into(), req.into());
    }
    CloudMachineRecord {
        machine_id: machine_id.into(),
        name: name.into(),
        provider: "managed".into(),
        size: "small".into(),
        state: CloudLifecycleState::Creating,
        sleep_mode: CloudSleepMode::StopBilled,
        installation_id: None,
        peer_instance_id: None,
        endpoint: None,
        rate_per_hour: None,
        created_at_ms,
        last_awake_at_ms: None,
        last_owner_interaction_at_ms: None,
        accumulated_awake_secs: 0,
        period_start_ms: created_at_ms,
        lease_id: None,
        lease_expires_at_ms: None,
        exhaustion_pending: false,
        idle_since_ms: None,
        apply_unconfirmed: false,
        pending_request: request_id.map(|req| PendingProvisioningRequest {
            op: PendingProvisioningOp::Create,
            request_id: req.into(),
            started_at_ms: created_at_ms,
            unconfirmed,
        }),
        last_error: unconfirmed.then(|| "timeout".into()),
        ownership: CloudOwnershipReceipt {
            provider_machine_id: String::new(),
            floating_ip_id: None,
            auxiliary_resource_ids: BTreeMap::new(),
            controller_id: "home-1".into(),
            labels,
        },
    }
}

fn backend_entry(
    provider_machine_id: &str,
    create_request_id: Option<&str>,
    name: Option<&str>,
    created_at_ms: Option<u64>,
) -> BackendMachineEntry {
    BackendMachineEntry {
        provider_machine_id: provider_machine_id.into(),
        create_request_id: create_request_id.map(str::to_owned),
        name: name.map(str::to_owned),
        created_at_ms,
        provider: Some("fly_machines".into()),
        size: Some("small".into()),
        state: Some(CloudLifecycleState::Awake),
        storage_tier: Some(CloudStorageTier::Hot),
        stopped_at_ms: None,
    }
}

#[test]
fn pure_reconcile_model_adopts_by_request_id_or_name_window_and_deletes_local_when_absent() {
    let policy = LocalReconcilePolicy::default();
    let rec = local_only_record(
        "cloud-EwiBSlgz",
        "quill-fly-124",
        Some("req-create-1"),
        1_000_000,
        true,
    );

    // 1. Exact create_request_id match adopts even if name differs or time window is wide.
    let by_req = backend_entry(
        "cm_a30376f6",
        Some("req-create-1"),
        Some("other-name"),
        Some(5_000_000),
    );
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            std::slice::from_ref(&rec),
            std::slice::from_ref(&by_req),
            1_005_000,
            LocalRecordReconcileIntent::Delete,
            policy,
        ),
        LocalRecordReconcileAction::Adopt {
            entry: by_req.clone(),
            reason: BackendMatchReason::CreateRequestId,
        }
    );

    // 2. Name + creation time window adopts when request_id is absent on backend entry.
    let by_name = backend_entry(
        "cm_name_match",
        None,
        Some("quill-fly-124"),
        Some(1_030_000),
    );
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            std::slice::from_ref(&rec),
            std::slice::from_ref(&by_name),
            1_035_000,
            LocalRecordReconcileIntent::ReconcileCreate,
            policy,
        ),
        LocalRecordReconcileAction::Adopt {
            entry: by_name,
            reason: BackendMatchReason::NameAndTimeWindow,
        }
    );

    // 3. Name outside time window does not match -> DeleteLocal on delete intent.
    let too_old = backend_entry(
        "cm_too_old",
        None,
        Some("quill-fly-124"),
        Some(1_000_000 + policy.name_match_window_ms + 1),
    );
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            std::slice::from_ref(&rec),
            std::slice::from_ref(&too_old),
            1_035_000,
            LocalRecordReconcileIntent::Delete,
            policy,
        ),
        LocalRecordReconcileAction::DeleteLocal
    );

    // 4. Contradicting request_id prevents false name-window adoption.
    let different_req = backend_entry(
        "cm_diff_req",
        Some("req-different"),
        Some("quill-fly-124"),
        Some(1_001_000),
    );
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            std::slice::from_ref(&rec),
            std::slice::from_ref(&different_req),
            1_005_000,
            LocalRecordReconcileIntent::Delete,
            policy,
        ),
        LocalRecordReconcileAction::DeleteLocal
    );

    // 5. Empty backend list: Delete intent deletes locally; ReconcileCreate keeps uncertain
    //    inside grace window and cleans up locally once injected clock passes grace window.
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            std::slice::from_ref(&rec),
            &[],
            1_005_000,
            LocalRecordReconcileIntent::Delete,
            policy,
        ),
        LocalRecordReconcileAction::DeleteLocal
    );
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            std::slice::from_ref(&rec),
            &[],
            1_000_000 + policy.uncertain_create_grace_ms - 1,
            LocalRecordReconcileIntent::ReconcileCreate,
            policy,
        ),
        LocalRecordReconcileAction::KeepUncertain
    );
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            std::slice::from_ref(&rec),
            &[],
            1_000_000 + policy.uncertain_create_grace_ms,
            LocalRecordReconcileIntent::ReconcileCreate,
            policy,
        ),
        LocalRecordReconcileAction::CleanUpLocal
    );

    // 6. Already-claimed backend ids and purged entries are skipped; multiple matches are ambiguous.
    let mut existing = rec.clone();
    existing.machine_id = "cloud-existing".into();
    existing.ownership.provider_machine_id = "cm_a30376f6".into();
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            &[rec.clone(), existing],
            std::slice::from_ref(&by_req),
            1_005_000,
            LocalRecordReconcileIntent::Delete,
            policy,
        ),
        LocalRecordReconcileAction::DeleteLocal
    );

    let dup_a = backend_entry("cm_dup_a", None, Some("quill-fly-124"), Some(1_001_000));
    let dup_b = backend_entry("cm_dup_b", None, Some("quill-fly-124"), Some(1_002_000));
    assert_eq!(
        plan_local_record_reconcile(
            &rec,
            std::slice::from_ref(&rec),
            &[dup_a, dup_b],
            1_005_000,
            LocalRecordReconcileIntent::Delete,
            policy,
        ),
        LocalRecordReconcileAction::Ambiguous {
            candidate_backend_ids: vec!["cm_dup_a".into(), "cm_dup_b".into()],
        }
    );

    // 7. Adopting transitions Creating -> Enrolling (or Asleep if stopped), never Failed.
    let adopted = apply_local_record_adoption(&rec, &by_req, 1_010_000).unwrap();
    assert_eq!(adopted.provider_machine_id(), Some("cm_a30376f6"));
    assert_eq!(adopted.state, CloudLifecycleState::Enrolling);
    assert_eq!(adopted.provider, "fly_machines");
    assert_eq!(adopted.pending_request, None);
    assert_eq!(adopted.last_error, None);

    let mut stopped_entry = by_req;
    stopped_entry.stopped_at_ms = Some(1_008_000);
    let adopted_asleep = apply_local_record_adoption(&rec, &stopped_entry, 1_010_000).unwrap();
    assert_eq!(adopted_asleep.state, CloudLifecycleState::Asleep);
}

struct HygieneProvisioning {
    backend_list: Mutex<Vec<BackendMachineEntry>>,
    create_outcomes: Mutex<VecDeque<Result<ProvisionedMachine, CloudError>>>,
    seen_delete_ids: Mutex<Vec<String>>,
    seen_wake_ids: Mutex<Vec<String>>,
    seen_sleep_ids: Mutex<Vec<String>>,
    create_calls: AtomicUsize,
    list_calls: AtomicUsize,
    create_backend_on_timeout: Mutex<Option<String>>,
}

impl HygieneProvisioning {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            backend_list: Mutex::new(Vec::new()),
            create_outcomes: Mutex::new(VecDeque::new()),
            seen_delete_ids: Mutex::new(Vec::new()),
            seen_wake_ids: Mutex::new(Vec::new()),
            seen_sleep_ids: Mutex::new(Vec::new()),
            create_calls: AtomicUsize::new(0),
            list_calls: AtomicUsize::new(0),
            create_backend_on_timeout: Mutex::new(None),
        })
    }
}

#[async_trait]
impl CloudProvisioningPort for HygieneProvisioning {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        self.create_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(backend_id) = self.create_backend_on_timeout.lock().take() {
            // Simulate backend completing creation even though the client request timed out.
            self.backend_list.lock().push(BackendMachineEntry {
                provider_machine_id: backend_id,
                create_request_id: Some(request.request_id.clone()),
                name: request.name.clone(),
                created_at_ms: Some(100_000),
                provider: Some("fly_machines".into()),
                size: Some(request.size.clone()),
                state: Some(CloudLifecycleState::Awake),
                storage_tier: Some(CloudStorageTier::Hot),
                stopped_at_ms: None,
            });
        }
        self.create_outcomes
            .lock()
            .pop_front()
            .unwrap_or_else(|| Err(CloudError::ProviderUnavailable("Offline".into())))
    }

    async fn wake(
        &self,
        machine_id: &str,
        _request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError> {
        self.seen_wake_ids.lock().push(machine_id.to_owned());
        Err(CloudError::Internal("unused".into()))
    }

    async fn sleep(
        &self,
        machine_id: &str,
        _request_id: &str,
    ) -> Result<CloudSleepMode, CloudError> {
        self.seen_sleep_ids.lock().push(machine_id.to_owned());
        Ok(CloudSleepMode::StopBilled)
    }

    async fn delete(&self, machine_id: &str, _request_id: &str) -> Result<(), CloudError> {
        self.seen_delete_ids.lock().push(machine_id.to_owned());
        self.backend_list
            .lock()
            .retain(|entry| entry.provider_machine_id != machine_id);
        Ok(())
    }

    async fn renew_lease(
        &self,
        _machine_id: &str,
        _lease_id: &str,
        _request_id: &str,
    ) -> Result<ProvisionedLease, CloudError> {
        Err(CloudError::Internal("unused".into()))
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(CloudUsageSummary::default())
    }

    async fn list_backend_machines(&self) -> Result<Vec<BackendMachineEntry>, CloudError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.backend_list.lock().clone())
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
            installation_id: "inst-hygiene".into(),
            peer_instance_id: Some("peer-hygiene".into()),
        })
    }
}

struct StubPeerControl;

#[async_trait]
impl CloudPeerControlPort for StubPeerControl {
    async fn read_idle(
        &self,
        _record: &CloudMachineRecord,
    ) -> Result<PeerBusySnapshot, CloudError> {
        Ok(PeerBusySnapshot::default())
    }

    async fn enter_drain(&self, _record: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        Ok(DrainToken {
            token: "tok".into(),
            entered_at_ms: 0,
        })
    }

    async fn reopen_drain(&self, _record: &CloudMachineRecord) -> Result<(), CloudError> {
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
        _send: &HeldCloudSend,
    ) -> FlushWriteOutcome {
        FlushWriteOutcome::Written
    }
}

#[tokio::test]
async fn local_only_record_deletes_locally_when_absent_and_never_sends_local_id_to_backend() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    CloudMachineStore::new(store_path.clone())
        .save(&[local_only_record(
            "cloud-EwiBSlgz",
            "quill-fly-124",
            Some("req-stuck-1"),
            100_000,
            true,
        )])
        .unwrap();

    let clock = InjectedClock::new(120_000);
    let ctrl =
        CloudMachineController::new(CloudMachineStore::new(store_path), "home-1".into(), clock);
    let prov = HygieneProvisioning::new();

    // Without confirmation, refuses without calling backend list or delete.
    let err = ctrl
        .delete("cloud-EwiBSlgz", false, prov.as_ref())
        .await
        .unwrap_err();
    assert_eq!(err.code(), "delete_confirmation_required");
    assert_eq!(prov.list_calls.load(Ordering::SeqCst), 0);
    assert!(prov.seen_delete_ids.lock().is_empty());

    // With confirmation and empty backend list, deletes locally and never sends local id.
    let receipt = ctrl
        .delete("cloud-EwiBSlgz", true, prov.as_ref())
        .await
        .expect("local-only cloud record must delete locally when absent on backend");
    assert_eq!(receipt.machine_id, "cloud-EwiBSlgz");
    assert_eq!(receipt.state, CloudLifecycleState::Deleted);
    assert_eq!(prov.list_calls.load(Ordering::SeqCst), 1);
    assert!(prov.seen_delete_ids.lock().is_empty());
    assert!(ctrl.list().is_empty());
}

#[tokio::test]
async fn local_only_record_delete_adopts_backend_id_and_deletes_backend_machine() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    CloudMachineStore::new(store_path.clone())
        .save(&[local_only_record(
            "cloud-EwiBSlgz",
            "quill-fly-124",
            Some("req-stuck-2"),
            100_000,
            true,
        )])
        .unwrap();

    let clock = InjectedClock::new(130_000);
    let ctrl =
        CloudMachineController::new(CloudMachineStore::new(store_path), "home-1".into(), clock);
    let prov = HygieneProvisioning::new();
    prov.backend_list.lock().push(backend_entry(
        "cm_adopted_real",
        None,
        Some("quill-fly-124"),
        Some(105_000),
    ));

    let receipt = ctrl
        .delete("cloud-EwiBSlgz", true, prov.as_ref())
        .await
        .expect("local-only record matching backend machine must adopt backend id then delete");
    assert_eq!(receipt.state, CloudLifecycleState::Deleted);
    assert_eq!(
        prov.seen_delete_ids.lock().as_slice(),
        &["cm_adopted_real".to_owned()]
    );
    assert!(ctrl.list().is_empty());
}

#[tokio::test]
async fn post_write_create_timeout_reconciles_and_adopts_created_machine_instead_of_looking_failed()
{
    let dir = tempfile::tempdir().unwrap();
    let clock = InjectedClock::new(100_000);
    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "home-1".into(),
        clock,
    );
    let prov = HygieneProvisioning::new();
    *prov.create_backend_on_timeout.lock() = Some("cm_a30376f6".into());
    prov.create_outcomes
        .lock()
        .push_back(Err(CloudError::ProviderUnavailable("Offline".into())));

    let err = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("quill-fly-orphan".into()),
                ..Default::default()
            },
            prov.as_ref(),
            &StubEnrollment,
            &StubPeerControl,
        )
        .await
        .unwrap_err();
    // Surfaces as provider_unconfirmed (uncertain), never provider_unavailable.
    assert_eq!(err.code(), "provider_unconfirmed");

    // Backend machine id is adopted immediately; record is Enrolling, never Failed or orphan.
    let records = ctrl.list_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].provider_machine_id(), Some("cm_a30376f6"));
    assert_eq!(records[0].state, CloudLifecycleState::Enrolling);
    assert_ne!(records[0].state, CloudLifecycleState::Failed);
    assert!(records[0].create_request_id().is_some());

    // Subsequent delete targets the adopted backend id, never the local id.
    let deleted = ctrl
        .delete(&records[0].machine_id, true, prov.as_ref())
        .await
        .unwrap();
    assert_eq!(deleted.state, CloudLifecycleState::Deleted);
    assert_eq!(
        prov.seen_delete_ids.lock().as_slice(),
        &["cm_a30376f6".to_owned()]
    );
}

#[tokio::test]
async fn uncertain_create_stays_uncertain_inside_grace_window_then_adopts_or_cleans_up_by_clock() {
    let dir = tempfile::tempdir().unwrap();
    let clock = InjectedClock::new(200_000);
    let ctrl = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "home-1".into(),
        clock.clone(),
    );
    let prov = HygieneProvisioning::new();
    prov.create_outcomes
        .lock()
        .push_back(Err(CloudError::ProviderUnavailable("Offline".into())));

    let err = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("slow-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            &StubEnrollment,
            &StubPeerControl,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "provider_unconfirmed");
    let rec = ctrl.list_records()[0].clone();
    assert_eq!(rec.state, CloudLifecycleState::Creating);
    assert_eq!(rec.provider_machine_id(), None);
    let req_id = rec.create_request_id().unwrap().to_owned();

    // Still inside grace window with empty backend list -> kept uncertain, not failed or deleted.
    clock.advance_ms(60_000);
    let r1 = ctrl.reconcile_local_records(prov.as_ref()).await.unwrap();
    assert_eq!(r1.kept_uncertain_ids, vec![rec.machine_id.clone()]);
    assert_eq!(ctrl.list_records()[0].state, CloudLifecycleState::Creating);

    // Wake on a local-only record without backend id must refuse without sending local id.
    let wake_err = ctrl
        .wake(&rec.machine_id, prov.as_ref(), &StubPeerControl, 10_000)
        .await
        .unwrap_err();
    assert_eq!(wake_err.code(), "invalid_transition");
    assert!(prov.seen_wake_ids.lock().is_empty());

    // Once backend list reports the machine by create_request_id, reconcile adopts it.
    prov.backend_list.lock().push(backend_entry(
        "cm_late_appear",
        Some(&req_id),
        Some("slow-box"),
        Some(205_000),
    ));
    let r2 = ctrl.reconcile_local_records(prov.as_ref()).await.unwrap();
    assert_eq!(r2.adopted_ids, vec![rec.machine_id.clone()]);
    assert_eq!(
        ctrl.list_records()[0].provider_machine_id(),
        Some("cm_late_appear")
    );
    assert_eq!(ctrl.list_records()[0].state, CloudLifecycleState::Enrolling);

    // A second local-only record with no backend match past the grace window is cleaned up.
    let dir2 = tempfile::tempdir().unwrap();
    let clock2 = InjectedClock::new(500_000);
    let store2 = CloudMachineStore::new(dir2.path().join("cloud_machines.json"));
    store2
        .save(&[local_only_record(
            "cloud-stale",
            "ghost-box",
            Some("req-ghost"),
            500_000,
            true,
        )])
        .unwrap();
    let ctrl2 = CloudMachineController::new(store2, "home-1".into(), clock2.clone());
    let prov2 = HygieneProvisioning::new();
    clock2.advance_ms(DEFAULT_UNCERTAIN_CREATE_GRACE_MS);
    let r3 = ctrl2.reconcile_local_records(prov2.as_ref()).await.unwrap();
    assert_eq!(r3.cleaned_up_ids, vec!["cloud-stale".to_owned()]);
    assert!(ctrl2.list().is_empty());
}
