use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::service::chat_send_transaction::ChatSendStatus;

use super::held_sends::{
    CloudWakeQueue, FlushWriteOutcome, HeldCloudSend, SingleFlightWakes, WakeSlot,
    DEFAULT_WAKE_HOLD_DEADLINE_MS,
};
use super::idle_drain::{
    evaluate_idle_and_renewal, IdleDecision, IdleEvalInput, IdlePolicy, IdleSleepAction,
    LocalPeerDrainGate, PeerBusySnapshot,
};
use super::placement::{
    check_machine_limit, select_placement, CloudLimitPolicy, CloudTarget, PlacementDecision,
};
use super::ports::{
    Clock, CloudBackendPort, CloudCreateRequest, CloudEnrollmentPort, CloudPeerControlPort,
    EnrollmentOutcome, SystemClock,
};
use super::state::{
    can_issue_enrollment_apply, ownership_labels, reconcile_provider_machines, transition,
    LifecycleEvent, ReconcileOutcome, LABEL_CONTROLLER, LABEL_MANAGED,
};
use super::store::CloudMachineStore;
use super::types::{
    CloudCostSummary, CloudError, CloudLifecycleState, CloudMachineRecord, CloudOwnershipReceipt,
    CloudReceipt, CLOUD_INGRESS_PORT, DEFAULT_AUTO_STOP_MINUTES,
};

pub const BOOTSTRAP_CONTROLLER_KEY_ENV: &str = "DOMIOS_BOOTSTRAP_CONTROLLER_KEY";
pub const BOOTSTRAP_NONCE_ENV: &str = "DOMIOS_BOOTSTRAP_NONCE";

#[derive(Clone, Debug)]
pub struct CreateCloudMachineParams {
    pub provider: String,
    pub size: String,
    pub name: Option<String>,
    pub auto_stop_minutes: u32,
    pub extra_labels: BTreeMap<String, String>,
}

pub struct CloudMachineController {
    store: CloudMachineStore,
    machines: Mutex<Vec<CloudMachineRecord>>,
    wake_queue: Mutex<CloudWakeQueue>,
    wakes: Arc<SingleFlightWakes>,
    local_drain_gate: Arc<LocalPeerDrainGate>,
    clock: Arc<dyn Clock>,
    controller_id: String,
}

impl CloudMachineController {
    pub fn new(store: CloudMachineStore, controller_id: String, clock: Arc<dyn Clock>) -> Self {
        let loaded = store.load();
        Self {
            store,
            machines: Mutex::new(loaded),
            wake_queue: Mutex::new(CloudWakeQueue::default()),
            wakes: Arc::new(SingleFlightWakes::default()),
            local_drain_gate: Arc::new(LocalPeerDrainGate::default()),
            clock,
            controller_id,
        }
    }

    pub fn open_default(controller_id: String) -> Self {
        Self::new(
            CloudMachineStore::new(CloudMachineStore::default_path()),
            controller_id,
            Arc::new(SystemClock),
        )
    }

    pub fn local_drain_gate(&self) -> &Arc<LocalPeerDrainGate> {
        &self.local_drain_gate
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    fn persist_locked(&self, machines: &[CloudMachineRecord]) -> Result<(), CloudError> {
        self.store
            .save(machines)
            .map_err(|error| CloudError::Internal(format!("persist cloud machines: {error}")))
    }

    fn find_record(&self, selector: &str) -> Result<CloudMachineRecord, CloudError> {
        let machines = self.machines.lock();
        let matches: Vec<&CloudMachineRecord> = machines
            .iter()
            .filter(|m| m.state != CloudLifecycleState::Deleted && m.matches_selector(selector))
            .collect();
        match matches.as_slice() {
            [] => Err(CloudError::MachineNotFound(selector.to_owned())),
            [single] => Ok((*single).clone()),
            _ => Err(CloudError::MachineAmbiguous(selector.to_owned())),
        }
    }

    fn update_record(
        &self,
        machine_id: &str,
        mutate: impl FnOnce(&CloudMachineRecord) -> Result<CloudMachineRecord, CloudError>,
    ) -> Result<CloudMachineRecord, CloudError> {
        let mut machines = self.machines.lock();
        let idx = machines
            .iter()
            .position(|m| m.machine_id == machine_id)
            .ok_or_else(|| CloudError::MachineNotFound(machine_id.to_owned()))?;
        let updated = mutate(&machines[idx])?;
        machines[idx] = updated.clone();
        self.persist_locked(&machines)?;
        Ok(updated)
    }

    pub fn list(&self) -> Vec<CloudReceipt> {
        let now_ms = self.clock.now_ms();
        self.machines
            .lock()
            .iter()
            .filter(|m| m.state != CloudLifecycleState::Deleted)
            .map(|m| m.receipt(now_ms, None))
            .collect()
    }

    pub fn list_records(&self) -> Vec<CloudMachineRecord> {
        self.machines
            .lock()
            .iter()
            .filter(|m| m.state != CloudLifecycleState::Deleted)
            .cloned()
            .collect()
    }

    pub fn find_by_peer(&self, peer_or_installation: &str) -> Option<CloudMachineRecord> {
        let target = peer_or_installation.trim();
        if target.is_empty() {
            return None;
        }
        self.machines
            .lock()
            .iter()
            .find(|m| m.state != CloudLifecycleState::Deleted && m.matches_selector(target))
            .cloned()
    }

    pub fn cost_summary_for_peer(&self, peer_or_installation: &str) -> Option<CloudCostSummary> {
        let now_ms = self.clock.now_ms();
        self.find_by_peer(peer_or_installation)
            .map(|rec| rec.cost_summary(now_ms))
    }

    pub async fn reconcile(
        &self,
        provider: &str,
        backend: &dyn CloudBackendPort,
    ) -> Result<ReconcileOutcome, CloudError> {
        let mut filter = BTreeMap::new();
        filter.insert(LABEL_MANAGED.into(), "true".into());
        filter.insert(LABEL_CONTROLLER.into(), self.controller_id.clone());
        let observed = backend.list(&filter).await?;
        let sleep_mode = backend.sleep_mode().await.unwrap_or_default();
        let default_rate = backend.hourly_rate("small").await.ok().flatten();
        let now_ms = self.clock.now_ms();

        let mut machines = self.machines.lock();
        let outcome = reconcile_provider_machines(
            &machines,
            &observed,
            &self.controller_id,
            provider,
            sleep_mode,
            default_rate,
            now_ms,
        );
        *machines = outcome.records.clone();
        self.persist_locked(&machines)?;
        Ok(outcome)
    }

    pub async fn create(
        &self,
        params: CreateCloudMachineParams,
        backend: &dyn CloudBackendPort,
        enrollment: &dyn CloudEnrollmentPort,
        limit_policy: CloudLimitPolicy,
    ) -> Result<CloudReceipt, CloudError> {
        {
            let machines = self.machines.lock();
            check_machine_limit(&machines, limit_policy)?;
        }

        let binding = enrollment.prepare_binding().await?;
        let sleep_mode = backend.sleep_mode().await?;
        let size = if params.size.trim().is_empty() {
            "small".to_owned()
        } else {
            params.size.trim().to_owned()
        };
        let rate_per_hour = backend.hourly_rate(&size).await.ok().flatten();
        let image_id = backend.prepare_image(&size).await?;

        let now_ms = self.clock.now_ms();
        let request_id = uuid::Uuid::new_v4();
        let short = &request_id.simple().to_string()[..8];
        let machine_id = format!("cloud-{short}");
        let name = params
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("cloud-{short}"));
        let auto_stop_minutes = if params.auto_stop_minutes == 0 {
            DEFAULT_AUTO_STOP_MINUTES
        } else {
            params.auto_stop_minutes
        };
        let labels = ownership_labels(
            &self.controller_id,
            &machine_id,
            &name,
            &size,
            &params.extra_labels,
        );

        let initial = CloudMachineRecord {
            machine_id: machine_id.clone(),
            name: name.clone(),
            provider: params.provider.clone(),
            size: size.clone(),
            state: CloudLifecycleState::Creating,
            sleep_mode,
            installation_id: None,
            peer_instance_id: None,
            rate_per_hour,
            created_at_ms: now_ms,
            last_awake_at_ms: None,
            accumulated_awake_secs: 0,
            period_start_ms: now_ms,
            auto_stop_minutes,
            last_activity_refresh_ms: None,
            idle_since_ms: None,
            apply_unconfirmed: false,
            last_error: None,
            ownership: CloudOwnershipReceipt {
                provider_machine_id: String::new(),
                floating_ip_id: None,
                auxiliary_resource_ids: BTreeMap::new(),
                controller_id: self.controller_id.clone(),
                labels: labels.clone(),
            },
        };
        {
            let mut machines = self.machines.lock();
            check_machine_limit(&machines, limit_policy)?;
            machines.push(initial);
            self.persist_locked(&machines)?;
        }

        let mut env = BTreeMap::new();
        env.insert(
            BOOTSTRAP_CONTROLLER_KEY_ENV.into(),
            binding.controller_key.clone(),
        );
        env.insert(
            BOOTSTRAP_NONCE_ENV.into(),
            (*binding.provider_creation_nonce).clone(),
        );
        let create_req = CloudCreateRequest {
            size: size.clone(),
            image_id,
            request_id,
            labels,
            env,
            auto_stop_minutes,
        };

        let created = match backend.create(&create_req).await {
            Ok(machine) => machine,
            Err(error) => {
                let fail_now = self.clock.now_ms();
                let _ = self.update_record(&machine_id, |rec| {
                    transition(rec, LifecycleEvent::Failed(error.to_string()), fail_now)
                });
                return Err(error);
            }
        };

        let enrolling = self.update_record(&machine_id, |rec| {
            transition(
                rec,
                LifecycleEvent::ProviderCreated {
                    provider_machine_id: created.provider_machine_id.clone(),
                },
                self.clock.now_ms(),
            )
        })?;

        if !can_issue_enrollment_apply(&enrolling) {
            return Err(CloudError::ApplyUnconfirmed {
                machine_id,
                installation_id: enrolling.installation_id.unwrap_or_default(),
            });
        }

        let route = match backend
            .access(&created.provider_machine_id, CLOUD_INGRESS_PORT)
            .await
        {
            Ok(route) => route,
            Err(error) => {
                let _ = self.update_record(&machine_id, |rec| {
                    transition(
                        rec,
                        LifecycleEvent::Failed(error.to_string()),
                        self.clock.now_ms(),
                    )
                });
                return Err(error);
            }
        };

        match enrollment
            .enroll(&route, &binding, &params.provider, &size)
            .await
        {
            Ok(EnrollmentOutcome::Enrolled {
                installation_id,
                peer_instance_id,
            }) => {
                let awake_now = self.clock.now_ms();
                let awake = self.update_record(&machine_id, |rec| {
                    transition(
                        rec,
                        LifecycleEvent::EnrolledReady {
                            installation_id: installation_id.clone(),
                            peer_instance_id: peer_instance_id.clone(),
                        },
                        awake_now,
                    )
                })?;
                Ok(awake.receipt(awake_now, None))
            }
            Ok(EnrollmentOutcome::ApplyUnconfirmed { installation_id }) => {
                let _ = self.update_record(&machine_id, |rec| {
                    transition(
                        rec,
                        LifecycleEvent::EnrollmentApplyUnconfirmed {
                            installation_id: installation_id.clone(),
                        },
                        self.clock.now_ms(),
                    )
                });
                Err(CloudError::ApplyUnconfirmed {
                    machine_id,
                    installation_id,
                })
            }
            Err(error) => {
                let _ = self.update_record(&machine_id, |rec| {
                    transition(
                        rec,
                        LifecycleEvent::Failed(error.to_string()),
                        self.clock.now_ms(),
                    )
                });
                Err(error)
            }
        }
    }

    pub async fn sleep(
        &self,
        selector: &str,
        backend: &dyn CloudBackendPort,
        peer_control: &dyn CloudPeerControlPort,
        delete_on_stop_billed: bool,
    ) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(selector)?;
        if record.state == CloudLifecycleState::Asleep {
            return Ok(record.receipt(self.clock.now_ms(), None));
        }

        let policy = IdlePolicy {
            idle_window_ms: 0,
            auto_stop_minutes: record.auto_stop_minutes,
            sleep_mode: record.sleep_mode,
            delete_on_stop_billed,
        };
        let action = policy.sleep_action();
        if action == IdleSleepAction::KeepUnderLease {
            return Ok(record.receipt(
                self.clock.now_ms(),
                Some(
                    "provider bills stopped servers in full (stop_billed); keeping running under lease instead of calling stop".into(),
                ),
            ));
        }

        let draining = self.update_record(&record.machine_id, |rec| {
            transition(rec, LifecycleEvent::DrainStarted, self.clock.now_ms())
        })?;

        if let Err(error) = peer_control.enter_drain(&draining).await {
            let _ = self.update_record(&record.machine_id, |rec| {
                transition(rec, LifecycleEvent::DrainAbortedBusy, self.clock.now_ms())
            });
            return Err(error);
        }

        let stop_result = match action {
            IdleSleepAction::StopProvider => {
                backend.stop(&draining.ownership.provider_machine_id).await
            }
            IdleSleepAction::DeleteForRecreate => backend.delete(&draining.ownership).await,
            IdleSleepAction::KeepUnderLease => Ok(()),
        };

        match stop_result {
            Ok(()) => {
                let now_ms = self.clock.now_ms();
                let asleep = self.update_record(&record.machine_id, |rec| {
                    transition(rec, LifecycleEvent::ProviderStopped, now_ms)
                })?;
                let warning = (action == IdleSleepAction::DeleteForRecreate).then(|| {
                    "provider uses delete_recreate sleep mode; instance was deleted to stop billing and will recreate on next wake".into()
                });
                Ok(asleep.receipt(now_ms, warning))
            }
            Err(error) => {
                let _ = peer_control.reopen_drain(&draining).await;
                let _ = self.update_record(&record.machine_id, |rec| {
                    transition(rec, LifecycleEvent::DrainAbortedBusy, self.clock.now_ms())
                });
                Err(error)
            }
        }
    }

    pub async fn wake(
        &self,
        selector: &str,
        backend: &dyn CloudBackendPort,
        peer_control: &dyn CloudPeerControlPort,
        deadline_ms: u64,
    ) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(selector)?;
        if record.state == CloudLifecycleState::OutOfCredits {
            return Err(CloudError::MachineOutOfCredits {
                machine_id: record.machine_id,
            });
        }
        if record.state == CloudLifecycleState::Awake {
            self.flush_held_sends(&record.machine_id, peer_control)
                .await;
            return Ok(record.receipt(self.clock.now_ms(), None));
        }

        match self.wakes.acquire(&record.machine_id) {
            WakeSlot::Follower(in_flight) => {
                in_flight
                    .wait()
                    .await
                    .map_err(CloudError::ProviderUnavailable)?;
                let refreshed = self.find_record(&record.machine_id)?;
                Ok(refreshed.receipt(self.clock.now_ms(), None))
            }
            WakeSlot::Leader(guard) => {
                let waking = if record.state == CloudLifecycleState::Waking {
                    record.clone()
                } else {
                    match self.update_record(&record.machine_id, |rec| {
                        transition(rec, LifecycleEvent::WakeRequested, self.clock.now_ms())
                    }) {
                        Ok(updated) => updated,
                        Err(error) => {
                            guard.finish(Err(error.to_string()));
                            return Err(error);
                        }
                    }
                };

                if let Err(error) = backend
                    .start(
                        &waking.ownership.provider_machine_id,
                        waking.auto_stop_minutes,
                    )
                    .await
                {
                    let _ = self.update_record(&record.machine_id, |rec| {
                        transition(
                            rec,
                            LifecycleEvent::WakeFailed(error.to_string()),
                            self.clock.now_ms(),
                        )
                    });
                    guard.finish(Err(error.to_string()));
                    return Err(error);
                }

                let ready_deadline = self.clock.now_ms().saturating_add(deadline_ms);
                if let Err(error) = peer_control.await_peer_ready(&waking, ready_deadline).await {
                    let _ = self.update_record(&record.machine_id, |rec| {
                        transition(
                            rec,
                            LifecycleEvent::WakeFailed(error.to_string()),
                            self.clock.now_ms(),
                        )
                    });
                    guard.finish(Err(error.to_string()));
                    return Err(error);
                }

                let now_ms = self.clock.now_ms();
                let awake = match self.update_record(&record.machine_id, |rec| {
                    transition(rec, LifecycleEvent::WakeReady, now_ms)
                }) {
                    Ok(updated) => updated,
                    Err(error) => {
                        guard.finish(Err(error.to_string()));
                        return Err(error);
                    }
                };
                guard.finish(Ok(()));
                self.flush_held_sends(&awake.machine_id, peer_control).await;
                Ok(awake.receipt(now_ms, None))
            }
        }
    }

    pub async fn delete(
        &self,
        selector: &str,
        confirm: bool,
        backend: &dyn CloudBackendPort,
    ) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(selector)?;
        if !confirm {
            return Err(CloudError::DeleteConfirmationRequired {
                machine_id: record.machine_id.clone(),
                warning: format!(
                    "Deleting cloud machine '{}' ({}) permanently destroys any unpushed work on it. Re-run with --confirm to proceed.",
                    record.name, record.machine_id
                ),
            });
        }
        backend.delete(&record.ownership).await?;
        let now_ms = self.clock.now_ms();
        let deleted = self.update_record(&record.machine_id, |rec| {
            transition(rec, LifecycleEvent::Deleted, now_ms)
        })?;
        Ok(deleted.receipt(now_ms, None))
    }

    pub async fn resolve_placement(
        &self,
        target: &CloudTarget,
        create_params: Option<CreateCloudMachineParams>,
        backend: &dyn CloudBackendPort,
        enrollment: &dyn CloudEnrollmentPort,
        peer_control: &dyn CloudPeerControlPort,
        limit_policy: CloudLimitPolicy,
    ) -> Result<CloudReceipt, CloudError> {
        let decision = {
            let machines = self.machines.lock();
            select_placement(&machines, target, limit_policy)?
        };
        match decision {
            PlacementDecision::ReuseAwake(id) => {
                let record = self.find_record(&id)?;
                Ok(record.receipt(self.clock.now_ms(), None))
            }
            PlacementDecision::WakeAndReuse(id) => {
                self.wake(&id, backend, peer_control, DEFAULT_WAKE_HOLD_DEADLINE_MS)
                    .await
            }
            PlacementDecision::CreateExplicit => {
                let params = create_params.ok_or_else(|| {
                    CloudError::Internal(
                        "explicit --cloud new requires provider create parameters".into(),
                    )
                })?;
                self.create(params, backend, enrollment, limit_policy).await
            }
        }
    }

    pub fn hold_send_for_wake(
        &self,
        machine_id: &str,
        tab_id: &str,
        delivery_id: String,
        method: String,
        path: String,
        body: serde_json::Value,
        hold_timeout_ms: u64,
    ) -> Result<HeldCloudSend, CloudError> {
        let now_ms = self.clock.now_ms();
        let deadline_ms = now_ms.saturating_add(hold_timeout_ms);
        self.wake_queue.lock().admit(
            machine_id,
            tab_id,
            delivery_id,
            method,
            path,
            body,
            now_ms,
            deadline_ms,
        )
    }

    pub async fn flush_held_sends(
        &self,
        machine_id: &str,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Vec<HeldCloudSend> {
        let Ok(record) = self.find_record(machine_id) else {
            return Vec::new();
        };
        let mut settled = Vec::new();
        loop {
            let next = {
                let now_ms = self.clock.now_ms();
                self.wake_queue
                    .lock()
                    .claim_next_for_machine(machine_id, now_ms)
            };
            let Some(claimed) = next else {
                break;
            };
            let outcome = peer_control.flush_send(&record, &claimed).await;
            let sleeping = outcome == FlushWriteOutcome::RefusedMachineSleeping;
            if let Some(updated) = self
                .wake_queue
                .lock()
                .settle_claimed(&claimed.send_id, outcome)
            {
                settled.push(updated);
            }
            if sleeping {
                break;
            }
        }
        settled
    }

    pub fn inspect_held_send(&self, send_id: &str) -> Option<ChatSendStatus> {
        self.wake_queue.lock().inspect(send_id)
    }

    pub fn cancel_held_send(&self, send_id: &str) -> Result<ChatSendStatus, CloudError> {
        self.wake_queue
            .lock()
            .cancel_before_write(send_id)
            .map(|rec| rec.to_chat_send_status())
    }

    pub fn held_send_count(&self) -> usize {
        self.wake_queue.lock().total_held()
    }

    pub fn expire_overdue_held_sends(&self, machine_id: &str) -> Vec<HeldCloudSend> {
        let now_ms = self.clock.now_ms();
        self.wake_queue.lock().expire_overdue(machine_id, now_ms)
    }

    pub fn set_out_of_credits(&self, selector: &str) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(selector)?;
        let now_ms = self.clock.now_ms();
        let updated = self.update_record(&record.machine_id, |rec| {
            transition(rec, LifecycleEvent::CreditsExhausted, now_ms)
        })?;
        Ok(updated.receipt(now_ms, None))
    }

    pub async fn evaluate_idle_tick(
        &self,
        machine_id: &str,
        busy: PeerBusySnapshot,
        idle_window_ms: u64,
        delete_on_stop_billed: bool,
        backend: &dyn CloudBackendPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<IdleDecision, CloudError> {
        let record = self.find_record(machine_id)?;
        let now_ms = self.clock.now_ms();
        let decision = evaluate_idle_and_renewal(IdleEvalInput {
            state: record.state,
            busy,
            now_ms,
            idle_since_ms: record.idle_since_ms,
            last_activity_refresh_ms: record.last_activity_refresh_ms,
            policy: IdlePolicy {
                idle_window_ms,
                auto_stop_minutes: record.auto_stop_minutes,
                sleep_mode: record.sleep_mode,
                delete_on_stop_billed,
            },
        });
        match decision {
            IdleDecision::RenewActivityNow { .. } => {
                backend
                    .refresh_activity(
                        &record.ownership.provider_machine_id,
                        record.auto_stop_minutes,
                    )
                    .await?;
                let _ = self.update_record(machine_id, |rec| {
                    let mut next = rec.clone();
                    next.last_activity_refresh_ms = Some(now_ms);
                    next.idle_since_ms = None;
                    Ok(next)
                });
            }
            IdleDecision::BeginTwoPhaseDrain { .. } => {
                let _ = self
                    .sleep(machine_id, backend, peer_control, delete_on_stop_billed)
                    .await?;
            }
            IdleDecision::ScheduleAt { idle_since_ms, .. } => {
                if record.idle_since_ms != idle_since_ms {
                    let _ = self.update_record(machine_id, |rec| {
                        let mut next = rec.clone();
                        next.idle_since_ms = idle_since_ms;
                        Ok(next)
                    });
                }
            }
            IdleDecision::None => {}
        }
        Ok(decision)
    }

    pub fn enrich_peers_value(&self, mut value: serde_json::Value) -> serde_json::Value {
        let receipts = self.list();
        if receipts.is_empty() {
            return value;
        }
        let now_ms = self.clock.now_ms();
        let records = self.list_records();
        let lookup = |id: &str| {
            records
                .iter()
                .find(|r| r.matches_selector(id))
                .map(|r| r.cost_summary(now_ms))
        };
        if let Some(installations) = value
            .get_mut("installations")
            .and_then(serde_json::Value::as_array_mut)
        {
            for row in installations {
                let key = row
                    .get("installation_id")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| row.get("instance_id").and_then(serde_json::Value::as_str))
                    .map(str::to_owned);
                if let Some(summary) = key.as_deref().and_then(lookup) {
                    if let Some(obj) = row.as_object_mut() {
                        obj.insert("kind".into(), serde_json::json!("cloud"));
                        obj.insert(
                            "cloud".into(),
                            serde_json::to_value(summary).unwrap_or_default(),
                        );
                    }
                }
            }
        }
        if let Some(paired) = value
            .get_mut("paired")
            .and_then(serde_json::Value::as_array_mut)
        {
            for peer in paired {
                let key = peer
                    .get("instance_id")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        peer.get("instance_name")
                            .and_then(serde_json::Value::as_str)
                    })
                    .map(str::to_owned);
                if let Some(summary) = key.as_deref().and_then(lookup) {
                    if let Some(obj) = peer.as_object_mut() {
                        obj.insert("kind".into(), serde_json::json!("cloud"));
                        obj.insert(
                            "cloud".into(),
                            serde_json::to_value(summary).unwrap_or_default(),
                        );
                    }
                }
            }
        }
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "cloud_machines".into(),
                serde_json::to_value(receipts).unwrap_or_default(),
            );
        }
        value
    }

    pub fn enrich_tab_list(&self, tabs: &mut [serde_json::Value]) {
        let records = self.list_records();
        if records.is_empty() {
            return;
        }
        let now_ms = self.clock.now_ms();
        for tab in tabs {
            let peer = tab
                .get("remote_peer")
                .and_then(serde_json::Value::as_str)
                .or_else(|| tab.get("peer_id").and_then(serde_json::Value::as_str))
                .map(str::to_owned);
            let Some(summary) = peer
                .as_deref()
                .and_then(|target| records.iter().find(|r| r.matches_selector(target)))
                .map(|r| r.cost_summary(now_ms))
            else {
                continue;
            };
            if let Some(obj) = tab.as_object_mut() {
                obj.insert(
                    "cloud".into(),
                    serde_json::to_value(summary).unwrap_or_default(),
                );
            }
        }
    }
}
