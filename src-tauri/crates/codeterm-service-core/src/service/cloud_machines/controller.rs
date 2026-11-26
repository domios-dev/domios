use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use parking_lot::Mutex;
use rand::RngCore;
use zeroize::Zeroizing;

use crate::mesh::crown::{AuthenticatedAgent, AuthenticatedOrigin};
use crate::service::chat_send_transaction::ChatSendStatus;

use super::held_sends::{
    CloudWakeQueue, FlushWriteOutcome, HeldCloudSend, SingleFlightWakes, WakeSlot,
    DEFAULT_WAKE_HOLD_DEADLINE_MS,
};
use super::idle_drain::{IdlePolicy, IdleSleepAction, LocalPeerDrainGate};
use super::placement::{select_placement, CloudTarget, PlacementDecision};
use super::ports::{
    Clock, CloudEnrollmentPort, CloudPeerControlPort, CloudProvisioningPort, CloudRouteSpec,
    EnrollmentOutcome, ProvisioningCreateRequest, SystemClock,
};
use super::state::{can_issue_enrollment_apply, ownership_labels, transition, LifecycleEvent};
use super::store::{CloudMachineStore, CloudWakeQueueStore};
use super::types::{
    CloudCostSummary, CloudError, CloudLifecycleState, CloudMachineRecord, CloudOwnershipReceipt,
    CloudReceipt, CloudSleepMode, CloudUsageSummary, PendingProvisioningOp,
    PendingProvisioningRequest, DEFAULT_LEASE_RENEW_LEAD_MS,
};

#[derive(Clone, Debug, Default)]
pub struct CreateCloudMachineParams {
    pub size: String,
    pub name: Option<String>,
    pub retry_machine_id: Option<String>,
    pub extra_labels: BTreeMap<String, String>,
}

pub fn mint_request_id() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub struct CloudMachineController {
    store: CloudMachineStore,
    wake_queue_store: CloudWakeQueueStore,
    machines: Mutex<Vec<CloudMachineRecord>>,
    wake_queue: Mutex<CloudWakeQueue>,
    flush_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    wakes: Arc<SingleFlightWakes>,
    lifecycle_tx: tokio::sync::watch::Sender<u64>,
    local_drain_gate: Arc<LocalPeerDrainGate>,
    pub(super) clock: Arc<dyn Clock>,
    controller_id: String,
}

impl CloudMachineController {
    pub fn new(store: CloudMachineStore, controller_id: String, clock: Arc<dyn Clock>) -> Self {
        let loaded = store.load();
        let wake_queue_store = store.wake_queue_store();
        let (next_seq, sends) = wake_queue_store.load();
        let (queue, reconciled_claimed) = CloudWakeQueue::from_persisted(next_seq, sends);
        if reconciled_claimed {
            let (seq, snapshot) = queue.snapshot_persisted();
            if let Err(error) = wake_queue_store.save(seq, &snapshot) {
                log::warn!("[cloud] failed to persist reconciled wake queue on startup: {error}");
            }
        }
        let (lifecycle_tx, _) = tokio::sync::watch::channel(0u64);
        Self {
            store,
            wake_queue_store,
            machines: Mutex::new(loaded),
            wake_queue: Mutex::new(queue),
            flush_locks: Mutex::new(HashMap::new()),
            wakes: Arc::new(SingleFlightWakes::default()),
            lifecycle_tx,
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

    pub fn subscribe_lifecycle(&self) -> tokio::sync::watch::Receiver<u64> {
        self.lifecycle_tx.subscribe()
    }

    fn notify_lifecycle(&self) {
        self.lifecycle_tx
            .send_modify(|rev| *rev = rev.wrapping_add(1));
    }

    pub fn local_drain_gate(&self) -> &Arc<LocalPeerDrainGate> {
        &self.local_drain_gate
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    fn machine_flush_lock(&self, machine_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.flush_locks.lock();
        Arc::clone(
            locks
                .entry(machine_id.to_owned())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    fn persist_locked(&self, machines: &[CloudMachineRecord]) -> Result<(), CloudError> {
        self.store
            .save(machines)
            .map_err(|error| CloudError::Internal(format!("persist cloud machines: {error}")))
    }

    fn persist_wake_queue_locked(&self, queue: &CloudWakeQueue) -> Result<(), CloudError> {
        let (next_seq, sends) = queue.snapshot_persisted();
        self.wake_queue_store
            .save(next_seq, &sends)
            .map_err(|error| CloudError::Internal(format!("persist cloud wake queue: {error}")))
    }

    pub(super) fn find_record(&self, selector: &str) -> Result<CloudMachineRecord, CloudError> {
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

    pub(super) fn update_record(
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
        drop(machines);
        self.notify_lifecycle();
        Ok(updated)
    }

    fn remove_tentative_record(&self, machine_id: &str) {
        let mut machines = self.machines.lock();
        let before = machines.len();
        machines.retain(|m| m.machine_id != machine_id);
        let removed = machines.len() != before;
        if removed {
            let _ = self.persist_locked(&machines);
        }
        drop(machines);
        if removed {
            self.notify_lifecycle();
        }
    }

    /// Persist a fresh request id before any backend call, or reuse the existing persisted id on retry.
    pub(super) fn begin_pending_request(
        &self,
        machine_id: &str,
        op: PendingProvisioningOp,
    ) -> Result<String, CloudError> {
        let now_ms = self.clock.now_ms();
        let mut chosen = String::new();
        self.update_record(machine_id, |rec| {
            let mut next = rec.clone();
            if let Some(existing) = &rec.pending_request {
                if existing.op == op {
                    chosen = existing.request_id.clone();
                    next.pending_request = Some(PendingProvisioningRequest {
                        op,
                        request_id: existing.request_id.clone(),
                        started_at_ms: existing.started_at_ms,
                        unconfirmed: false,
                    });
                    return Ok(next);
                }
            }
            let request_id = mint_request_id();
            chosen = request_id.clone();
            next.pending_request = Some(PendingProvisioningRequest {
                op,
                request_id,
                started_at_ms: now_ms,
                unconfirmed: false,
            });
            Ok(next)
        })?;
        Ok(chosen)
    }

    pub(super) fn mark_pending_unconfirmed(
        &self,
        machine_id: &str,
        op: PendingProvisioningOp,
        reason: &str,
    ) {
        let _ = self.update_record(machine_id, |rec| {
            let mut next = rec.clone();
            if let Some(pending) = next.pending_request.as_mut().filter(|p| p.op == op) {
                pending.unconfirmed = true;
            }
            next.last_error = Some(reason.to_owned());
            Ok(next)
        });
    }

    pub(super) fn clear_pending_request(&self, machine_id: &str) {
        let _ = self.update_record(machine_id, |rec| {
            let mut next = rec.clone();
            next.pending_request = None;
            Ok(next)
        });
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

    pub async fn usage(
        &self,
        provisioning: &dyn CloudProvisioningPort,
    ) -> Result<CloudUsageSummary, CloudError> {
        provisioning.usage().await
    }

    pub async fn create(
        &self,
        params: CreateCloudMachineParams,
        provisioning: &dyn CloudProvisioningPort,
        enrollment: &dyn CloudEnrollmentPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudReceipt, CloudError> {
        let size = if params.size.trim().is_empty() {
            "small".to_owned()
        } else {
            params.size.trim().to_owned()
        };
        let requested_name = params
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned);

        // Reuse a persisted pending create request id on retry after an uncertain outcome.
        let (machine_id, request_id, name) = {
            let mut machines = self.machines.lock();
            let existing_idx = if let Some(retry_id) = params.retry_machine_id.as_deref() {
                machines.iter().position(|m| {
                    m.machine_id == retry_id
                        && m.state == CloudLifecycleState::Creating
                        && m.pending_request
                            .as_ref()
                            .is_some_and(|p| p.op == PendingProvisioningOp::Create)
                })
            } else {
                machines.iter().position(|m| {
                    m.state == CloudLifecycleState::Creating
                        && m.size == size
                        && requested_name
                            .as_deref()
                            .is_none_or(|wanted| m.name == wanted)
                        && m.pending_request
                            .as_ref()
                            .is_some_and(|p| p.op == PendingProvisioningOp::Create && p.unconfirmed)
                })
            };

            if let Some(idx) = existing_idx {
                let rec = &mut machines[idx];
                let req_id = rec
                    .pending_request
                    .as_ref()
                    .map(|p| p.request_id.clone())
                    .unwrap_or_else(mint_request_id);
                if let Some(pending) = rec.pending_request.as_mut() {
                    pending.unconfirmed = false;
                }
                let mid = rec.machine_id.clone();
                let mname = rec.name.clone();
                self.persist_locked(&machines)?;
                (mid, req_id, mname)
            } else {
                let now_ms = self.clock.now_ms();
                let request_id = mint_request_id();
                let short = &request_id[..8.min(request_id.len())];
                let machine_id = format!("cloud-{short}");
                let name = requested_name.unwrap_or_else(|| format!("cloud-{short}"));
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
                    provider: "managed".into(),
                    size: size.clone(),
                    state: CloudLifecycleState::Creating,
                    sleep_mode: CloudSleepMode::DeleteRecreate,
                    installation_id: None,
                    peer_instance_id: None,
                    endpoint_addr: None,
                    rate_per_hour: None,
                    created_at_ms: now_ms,
                    last_awake_at_ms: None,
                    last_owner_interaction_at_ms: None,
                    accumulated_awake_secs: 0,
                    period_start_ms: now_ms,
                    lease_id: None,
                    lease_expires_at_ms: None,
                    exhaustion_pending: false,
                    idle_since_ms: None,
                    apply_unconfirmed: false,
                    pending_request: Some(PendingProvisioningRequest {
                        op: PendingProvisioningOp::Create,
                        request_id: request_id.clone(),
                        started_at_ms: now_ms,
                        unconfirmed: false,
                    }),
                    last_error: None,
                    ownership: CloudOwnershipReceipt {
                        provider_machine_id: String::new(),
                        floating_ip_id: None,
                        auxiliary_resource_ids: BTreeMap::new(),
                        controller_id: self.controller_id.clone(),
                        labels,
                    },
                };
                machines.push(initial);
                self.persist_locked(&machines)?;
                (machine_id, request_id, name)
            }
        };

        let binding = match enrollment.prepare_binding().await {
            Ok(binding) => binding,
            Err(error) => {
                self.remove_tentative_record(&machine_id);
                return Err(error);
            }
        };

        let create_req = ProvisioningCreateRequest {
            size: size.clone(),
            name: Some(name),
            controller_key: binding.controller_key.clone(),
            creation_psk: Zeroizing::new((*binding.provider_creation_nonce).clone()),
            request_id: request_id.clone(),
        };

        let provisioned = provisioning.create(&create_req).await;
        // Drop the request copy of the creation PSK immediately after the backend call.
        drop(create_req);

        let provisioned = match provisioned {
            Ok(machine) => machine,
            Err(error) => {
                drop(binding);
                if error.is_uncertain_provisioning_outcome() {
                    self.mark_pending_unconfirmed(
                        &machine_id,
                        PendingProvisioningOp::Create,
                        &error.to_string(),
                    );
                } else {
                    self.remove_tentative_record(&machine_id);
                }
                return Err(error);
            }
        };

        let enrolling = self.update_record(&machine_id, |rec| {
            let mut next = transition(
                rec,
                LifecycleEvent::ProviderCreated {
                    provider_machine_id: provisioned.machine_id.clone(),
                },
                self.clock.now_ms(),
            )?;
            next.provider = provisioned.provider.clone();
            next.size = provisioned.size.clone();
            next.sleep_mode = provisioned.sleep_mode;
            next.rate_per_hour = Some(provisioned.rate.clone());
            next.endpoint_addr = Some(provisioned.endpoint);
            next.lease_id = Some(provisioned.lease_id.clone());
            next.lease_expires_at_ms = Some(provisioned.lease_expires_at_ms);
            next.pending_request = None;
            Ok(next)
        })?;

        if !can_issue_enrollment_apply(&enrolling) {
            drop(binding);
            return Err(CloudError::ApplyUnconfirmed {
                machine_id,
                installation_id: enrolling.installation_id.unwrap_or_default(),
            });
        }

        let route = CloudRouteSpec::Direct {
            address: provisioned.endpoint,
        };
        let enroll_result = enrollment
            .enroll(&route, &binding, &provisioned.provider, &provisioned.size)
            .await;
        // Creation secret must not outlive bootstrap enrollment.
        drop(binding);

        match enroll_result {
            Ok(EnrollmentOutcome::Enrolled {
                installation_id,
                peer_instance_id,
            }) => {
                let enrolled = self.update_record(&machine_id, |rec| {
                    let mut next = rec.clone();
                    next.installation_id = Some(installation_id.clone());
                    if peer_instance_id.is_some() {
                        next.peer_instance_id = peer_instance_id.clone();
                    }
                    Ok(next)
                })?;
                if let Err(error) = peer_control
                    .update_endpoint(&enrolled, provisioned.endpoint)
                    .await
                {
                    let _ = self.update_record(&machine_id, |rec| {
                        transition(
                            rec,
                            LifecycleEvent::Failed(error.to_string()),
                            self.clock.now_ms(),
                        )
                    });
                    return Err(error);
                }
                if let Err(error) = peer_control
                    .publish_direct_endpoint(&enrolled, provisioned.endpoint)
                    .await
                {
                    let _ = self.update_record(&machine_id, |rec| {
                        transition(
                            rec,
                            LifecycleEvent::Failed(error.to_string()),
                            self.clock.now_ms(),
                        )
                    });
                    return Err(error);
                }
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
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
        delete_on_stop_billed: bool,
    ) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(selector)?;
        if record.state == CloudLifecycleState::Asleep {
            return Ok(record.receipt(self.clock.now_ms(), None));
        }

        let policy = IdlePolicy {
            idle_window_ms: 0,
            lease_renew_lead_ms: DEFAULT_LEASE_RENEW_LEAD_MS,
            max_awake_without_owner_ms: None,
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

        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Sleep)?;
        match provisioning
            .sleep(draining.backend_machine_id(), &request_id)
            .await
        {
            Ok(sleep_mode) => {
                let now_ms = self.clock.now_ms();
                let asleep = self.update_record(&record.machine_id, |rec| {
                    let mut next = transition(rec, LifecycleEvent::ProviderStopped, now_ms)?;
                    next.sleep_mode = sleep_mode;
                    next.pending_request = None;
                    Ok(next)
                })?;
                let warning = (sleep_mode == CloudSleepMode::DeleteRecreate
                    || action == IdleSleepAction::DeleteForRecreate)
                    .then(|| {
                        "machine uses delete_recreate sleep mode; instance was imaged and deleted to stop billing and will recreate with a new IPv4 on next wake".into()
                    });
                Ok(asleep.receipt(now_ms, warning))
            }
            Err(error) => {
                if error.is_uncertain_provisioning_outcome() {
                    self.mark_pending_unconfirmed(
                        &record.machine_id,
                        PendingProvisioningOp::Sleep,
                        &error.to_string(),
                    );
                } else {
                    self.clear_pending_request(&record.machine_id);
                }
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
        provisioning: &dyn CloudProvisioningPort,
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

                let request_id = match self
                    .begin_pending_request(&record.machine_id, PendingProvisioningOp::Wake)
                {
                    Ok(id) => id,
                    Err(error) => {
                        guard.finish(Err(error.to_string()));
                        return Err(error);
                    }
                };

                let woken = match provisioning
                    .wake(waking.backend_machine_id(), &request_id)
                    .await
                {
                    Ok(machine) => machine,
                    Err(error) => {
                        if matches!(error, CloudError::CreditsExhausted) {
                            let _ = self.update_record(&record.machine_id, |rec| {
                                let mut next = transition(
                                    rec,
                                    LifecycleEvent::CreditsExhausted,
                                    self.clock.now_ms(),
                                )?;
                                next.pending_request = None;
                                Ok(next)
                            });
                        } else {
                            if error.is_uncertain_provisioning_outcome() {
                                self.mark_pending_unconfirmed(
                                    &record.machine_id,
                                    PendingProvisioningOp::Wake,
                                    &error.to_string(),
                                );
                            } else {
                                self.clear_pending_request(&record.machine_id);
                            }
                            let _ = self.update_record(&record.machine_id, |rec| {
                                transition(
                                    rec,
                                    LifecycleEvent::WakeFailed(error.to_string()),
                                    self.clock.now_ms(),
                                )
                            });
                        }
                        guard.finish(Err(error.to_string()));
                        return Err(error);
                    }
                };

                // Persist updated endpoint (new IPv4 on delete_recreate) and lease before publication and held-send flush.
                let waking = match self.update_record(&record.machine_id, |rec| {
                    let mut next = rec.clone();
                    next.provider = woken.provider.clone();
                    next.size = woken.size.clone();
                    next.sleep_mode = woken.sleep_mode;
                    next.rate_per_hour = Some(woken.rate.clone());
                    next.endpoint_addr = Some(woken.endpoint);
                    next.lease_id = Some(woken.lease_id.clone());
                    next.lease_expires_at_ms = Some(woken.lease_expires_at_ms);
                    next.exhaustion_pending = false;
                    next.pending_request = None;
                    Ok(next)
                }) {
                    Ok(updated) => updated,
                    Err(error) => {
                        guard.finish(Err(error.to_string()));
                        return Err(error);
                    }
                };

                if let Err(error) = peer_control.update_endpoint(&waking, woken.endpoint).await {
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

                // Publish signed direct endpoint before peer readiness and held-send release; failure counts as not ready.
                if let Err(error) = peer_control
                    .publish_direct_endpoint(&waking, woken.endpoint)
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
        provisioning: &dyn CloudProvisioningPort,
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
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Delete)?;
        if let Err(error) = provisioning
            .delete(record.backend_machine_id(), &request_id)
            .await
        {
            if error.is_uncertain_provisioning_outcome() {
                self.mark_pending_unconfirmed(
                    &record.machine_id,
                    PendingProvisioningOp::Delete,
                    &error.to_string(),
                );
            } else {
                self.clear_pending_request(&record.machine_id);
            }
            return Err(error);
        }
        let now_ms = self.clock.now_ms();
        let deleted = self.update_record(&record.machine_id, |rec| {
            let mut next = transition(rec, LifecycleEvent::Deleted, now_ms)?;
            next.pending_request = None;
            Ok(next)
        })?;
        Ok(deleted.receipt(now_ms, None))
    }

    pub async fn resolve_placement(
        &self,
        target: &CloudTarget,
        create_params: Option<CreateCloudMachineParams>,
        provisioning: &dyn CloudProvisioningPort,
        enrollment: &dyn CloudEnrollmentPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudReceipt, CloudError> {
        let decision = {
            let machines = self.machines.lock();
            select_placement(&machines, target)?
        };
        match decision {
            PlacementDecision::ReuseAwake(id) => {
                let record = self.find_record(&id)?;
                Ok(record.receipt(self.clock.now_ms(), None))
            }
            PlacementDecision::WakeAndReuse(id) => {
                self.wake(
                    &id,
                    provisioning,
                    peer_control,
                    DEFAULT_WAKE_HOLD_DEADLINE_MS,
                )
                .await
            }
            PlacementDecision::CreateExplicit => {
                let params = create_params.unwrap_or_default();
                self.create(params, provisioning, enrollment, peer_control)
                    .await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hold_send_for_wake(
        &self,
        machine_id: &str,
        tab_id: &str,
        delivery_id: String,
        method: String,
        path: String,
        body: serde_json::Value,
        origin: AuthenticatedOrigin,
        agent: Option<AuthenticatedAgent>,
        hold_timeout_ms: u64,
    ) -> Result<HeldCloudSend, CloudError> {
        let now_ms = self.clock.now_ms();
        let deadline_ms = now_ms.saturating_add(hold_timeout_ms);
        let mut queue = self.wake_queue.lock();
        let held = queue.admit(
            machine_id,
            tab_id,
            delivery_id,
            method,
            path,
            body,
            origin,
            agent,
            now_ms,
            deadline_ms,
        )?;
        self.persist_wake_queue_locked(&queue)?;
        Ok(held)
    }

    pub async fn flush_held_sends(
        &self,
        machine_id: &str,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Vec<HeldCloudSend> {
        // Serialize claim, flush, and settlement per machine so concurrent wakes/flushes never race.
        let flush_lock = self.machine_flush_lock(machine_id);
        let _flush_guard = flush_lock.lock().await;

        let Ok(record) = self.find_record(machine_id) else {
            return Vec::new();
        };
        let mut settled = Vec::new();
        loop {
            let next = {
                let now_ms = self.clock.now_ms();
                let mut queue = self.wake_queue.lock();
                let claimed = queue.claim_next_for_machine(machine_id, now_ms);
                if claimed.is_some() {
                    // Persist Claimed before writing so restart promotes it to SubmitUnconfirmed, never replaying.
                    if let Err(error) = self.persist_wake_queue_locked(&queue) {
                        log::warn!("[cloud] failed to persist claimed send before flush: {error}");
                        if let Some(ref c) = claimed {
                            let _ = queue.settle_claimed(
                                &c.send_id,
                                FlushWriteOutcome::RefusedMachineSleeping,
                            );
                        }
                        break;
                    }
                }
                claimed
            };
            let Some(claimed) = next else {
                break;
            };
            let outcome = peer_control.flush_send(&record, &claimed).await;
            let keep_held = outcome == FlushWriteOutcome::RefusedMachineSleeping;
            let updated = {
                let mut queue = self.wake_queue.lock();
                let updated = queue.settle_claimed(&claimed.send_id, outcome);
                if let Err(error) = self.persist_wake_queue_locked(&queue) {
                    log::warn!("[cloud] failed to persist settled send outcome: {error}");
                }
                updated
            };
            if let Some(updated) = updated {
                settled.push(updated);
            }
            if keep_held {
                break;
            }
        }
        settled
    }

    pub fn inspect_held_send(&self, send_id: &str) -> Option<ChatSendStatus> {
        self.wake_queue.lock().inspect(send_id)
    }

    pub fn cancel_held_send(&self, send_id: &str) -> Result<ChatSendStatus, CloudError> {
        let mut queue = self.wake_queue.lock();
        let rec = queue.cancel_before_write(send_id)?;
        self.persist_wake_queue_locked(&queue)?;
        Ok(rec.to_chat_send_status())
    }

    pub fn held_send_count(&self) -> usize {
        self.wake_queue.lock().total_held()
    }

    pub fn expire_overdue_held_sends(&self, machine_id: &str) -> Vec<HeldCloudSend> {
        let now_ms = self.clock.now_ms();
        let mut queue = self.wake_queue.lock();
        let expired = queue.expire_overdue(machine_id, now_ms);
        if !expired.is_empty() {
            let _ = self.persist_wake_queue_locked(&queue);
        }
        expired
    }

    pub fn set_out_of_credits(&self, selector: &str) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(selector)?;
        let now_ms = self.clock.now_ms();
        let updated = self.update_record(&record.machine_id, |rec| {
            transition(rec, LifecycleEvent::CreditsExhausted, now_ms)
        })?;
        Ok(updated.receipt(now_ms, None))
    }

    pub fn restore_credits(&self, selector: &str) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(selector)?;
        let now_ms = self.clock.now_ms();
        let updated = self.update_record(&record.machine_id, |rec| {
            transition(rec, LifecycleEvent::CreditsRestored, now_ms)
        })?;
        Ok(updated.receipt(now_ms, None))
    }
}
