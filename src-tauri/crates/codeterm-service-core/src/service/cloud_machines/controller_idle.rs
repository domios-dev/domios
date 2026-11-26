use super::controller::{mint_request_id, CloudMachineController};
use super::idle_drain::{
    evaluate_idle_and_renewal, IdleDecision, IdleEvalInput, IdlePolicy, IdleSleepAction,
    PeerBusySnapshot,
};
use super::ports::{CloudPeerControlPort, CloudProvisioningPort};
use super::state::{transition, LifecycleEvent};
use super::types::{CloudError, CloudLifecycleState, CloudReceipt, PendingProvisioningOp};

impl CloudMachineController {
    pub(super) async fn sleep_for_exhaustion(
        &self,
        machine_id: &str,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(machine_id)?;
        let _ = peer_control.enter_drain(&record).await;
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Sleep)?;
        match provisioning
            .sleep(record.backend_machine_id(), &request_id)
            .await
        {
            Ok(sleep_mode) => {
                let now_ms = self.clock.now_ms();
                let exhausted = self.update_record(&record.machine_id, |rec| {
                    let mut next = transition(rec, LifecycleEvent::CreditsExhausted, now_ms)?;
                    next.sleep_mode = sleep_mode;
                    next.pending_request = None;
                    Ok(next)
                })?;
                Ok(exhausted.receipt(now_ms, Some("Out of credits: top up to wake".into())))
            }
            Err(CloudError::Conflict(_) | CloudError::MachineNotFound(_)) => {
                let now_ms = self.clock.now_ms();
                let exhausted = self.update_record(&record.machine_id, |rec| {
                    let mut next = transition(rec, LifecycleEvent::CreditsExhausted, now_ms)?;
                    next.pending_request = None;
                    Ok(next)
                })?;
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
        let record = self.find_record(machine_id)?;
        let _ = peer_control.enter_drain(&record).await;
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Sleep)?;
        match provisioning
            .sleep(record.backend_machine_id(), &request_id)
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
                let asleep = self.update_record(&record.machine_id, |rec| {
                    let mut next = transition(rec, LifecycleEvent::ProviderStopped, now_ms)?;
                    next.pending_request = None;
                    Ok(next)
                })?;
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
                let lease_id = record.lease_id.clone().unwrap_or_default();
                let request_id = mint_request_id();
                match provisioning
                    .renew_lease(record.backend_machine_id(), &lease_id, &request_id)
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
                        let _ = self.update_record(machine_id, |rec| {
                            let mut next =
                                transition(rec, LifecycleEvent::ProviderStopped, now_ms)?;
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
                    .sleep(
                        machine_id,
                        provisioning,
                        peer_control,
                        delete_on_stop_billed,
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

        let lease_id = record.lease_id.clone().unwrap_or_default();
        let request_id = mint_request_id();
        match provisioning
            .renew_lease(record.backend_machine_id(), &lease_id, &request_id)
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
                let _ = self.update_record(machine_id, |rec| {
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
