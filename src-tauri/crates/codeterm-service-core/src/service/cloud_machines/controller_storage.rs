use zeroize::Zeroizing;

use super::controller::{mint_request_id, CloudMachineController};
use super::controller_reconcile::require_provider_machine_id;
use super::ports::{
    CloudEnrollmentPort, CloudPeerControlPort, CloudProvisioningPort, EnrollmentOutcome,
};
use super::state::{transition, LifecycleEvent};
use super::storage::{
    validate_volume_upgrade_gb, CloudColdWakeParams, CloudStorageTier, CloudVolumeResizeReply,
    AUX_KEY_ALLOCATED_GB, AUX_KEY_PURGE_AT, AUX_KEY_RECOVERABLE_ARCHIVE_ID, AUX_KEY_STORAGE_TIER,
};
use super::types::{
    CloudError, CloudLifecycleState, CloudMachineRecord, CloudReceipt, PendingProvisioningOp,
};

impl CloudMachineController {
    /// Cold wake or recoverable recreate onto a fresh volume: prepares a replacement enrollment binding,
    /// provisions a fresh volume from the verified owner-bound archive, enrolls the replacement installation,
    /// and preserves the archive reference if provisioning or enrollment fails.
    pub async fn recreate_from_archive(
        &self,
        selector: &str,
        archive_id: Option<&str>,
        provisioning: &dyn CloudProvisioningPort,
        enrollment: &dyn CloudEnrollmentPort,
        peer_control: &dyn CloudPeerControlPort,
        deadline_ms: u64,
    ) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record_including_deleted(selector)?;
        let record = if record.state == CloudLifecycleState::OutOfCredits {
            self.refresh_credits_for_wake(&record.machine_id, provisioning)
                .await?
        } else {
            record
        };
        let resolved_archive_id = archive_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                record
                    .ownership
                    .auxiliary_resource_ids
                    .get(AUX_KEY_RECOVERABLE_ARCHIVE_ID)
                    .cloned()
            });
        let provider_machine_id = require_provider_machine_id(&record)?.to_owned();
        let binding = enrollment.prepare_binding().await?;
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Wake)?;
        let cold_params = CloudColdWakeParams {
            controller_key: binding.controller_key.clone(),
            creation_psk: Zeroizing::new((*binding.provider_creation_nonce).clone()),
            archive_id: resolved_archive_id,
        };
        let provisioned = provisioning
            .wake_cold(&provider_machine_id, &request_id, &cold_params)
            .await;
        drop(cold_params);
        let provisioned = match provisioned {
            Ok(machine) => machine,
            Err(error) => {
                drop(binding);
                if error.is_uncertain_provisioning_outcome() {
                    self.mark_pending_unconfirmed(
                        &record.machine_id,
                        PendingProvisioningOp::Wake,
                        &error.to_string(),
                    );
                } else {
                    self.clear_pending_request(&record.machine_id);
                }
                return Err(error);
            }
        };
        let enrolling = self.update_record(&record.machine_id, |rec| {
            let mut next = rec.clone();
            next.state = CloudLifecycleState::Enrolling;
            next.provider = provisioned.provider.clone();
            next.size = provisioned.size.clone();
            next.sleep_mode = provisioned.sleep_mode;
            next.rate_per_hour = Some(provisioned.rate.clone());
            next.endpoint = Some(provisioned.endpoint.clone());
            next.lease_id = Some(provisioned.lease_id.clone());
            next.lease_expires_at_ms = Some(provisioned.lease_expires_at_ms);
            next.pending_request = None;
            next.installation_id = None;
            next.peer_instance_id = None;
            next.apply_unconfirmed = false;
            next.last_error = None;
            Ok(next)
        })?;
        let route = provisioned.endpoint.clone();
        let enroll_result = enrollment
            .enroll(&route, &binding, &provisioned.provider, &provisioned.size)
            .await;
        drop(binding);
        match enroll_result {
            Ok(EnrollmentOutcome::Enrolled {
                installation_id,
                peer_instance_id,
            }) => {
                let enrolled = self.update_record(&enrolling.machine_id, |rec| {
                    let mut next = rec.clone();
                    next.installation_id = Some(installation_id.clone());
                    if peer_instance_id.is_some() {
                        next.peer_instance_id = peer_instance_id.clone();
                    }
                    Ok(next)
                })?;
                peer_control
                    .update_endpoint(&enrolled, &provisioned.endpoint)
                    .await?;
                peer_control
                    .publish_direct_endpoint(&enrolled, &provisioned.endpoint)
                    .await?;
                let ready_deadline = self.clock.now_ms().saturating_add(deadline_ms);
                peer_control
                    .await_peer_ready(&enrolled, ready_deadline)
                    .await?;
                let awake_now = self.clock.now_ms();
                let awake = self.update_record(&enrolled.machine_id, |rec| {
                    let mut next = transition(
                        rec,
                        LifecycleEvent::EnrolledReady {
                            installation_id: installation_id.clone(),
                            peer_instance_id: peer_instance_id.clone(),
                        },
                        awake_now,
                    )?;
                    next.ownership.auxiliary_resource_ids.insert(
                        AUX_KEY_STORAGE_TIER.into(),
                        CloudStorageTier::Hot.as_str().into(),
                    );
                    Ok(next)
                })?;
                self.record_product_event(codeterm_analytics::Event::CloudMachineWoken {});
                Ok(awake.receipt(awake_now, None))
            }
            Ok(EnrollmentOutcome::ApplyUnconfirmed { installation_id }) => {
                let _ = self.update_record(&enrolling.machine_id, |rec| {
                    transition(
                        rec,
                        LifecycleEvent::EnrollmentApplyUnconfirmed {
                            installation_id: installation_id.clone(),
                        },
                        self.clock.now_ms(),
                    )
                });
                Err(CloudError::ApplyUnconfirmed {
                    machine_id: enrolling.machine_id,
                    installation_id,
                })
            }
            Err(error) => {
                let _ = self.update_record(&enrolling.machine_id, |rec| {
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

    /// Owner-confirmed live volume size increase (grow-only, strictly refuses without `confirm: true`).
    pub async fn resize_volume(
        &self,
        selector: &str,
        new_size_gb: u32,
        confirm: bool,
        provisioning: &dyn CloudProvisioningPort,
    ) -> Result<CloudVolumeResizeReply, CloudError> {
        let record = self.find_record(selector)?;
        let current_gb = record
            .ownership
            .auxiliary_resource_ids
            .get(AUX_KEY_ALLOCATED_GB)
            .and_then(|v| v.parse::<u32>().ok());
        if let Err(reason) = validate_volume_upgrade_gb(current_gb, new_size_gb) {
            return Err(CloudError::ProviderRefused(reason));
        }
        if !confirm {
            return Err(CloudError::VolumeUpgradeConfirmationRequired {
                machine_id: record.machine_id.clone(),
                new_size_gb,
                warning: format!(
                    "Expanding cloud machine '{}' ({}) volume to {} GB increases billed hot storage capacity and cannot be shrunk. Re-run with --confirm to proceed.",
                    record.name, record.machine_id, new_size_gb
                ),
            });
        }
        let provider_machine_id = require_provider_machine_id(&record)?.to_owned();
        let request_id = mint_request_id();
        let reply = provisioning
            .resize_volume(&provider_machine_id, new_size_gb, &request_id)
            .await?;
        let _ = self.update_record(&record.machine_id, |rec| {
            let mut next = rec.clone();
            next.ownership
                .auxiliary_resource_ids
                .insert(AUX_KEY_ALLOCATED_GB.into(), reply.allocated_gb.to_string());
            Ok(next)
        });
        Ok(reply)
    }

    /// Owner-triggered immediate hot->cold archive: drains and sleeps an awake idle machine first
    /// (refusing with `MachineBusy` while work runs), then invokes `archiveManagedCloudMachineNow`
    /// through the verified eviction chain and updates local storage-tier metadata on success.
    pub async fn archive_now(
        &self,
        selector: &str,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<CloudReceipt, CloudError> {
        let initial = self.find_record(selector)?;
        if matches!(
            initial.state,
            CloudLifecycleState::Awake | CloudLifecycleState::Draining
        ) {
            self.sleep(&initial.machine_id, provisioning, peer_control, true)
                .await?;
        }
        let record = self.find_record(&initial.machine_id)?;
        if !matches!(
            record.state,
            CloudLifecycleState::Asleep | CloudLifecycleState::OutOfCredits
        ) {
            return Err(CloudError::InvalidTransition {
                from: record.state,
                event: "archive_now",
            });
        }
        let provider_machine_id = require_provider_machine_id(&record)?.to_owned();
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Archive)?;
        let archived = match provisioning
            .archive_now(&provider_machine_id, &request_id)
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                if error.is_uncertain_provisioning_outcome() {
                    self.mark_pending_unconfirmed(
                        &record.machine_id,
                        PendingProvisioningOp::Archive,
                        &error.to_string(),
                    );
                } else {
                    self.clear_pending_request(&record.machine_id);
                }
                return Err(error);
            }
        };
        let now_ms = self.clock.now_ms();
        let updated = self.update_record(&record.machine_id, |rec| {
            let mut next = rec.clone();
            next.state = if archived.state == "out_of_credits" {
                CloudLifecycleState::OutOfCredits
            } else {
                CloudLifecycleState::Asleep
            };
            next.endpoint = None;
            next.lease_id = None;
            next.lease_expires_at_ms = None;
            next.pending_request = None;
            next.last_error = None;
            next.ownership.auxiliary_resource_ids.insert(
                AUX_KEY_STORAGE_TIER.into(),
                CloudStorageTier::Cold.as_str().into(),
            );
            next.ownership.auxiliary_resource_ids.insert(
                AUX_KEY_RECOVERABLE_ARCHIVE_ID.into(),
                archived.archive_id.clone(),
            );
            next.ownership
                .auxiliary_resource_ids
                .insert(AUX_KEY_PURGE_AT.into(), archived.purge_at.to_string());
            next.ownership
                .auxiliary_resource_ids
                .insert(AUX_KEY_ALLOCATED_GB.into(), "0".into());
            Ok(next)
        })?;
        Ok(updated.receipt(now_ms, None))
    }

    pub(super) fn find_record_including_deleted(
        &self,
        selector: &str,
    ) -> Result<CloudMachineRecord, CloudError> {
        self.find_any_record(selector)
    }
}
