use super::controller::{mint_request_id, CloudMachineController};
use super::controller_reconcile::require_provider_machine_id;
use super::idle_drain::{
    evaluate_idle_and_renewal, IdleDecision, IdleEvalInput, IdlePolicy, IdleSleepAction,
    PeerBusySnapshot,
};
use super::ports::{CloudPeerControlPort, CloudProvisioningPort, ProvisionedLease};
use super::state::{
    apply_draining_truth, infer_backend_truth_from_usage, plan_draining_recovery, transition,
    usage_reports_credits_exhausted, BackendDrainingTruth, DrainingReconcileIntent,
    DrainingRecoveryPlan, LifecycleEvent,
};
use super::types::{
    CloudError, CloudLifecycleState, CloudMachineRecord, CloudReceipt, CloudSleepMode,
    PendingProvisioningOp, DEFAULT_LEASE_RENEW_LEAD_MS,
};

pub(super) struct ActiveDrainGuard<'a> {
    controller: &'a CloudMachineController,
    machine_id: String,
}

impl Drop for ActiveDrainGuard<'_> {
    fn drop(&mut self) {
        self.controller.clear_active_drain(&self.machine_id);
    }
}

struct ProbedDrainingTruth {
    truth: BackendDrainingTruth,
    renewed_lease: Option<ProvisionedLease>,
    sleep_mode: Option<CloudSleepMode>,
    already_stopped_on_backend: bool,
}

impl CloudMachineController {
    pub(super) fn acquire_active_drain(&self, machine_id: &str) -> ActiveDrainGuard<'_> {
        self.mark_active_drain(machine_id);
        ActiveDrainGuard {
            controller: self,
            machine_id: machine_id.to_owned(),
        }
    }

    fn update_record_and_note_sleep(
        &self,
        machine_id: &str,
        peer_control: &dyn CloudPeerControlPort,
        mutate: impl FnOnce(&CloudMachineRecord) -> Result<CloudMachineRecord, CloudError>,
    ) -> Result<CloudMachineRecord, CloudError> {
        let updated = self.update_record(machine_id, mutate)?;
        if matches!(
            updated.state,
            CloudLifecycleState::Asleep | CloudLifecycleState::OutOfCredits
        ) {
            self.clear_link_flush_generation(&updated.machine_id);
            peer_control.note_machine_asleep(&updated);
        }
        Ok(updated)
    }

    pub(super) async fn complete_drain_sleep_locked(
        &self,
        draining: &CloudMachineRecord,
        action: IdleSleepAction,
        reason: codeterm_analytics::CloudSleepReason,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudReceipt, CloudError> {
        let provider_id = require_provider_machine_id(draining)?.to_owned();
        if let Err(error) = peer_control.enter_drain(draining).await {
            let _ = self.update_record(&draining.machine_id, |rec| {
                transition(rec, LifecycleEvent::DrainAbortedBusy, self.clock.now_ms())
            });
            return Err(error);
        }

        let request_id =
            self.begin_pending_request(&draining.machine_id, PendingProvisioningOp::Sleep)?;
        match provisioning.sleep(&provider_id, &request_id).await {
            Ok(sleep_mode) => {
                let now_ms = self.clock.now_ms();
                let asleep =
                    self.update_record_and_note_sleep(&draining.machine_id, peer_control, |rec| {
                        let mut next = transition(rec, LifecycleEvent::ProviderStopped, now_ms)?;
                        next.sleep_mode = sleep_mode;
                        next.pending_request = None;
                        Ok(next)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept { reason });
                let warning = (sleep_mode == CloudSleepMode::DeleteRecreate
                    || action == IdleSleepAction::DeleteForRecreate)
                    .then(|| {
                        "machine uses delete_recreate sleep mode; instance was imaged and deleted to stop billing and will recreate with a new IPv4 on next wake".into()
                    });
                Ok(asleep.receipt(now_ms, warning))
            }
            Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {
                let _ = peer_control.reopen_drain(draining).await;
                let now_ms = self.clock.now_ms();
                let asleep =
                    self.update_record_and_note_sleep(&draining.machine_id, peer_control, |rec| {
                        apply_draining_truth(rec, BackendDrainingTruth::Asleep, now_ms)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept { reason });
                Ok(asleep.receipt(now_ms, None))
            }
            Err(CloudError::CreditsExhausted | CloudError::LeaseRequired) => {
                let _ = peer_control.reopen_drain(draining).await;
                let now_ms = self.clock.now_ms();
                let exhausted =
                    self.update_record_and_note_sleep(&draining.machine_id, peer_control, |rec| {
                        apply_draining_truth(rec, BackendDrainingTruth::OutOfCredits, now_ms)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                    reason: codeterm_analytics::CloudSleepReason::Exhausted,
                });
                Ok(exhausted.receipt(now_ms, Some("Out of credits: top up to wake".into())))
            }
            Err(error) => {
                if error.is_uncertain_provisioning_outcome() {
                    self.mark_pending_unconfirmed(
                        &draining.machine_id,
                        PendingProvisioningOp::Sleep,
                        &error.to_string(),
                    );
                } else {
                    self.clear_pending_request(&draining.machine_id);
                }
                let _ = peer_control.reopen_drain(draining).await;
                let _ = self.update_record(&draining.machine_id, |rec| {
                    transition(rec, LifecycleEvent::DrainAbortedBusy, self.clock.now_ms())
                });
                Err(error)
            }
        }
    }

    pub(super) async fn sleep_for_exhaustion(
        &self,
        machine_id: &str,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudReceipt, CloudError> {
        let initial = self.find_record(machine_id)?;
        let drain_lock = self.machine_drain_lock(&initial.machine_id);
        let _drain_lock_guard = drain_lock.lock().await;
        let _active_drain = self.acquire_active_drain(&initial.machine_id);

        let record = self.find_record(&initial.machine_id)?;
        let provider_id = require_provider_machine_id(&record)?.to_owned();
        let _ = peer_control.enter_drain(&record).await;
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Sleep)?;
        match provisioning.sleep(&provider_id, &request_id).await {
            Ok(sleep_mode) => {
                let now_ms = self.clock.now_ms();
                let exhausted =
                    self.update_record_and_note_sleep(&record.machine_id, peer_control, |rec| {
                        let mut next = transition(rec, LifecycleEvent::CreditsExhausted, now_ms)?;
                        next.sleep_mode = sleep_mode;
                        next.pending_request = None;
                        Ok(next)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                    reason: codeterm_analytics::CloudSleepReason::Exhausted,
                });
                Ok(exhausted.receipt(now_ms, Some("Out of credits: top up to wake".into())))
            }
            Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {
                let now_ms = self.clock.now_ms();
                let exhausted =
                    self.update_record_and_note_sleep(&record.machine_id, peer_control, |rec| {
                        let mut next = transition(rec, LifecycleEvent::CreditsExhausted, now_ms)?;
                        next.pending_request = None;
                        Ok(next)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                    reason: codeterm_analytics::CloudSleepReason::Exhausted,
                });
                Ok(exhausted.receipt(now_ms, Some("Out of credits: top up to wake".into())))
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
                Err(error)
            }
        }
    }

    async fn sleep_for_ceiling(
        &self,
        machine_id: &str,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudReceipt, CloudError> {
        let initial = self.find_record(machine_id)?;
        let drain_lock = self.machine_drain_lock(&initial.machine_id);
        let _drain_lock_guard = drain_lock.lock().await;
        let _active_drain = self.acquire_active_drain(&initial.machine_id);

        let record = self.find_record(&initial.machine_id)?;
        let provider_id = require_provider_machine_id(&record)?.to_owned();
        let _ = peer_control.enter_drain(&record).await;
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Sleep)?;
        match provisioning.sleep(&provider_id, &request_id).await {
            Ok(sleep_mode) => {
                let now_ms = self.clock.now_ms();
                let asleep =
                    self.update_record_and_note_sleep(&record.machine_id, peer_control, |rec| {
                        let mut next = transition(rec, LifecycleEvent::ProviderStopped, now_ms)?;
                        next.sleep_mode = sleep_mode;
                        next.pending_request = None;
                        Ok(next)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                    reason: codeterm_analytics::CloudSleepReason::Idle,
                });
                Ok(asleep.receipt(
                    now_ms,
                    Some(
                        "Cloud machine slept after exceeding max awake time without owner interaction"
                            .into(),
                    ),
                ))
            }
            Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {
                let now_ms = self.clock.now_ms();
                let asleep =
                    self.update_record_and_note_sleep(&record.machine_id, peer_control, |rec| {
                        let mut next = transition(rec, LifecycleEvent::ProviderStopped, now_ms)?;
                        next.pending_request = None;
                        Ok(next)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                    reason: codeterm_analytics::CloudSleepReason::Idle,
                });
                Ok(asleep.receipt(now_ms, None))
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
                let _ = peer_control.reopen_drain(&record).await;
                Err(error)
            }
        }
    }

    async fn probe_draining_backend_truth(
        &self,
        record: &CloudMachineRecord,
        provisioning: &dyn CloudProvisioningPort,
    ) -> Result<ProbedDrainingTruth, CloudError> {
        let mut usage_exhausted = false;
        if let Ok(summary) = self.usage(provisioning).await {
            if let Some(truth) = infer_backend_truth_from_usage(record, &summary) {
                return Ok(ProbedDrainingTruth {
                    truth,
                    renewed_lease: None,
                    sleep_mode: None,
                    already_stopped_on_backend: true,
                });
            }
            usage_exhausted = usage_reports_credits_exhausted(&summary);
        }

        let provider_id = require_provider_machine_id(record)?.to_owned();
        if let Some(lease_id) = record
            .lease_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            let request_id = mint_request_id();
            return match provisioning
                .renew_lease(&provider_id, lease_id, &request_id)
                .await
            {
                Ok(renewed) => Ok(ProbedDrainingTruth {
                    truth: BackendDrainingTruth::Awake,
                    renewed_lease: Some(renewed),
                    sleep_mode: None,
                    already_stopped_on_backend: false,
                }),
                Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {
                    let truth = if record.exhaustion_pending || usage_exhausted {
                        BackendDrainingTruth::OutOfCredits
                    } else {
                        BackendDrainingTruth::Asleep
                    };
                    Ok(ProbedDrainingTruth {
                        truth,
                        renewed_lease: None,
                        sleep_mode: None,
                        already_stopped_on_backend: true,
                    })
                }
                Err(CloudError::CreditsExhausted | CloudError::LeaseRequired) => {
                    Ok(ProbedDrainingTruth {
                        truth: BackendDrainingTruth::OutOfCredits,
                        renewed_lease: None,
                        sleep_mode: None,
                        already_stopped_on_backend: false,
                    })
                }
                Err(error) => Err(error),
            };
        }

        // No persisted lease_id to renew: settle backend via idempotent sleep.
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Sleep)?;
        match provisioning.sleep(&provider_id, &request_id).await {
            Ok(sleep_mode) => {
                let truth = if record.exhaustion_pending || usage_exhausted {
                    BackendDrainingTruth::OutOfCredits
                } else {
                    BackendDrainingTruth::Asleep
                };
                Ok(ProbedDrainingTruth {
                    truth,
                    renewed_lease: None,
                    sleep_mode: Some(sleep_mode),
                    already_stopped_on_backend: true,
                })
            }
            Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {
                let truth = if record.exhaustion_pending || usage_exhausted {
                    BackendDrainingTruth::OutOfCredits
                } else {
                    BackendDrainingTruth::Asleep
                };
                Ok(ProbedDrainingTruth {
                    truth,
                    renewed_lease: None,
                    sleep_mode: None,
                    already_stopped_on_backend: true,
                })
            }
            Err(CloudError::CreditsExhausted | CloudError::LeaseRequired) => {
                Ok(ProbedDrainingTruth {
                    truth: BackendDrainingTruth::OutOfCredits,
                    renewed_lease: None,
                    sleep_mode: None,
                    already_stopped_on_backend: true,
                })
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
                Err(error)
            }
        }
    }

    pub(super) async fn reconcile_draining_locked(
        &self,
        machine_id: &str,
        intent: DrainingReconcileIntent,
        delete_on_stop_billed: bool,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudMachineRecord, CloudError> {
        let record = self.find_record(machine_id)?;
        if record.state != CloudLifecycleState::Draining {
            return Ok(record);
        }

        let probed = self
            .probe_draining_backend_truth(&record, provisioning)
            .await?;
        let policy = IdlePolicy {
            idle_window_ms: 0,
            lease_renew_lead_ms: DEFAULT_LEASE_RENEW_LEAD_MS,
            max_awake_without_owner_ms: None,
            sleep_mode: record.sleep_mode,
            delete_on_stop_billed,
        };
        let plan =
            plan_draining_recovery(record.state, probed.truth, intent, policy.sleep_action());

        match plan {
            DrainingRecoveryPlan::NotDraining => Ok(record),
            DrainingRecoveryPlan::FinishAsleep => {
                let _ = peer_control.reopen_drain(&record).await;
                let now_ms = self.clock.now_ms();
                let asleep =
                    self.update_record_and_note_sleep(machine_id, peer_control, |rec| {
                        let mut next =
                            apply_draining_truth(rec, BackendDrainingTruth::Asleep, now_ms)?;
                        if let Some(sleep_mode) = probed.sleep_mode {
                            next.sleep_mode = sleep_mode;
                        }
                        Ok(next)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                    reason: codeterm_analytics::CloudSleepReason::Idle,
                });
                Ok(asleep)
            }
            DrainingRecoveryPlan::FinishOutOfCredits => {
                let mut sleep_mode = probed.sleep_mode;
                if !probed.already_stopped_on_backend {
                    let provider_id = require_provider_machine_id(&record)?.to_owned();
                    let request_id =
                        self.begin_pending_request(machine_id, PendingProvisioningOp::Sleep)?;
                    match provisioning.sleep(&provider_id, &request_id).await {
                        Ok(mode) => sleep_mode = Some(mode),
                        Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {}
                        Err(error) => {
                            if error.is_uncertain_provisioning_outcome() {
                                self.mark_pending_unconfirmed(
                                    machine_id,
                                    PendingProvisioningOp::Sleep,
                                    &error.to_string(),
                                );
                            } else {
                                self.clear_pending_request(machine_id);
                            }
                            return Err(error);
                        }
                    }
                }
                let _ = peer_control.reopen_drain(&record).await;
                let now_ms = self.clock.now_ms();
                let exhausted =
                    self.update_record_and_note_sleep(machine_id, peer_control, |rec| {
                        let mut next =
                            apply_draining_truth(rec, BackendDrainingTruth::OutOfCredits, now_ms)?;
                        if let Some(mode) = sleep_mode {
                            next.sleep_mode = mode;
                        }
                        Ok(next)
                    })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                    reason: codeterm_analytics::CloudSleepReason::Exhausted,
                });
                Ok(exhausted)
            }
            DrainingRecoveryPlan::ReopenAwake => {
                let _ = peer_control.reopen_drain(&record).await;
                let now_ms = self.clock.now_ms();
                self.update_record(machine_id, |rec| {
                    let mut next = apply_draining_truth(rec, BackendDrainingTruth::Awake, now_ms)?;
                    if let Some(renewed) = &probed.renewed_lease {
                        next.lease_id = Some(renewed.lease_id.clone());
                        next.lease_expires_at_ms = Some(renewed.lease_expires_at_ms);
                    }
                    Ok(next)
                })
            }
            DrainingRecoveryPlan::ResumeSleep => {
                // Persist any renewed lease before resuming the two-phase drain/sleep.
                let refreshed = if let Some(renewed) = &probed.renewed_lease {
                    self.update_record(machine_id, |rec| {
                        let mut next = rec.clone();
                        next.lease_id = Some(renewed.lease_id.clone());
                        next.lease_expires_at_ms = Some(renewed.lease_expires_at_ms);
                        next.exhaustion_pending = false;
                        Ok(next)
                    })?
                } else {
                    record
                };

                if peer_control.enter_drain(&refreshed).await.is_err() {
                    let _ = peer_control.reopen_drain(&refreshed).await;
                    let now_ms = self.clock.now_ms();
                    return self.update_record(machine_id, |rec| {
                        apply_draining_truth(rec, BackendDrainingTruth::Awake, now_ms)
                    });
                }

                let provider_id = require_provider_machine_id(&refreshed)?.to_owned();
                let request_id =
                    self.begin_pending_request(machine_id, PendingProvisioningOp::Sleep)?;
                match provisioning.sleep(&provider_id, &request_id).await {
                    Ok(sleep_mode) => {
                        let now_ms = self.clock.now_ms();
                        let asleep =
                            self.update_record_and_note_sleep(machine_id, peer_control, |rec| {
                                let mut next = apply_draining_truth(
                                    rec,
                                    BackendDrainingTruth::Asleep,
                                    now_ms,
                                )?;
                                next.sleep_mode = sleep_mode;
                                Ok(next)
                            })?;
                        self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                            reason: codeterm_analytics::CloudSleepReason::Idle,
                        });
                        Ok(asleep)
                    }
                    Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {
                        let _ = peer_control.reopen_drain(&refreshed).await;
                        let now_ms = self.clock.now_ms();
                        let asleep =
                            self.update_record_and_note_sleep(machine_id, peer_control, |rec| {
                                apply_draining_truth(rec, BackendDrainingTruth::Asleep, now_ms)
                            })?;
                        self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                            reason: codeterm_analytics::CloudSleepReason::Idle,
                        });
                        Ok(asleep)
                    }
                    Err(CloudError::CreditsExhausted | CloudError::LeaseRequired) => {
                        let _ = peer_control.reopen_drain(&refreshed).await;
                        let now_ms = self.clock.now_ms();
                        let exhausted =
                            self.update_record_and_note_sleep(machine_id, peer_control, |rec| {
                                apply_draining_truth(
                                    rec,
                                    BackendDrainingTruth::OutOfCredits,
                                    now_ms,
                                )
                            })?;
                        self.record_product_event(codeterm_analytics::Event::CloudMachineSlept {
                            reason: codeterm_analytics::CloudSleepReason::Exhausted,
                        });
                        Ok(exhausted)
                    }
                    Err(error) => {
                        log::warn!(
                            "[cloud] resume sleep during Draining recovery failed for {machine_id}: {error}; returning to Awake"
                        );
                        if error.is_uncertain_provisioning_outcome() {
                            self.mark_pending_unconfirmed(
                                machine_id,
                                PendingProvisioningOp::Sleep,
                                &error.to_string(),
                            );
                        } else {
                            self.clear_pending_request(machine_id);
                        }
                        let _ = peer_control.reopen_drain(&refreshed).await;
                        let now_ms = self.clock.now_ms();
                        self.update_record(machine_id, |rec| {
                            let mut next =
                                transition(rec, LifecycleEvent::DrainAbortedBusy, now_ms)?;
                            if next.last_awake_at_ms.is_none() {
                                next.last_awake_at_ms = Some(now_ms);
                            }
                            Ok(next)
                        })
                    }
                }
            }
        }
    }

    /// Reconciles a single stale `Draining` machine record against backend truth, serialized
    /// against any in-flight drain or sleep on the same machine.
    pub async fn reconcile_draining(
        &self,
        machine_id: &str,
        intent: DrainingReconcileIntent,
        delete_on_stop_billed: bool,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudMachineRecord, CloudError> {
        let initial = self.find_record(machine_id)?;
        let drain_lock = self.machine_drain_lock(&initial.machine_id);
        let _drain_lock_guard = drain_lock.lock().await;
        let _active_drain = self.acquire_active_drain(&initial.machine_id);
        self.reconcile_draining_locked(
            &initial.machine_id,
            intent,
            delete_on_stop_billed,
            provisioning,
            peer_control,
        )
        .await
    }

    /// Reconciles every persisted `Draining` record that currently has no live drain owner.
    pub async fn reconcile_draining_machines(
        &self,
        delete_on_stop_billed: bool,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Vec<Result<CloudMachineRecord, CloudError>> {
        let draining_ids: Vec<String> = self
            .list_records()
            .into_iter()
            .filter(|rec| {
                rec.state == CloudLifecycleState::Draining
                    && !self.has_active_drain(&rec.machine_id)
            })
            .map(|rec| rec.machine_id)
            .collect();
        let mut outcomes = Vec::with_capacity(draining_ids.len());
        for machine_id in draining_ids {
            outcomes.push(
                self.reconcile_draining(
                    &machine_id,
                    DrainingReconcileIntent::FinishDrainOrRecover,
                    delete_on_stop_billed,
                    provisioning,
                    peer_control,
                )
                .await,
            );
        }
        outcomes
    }

    /// Records an owner-originated interaction with a cloud machine, extending any
    /// configured `cloud_max_awake_without_owner_minutes` ceiling.
    pub fn record_owner_interaction(&self, selector: &str) {
        let Some(record) = self.find_by_peer(selector) else {
            return;
        };
        let now_ms = self.clock.now_ms();
        let _ = self.update_record(&record.machine_id, |rec| {
            let mut next = rec.clone();
            next.last_owner_interaction_at_ms = Some(now_ms);
            Ok(next)
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn evaluate_idle_tick(
        &self,
        machine_id: &str,
        busy: PeerBusySnapshot,
        idle_window_ms: u64,
        lease_renew_lead_ms: u64,
        max_awake_without_owner_ms: Option<u64>,
        delete_on_stop_billed: bool,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<IdleDecision, CloudError> {
        let record = self.find_record(machine_id)?;
        let now_ms = self.clock.now_ms();
        let last_owner_interaction_at_ms = record
            .last_owner_interaction_at_ms
            .or(record.last_awake_at_ms);
        let ceiling_at = max_awake_without_owner_ms
            .filter(|&limit| limit > 0)
            .and_then(|limit| last_owner_interaction_at_ms.map(|last| last.saturating_add(limit)));
        let decision = evaluate_idle_and_renewal(IdleEvalInput {
            state: record.state,
            busy,
            now_ms,
            idle_since_ms: record.idle_since_ms,
            last_owner_interaction_at_ms,
            lease_expires_at_ms: record.lease_expires_at_ms,
            exhaustion_pending: record.exhaustion_pending,
            policy: IdlePolicy {
                idle_window_ms,
                lease_renew_lead_ms,
                max_awake_without_owner_ms,
                sleep_mode: record.sleep_mode,
                delete_on_stop_billed,
            },
        });
        match decision {
            IdleDecision::RenewLeaseNow {
                lease_expires_at_ms,
                idle_since_ms,
            } => {
                let provider_id = require_provider_machine_id(&record)?.to_owned();
                let lease_id = record.lease_id.clone().unwrap_or_default();
                let request_id = mint_request_id();
                match provisioning
                    .renew_lease(&provider_id, &lease_id, &request_id)
                    .await
                {
                    Ok(renewed) => {
                        let _ = self.update_record(machine_id, |rec| {
                            let mut next = rec.clone();
                            next.lease_id = Some(renewed.lease_id.clone());
                            next.lease_expires_at_ms = Some(renewed.lease_expires_at_ms);
                            next.idle_since_ms = idle_since_ms;
                            next.exhaustion_pending = false;
                            Ok(next)
                        });
                        let mut next_at = renewed
                            .lease_expires_at_ms
                            .saturating_sub(lease_renew_lead_ms);
                        if let Some(idle_since) = idle_since_ms {
                            next_at = next_at.min(idle_since.saturating_add(idle_window_ms));
                        }
                        if let Some(ceiling) = ceiling_at {
                            next_at = next_at.min(ceiling);
                        }
                        Ok(IdleDecision::ScheduleAt {
                            at_ms: next_at,
                            idle_since_ms,
                        })
                    }
                    Err(CloudError::CreditsExhausted | CloudError::LeaseRequired) => {
                        let _ = self.update_record(machine_id, |rec| {
                            let mut next = rec.clone();
                            next.exhaustion_pending = true;
                            next.idle_since_ms = idle_since_ms;
                            Ok(next)
                        });
                        if now_ms >= lease_expires_at_ms {
                            let _ = self
                                .sleep_for_exhaustion(machine_id, provisioning, peer_control)
                                .await?;
                            Ok(IdleDecision::BeginTwoPhaseDrain {
                                action: IdleSleepAction::DeleteForRecreate,
                                out_of_credits: true,
                                ceiling_exceeded: false,
                            })
                        } else {
                            // Finish the current prepaid lease, then sleep at its expiry (or sooner if idle sleep arrives first).
                            let mut next_at = lease_expires_at_ms;
                            if let Some(idle_since) = idle_since_ms {
                                next_at = next_at.min(idle_since.saturating_add(idle_window_ms));
                            }
                            if let Some(ceiling) = ceiling_at {
                                next_at = next_at.min(ceiling);
                            }
                            Ok(IdleDecision::ScheduleAt {
                                at_ms: next_at,
                                idle_since_ms,
                            })
                        }
                    }
                    Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_))
                        if now_ms >= lease_expires_at_ms =>
                    {
                        // Backend reconciliation already slept this machine after lease expiry while home was offline.
                        if self
                            .update_record_and_note_sleep(machine_id, peer_control, |rec| {
                                let mut next =
                                    transition(rec, LifecycleEvent::ProviderStopped, now_ms)?;
                                next.lease_id = None;
                                next.lease_expires_at_ms = None;
                                next.pending_request = None;
                                Ok(next)
                            })
                            .is_ok()
                        {
                            self.record_product_event(
                                codeterm_analytics::Event::CloudMachineSlept {
                                    reason: codeterm_analytics::CloudSleepReason::Idle,
                                },
                            );
                        }
                        Ok(IdleDecision::BeginTwoPhaseDrain {
                            action: IdleSleepAction::StopProvider,
                            out_of_credits: false,
                            ceiling_exceeded: false,
                        })
                    }
                    Err(error) => Err(error),
                }
            }
            IdleDecision::BeginTwoPhaseDrain {
                out_of_credits: true,
                ..
            } => {
                let _ = self
                    .sleep_for_exhaustion(machine_id, provisioning, peer_control)
                    .await?;
                Ok(decision)
            }
            IdleDecision::BeginTwoPhaseDrain {
                out_of_credits: false,
                ceiling_exceeded: true,
                ..
            } => {
                let _ = self
                    .sleep_for_ceiling(machine_id, provisioning, peer_control)
                    .await?;
                Ok(decision)
            }
            IdleDecision::BeginTwoPhaseDrain {
                out_of_credits: false,
                ceiling_exceeded: false,
                ..
            } => {
                let _ = self
                    .sleep_with_reason(
                        machine_id,
                        provisioning,
                        peer_control,
                        delete_on_stop_billed,
                        codeterm_analytics::CloudSleepReason::Idle,
                    )
                    .await?;
                Ok(decision)
            }
            IdleDecision::ScheduleAt { idle_since_ms, .. } => {
                if record.idle_since_ms != idle_since_ms {
                    let _ = self.update_record(machine_id, |rec| {
                        let mut next = rec.clone();
                        next.idle_since_ms = idle_since_ms;
                        Ok(next)
                    });
                }
                Ok(decision)
            }
            IdleDecision::None => Ok(decision),
        }
    }

    /// Reconciles lease state with the backend when the cloud peer is unreachable over mesh
    /// (`read_idle` failed), so a backend-slept or credit-exhausted peer never leaves home stuck
    /// `Awake` forever. Contacts the backend only once inside/past the renew window (or enforces
    /// any configured owner-inactivity ceiling).
    #[allow(clippy::too_many_arguments)]
    pub async fn reconcile_unreachable_lease(
        &self,
        machine_id: &str,
        lease_renew_lead_ms: u64,
        max_awake_without_owner_ms: Option<u64>,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<IdleDecision, CloudError> {
        let record = self.find_record(machine_id)?;
        if record.state != CloudLifecycleState::Awake {
            return Ok(IdleDecision::None);
        }
        let now_ms = self.clock.now_ms();
        let last_owner_interaction_at_ms = record
            .last_owner_interaction_at_ms
            .or(record.last_awake_at_ms);
        let ceiling_at = max_awake_without_owner_ms
            .filter(|&limit| limit > 0)
            .and_then(|limit| last_owner_interaction_at_ms.map(|last| last.saturating_add(limit)));

        if record.exhaustion_pending {
            let expires_at = record.lease_expires_at_ms.unwrap_or(now_ms);
            if now_ms >= expires_at || ceiling_at.is_some_and(|c| now_ms >= c) {
                let _ = self
                    .sleep_for_exhaustion(machine_id, provisioning, peer_control)
                    .await?;
                return Ok(IdleDecision::BeginTwoPhaseDrain {
                    action: IdleSleepAction::DeleteForRecreate,
                    out_of_credits: true,
                    ceiling_exceeded: false,
                });
            }
            return Ok(IdleDecision::ScheduleAt {
                at_ms: ceiling_at.map_or(expires_at, |c| expires_at.min(c)),
                idle_since_ms: record.idle_since_ms,
            });
        }

        if ceiling_at.is_some_and(|c| now_ms >= c) {
            let _ = self
                .sleep_for_ceiling(machine_id, provisioning, peer_control)
                .await?;
            return Ok(IdleDecision::BeginTwoPhaseDrain {
                action: IdleSleepAction::DeleteForRecreate,
                out_of_credits: false,
                ceiling_exceeded: true,
            });
        }

        let Some(expires_at) = record.lease_expires_at_ms else {
            return Ok(match ceiling_at {
                Some(at_ms) => IdleDecision::ScheduleAt {
                    at_ms,
                    idle_since_ms: record.idle_since_ms,
                },
                None => IdleDecision::None,
            });
        };
        let renew_at = expires_at.saturating_sub(lease_renew_lead_ms);
        if now_ms < renew_at {
            return Ok(IdleDecision::ScheduleAt {
                at_ms: ceiling_at.map_or(renew_at, |c| renew_at.min(c)),
                idle_since_ms: record.idle_since_ms,
            });
        }

        let provider_id = require_provider_machine_id(&record)?.to_owned();
        let lease_id = record.lease_id.clone().unwrap_or_default();
        let request_id = mint_request_id();
        match provisioning
            .renew_lease(&provider_id, &lease_id, &request_id)
            .await
        {
            Ok(renewed) => {
                let _ = self.update_record(machine_id, |rec| {
                    let mut next = rec.clone();
                    next.lease_id = Some(renewed.lease_id.clone());
                    next.lease_expires_at_ms = Some(renewed.lease_expires_at_ms);
                    next.exhaustion_pending = false;
                    Ok(next)
                });
                let next_renew = renewed
                    .lease_expires_at_ms
                    .saturating_sub(lease_renew_lead_ms);
                Ok(IdleDecision::ScheduleAt {
                    at_ms: ceiling_at.map_or(next_renew, |c| next_renew.min(c)),
                    idle_since_ms: record.idle_since_ms,
                })
            }
            Err(CloudError::CreditsExhausted | CloudError::LeaseRequired) => {
                let _ = self.update_record(machine_id, |rec| {
                    let mut next = rec.clone();
                    next.exhaustion_pending = true;
                    Ok(next)
                });
                if now_ms >= expires_at {
                    let _ = self
                        .sleep_for_exhaustion(machine_id, provisioning, peer_control)
                        .await?;
                    Ok(IdleDecision::BeginTwoPhaseDrain {
                        action: IdleSleepAction::DeleteForRecreate,
                        out_of_credits: true,
                        ceiling_exceeded: false,
                    })
                } else {
                    Ok(IdleDecision::ScheduleAt {
                        at_ms: ceiling_at.map_or(expires_at, |c| expires_at.min(c)),
                        idle_since_ms: record.idle_since_ms,
                    })
                }
            }
            Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {
                // Unreachable peer + backend conflict/not-found means the machine is already stopped/deleted.
                let _ = self.update_record_and_note_sleep(machine_id, peer_control, |rec| {
                    let mut next = transition(rec, LifecycleEvent::ProviderStopped, now_ms)?;
                    next.lease_id = None;
                    next.lease_expires_at_ms = None;
                    next.pending_request = None;
                    Ok(next)
                });
                Ok(IdleDecision::BeginTwoPhaseDrain {
                    action: IdleSleepAction::StopProvider,
                    out_of_credits: false,
                    ceiling_exceeded: false,
                })
            }
            Err(error) => Err(error),
        }
    }
}
