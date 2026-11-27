use super::controller::CloudMachineController;
use super::ports::CloudProvisioningPort;
use super::reconcile::{
    apply_local_record_adoption, apply_local_record_delete, plan_local_record_reconcile,
    BackendMachineEntry, LocalReconcilePolicy, LocalRecordReconcileAction,
    LocalRecordReconcileIntent,
};
use super::state::{transition, LifecycleEvent};
use super::storage::{
    CloudDeleteMode, CloudStorageTier, AUX_KEY_PURGE_AT, AUX_KEY_RECOVERABLE_ARCHIVE_ID,
    AUX_KEY_STORAGE_TIER,
};
use super::types::{CloudError, CloudMachineRecord, CloudReceipt, PendingProvisioningOp};

#[derive(Clone, Debug, PartialEq)]
pub enum LocalRecordReconcileOutcome {
    AlreadyBound(CloudMachineRecord),
    Adopted(CloudMachineRecord),
    DeletedLocal(CloudMachineRecord),
    KeptUncertain,
    Ambiguous(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum DeleteResolution {
    DeletedLocal(CloudMachineRecord),
    Backend {
        record: CloudMachineRecord,
        provider_machine_id: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalReconcileReport {
    pub adopted_ids: Vec<String>,
    pub cleaned_up_ids: Vec<String>,
    pub kept_uncertain_ids: Vec<String>,
    pub ambiguous_ids: Vec<String>,
}

pub(super) fn require_provider_machine_id(record: &CloudMachineRecord) -> Result<&str, CloudError> {
    record.provider_machine_id().ok_or_else(|| {
        CloudError::ProviderUnconfirmed(format!(
            "cloud machine {} has no confirmed backend machine id; reconcile or delete it first",
            record.machine_id
        ))
    })
}

impl CloudMachineController {
    /// Post-write create timeout / uncertain failure handler: marks the record uncertain with its
    /// creation request id, reconciles against the backend list to adopt any created machine, and
    /// surfaces `ProviderUnconfirmed` instead of `ProviderUnavailable` or `Failed`.
    pub(super) async fn handle_uncertain_create_outcome(
        &self,
        machine_id: &str,
        error: CloudError,
        provisioning: &dyn CloudProvisioningPort,
    ) -> CloudError {
        let detail = match &error {
            CloudError::ProviderUnconfirmed(detail) => detail.clone(),
            other => other.to_string(),
        };
        self.mark_pending_unconfirmed(machine_id, PendingProvisioningOp::Create, &detail);
        match self
            .reconcile_local_record(
                machine_id,
                LocalRecordReconcileIntent::ReconcileCreate,
                provisioning,
            )
            .await
        {
            Ok(LocalRecordReconcileOutcome::Adopted(adopted)) => CloudError::ProviderUnconfirmed(
                format!(
                    "create request outcome was unconfirmed ({detail}); adopted backend machine {}",
                    adopted.ownership.provider_machine_id
                ),
            ),
            Ok(LocalRecordReconcileOutcome::DeletedLocal(_)) => CloudError::ProviderUnconfirmed(
                format!(
                    "create request outcome was unconfirmed ({detail}); no backend machine existed and tentative record was cleaned up"
                ),
            ),
            Ok(
                LocalRecordReconcileOutcome::AlreadyBound(_)
                | LocalRecordReconcileOutcome::KeptUncertain
                | LocalRecordReconcileOutcome::Ambiguous(_),
            )
            | Err(_) => CloudError::ProviderUnconfirmed(detail),
        }
    }

    /// Resolve a record before calling backend delete: never sends a local id as a provider id.
    /// Reconciles local-only records against the backend list to adopt an existing backend id or
    /// delete locally when nothing exists on the backend.
    pub(super) async fn resolve_delete_target(
        &self,
        record: &CloudMachineRecord,
        provisioning: &dyn CloudProvisioningPort,
    ) -> Result<DeleteResolution, CloudError> {
        if let Some(provider_id) = record.provider_machine_id() {
            return Ok(DeleteResolution::Backend {
                record: record.clone(),
                provider_machine_id: provider_id.to_owned(),
            });
        }
        match self
            .reconcile_local_record(
                &record.machine_id,
                LocalRecordReconcileIntent::Delete,
                provisioning,
            )
            .await?
        {
            LocalRecordReconcileOutcome::AlreadyBound(bound)
            | LocalRecordReconcileOutcome::Adopted(bound) => {
                let provider_machine_id = require_provider_machine_id(&bound)?.to_owned();
                Ok(DeleteResolution::Backend {
                    record: bound,
                    provider_machine_id,
                })
            }
            LocalRecordReconcileOutcome::DeletedLocal(deleted) => {
                Ok(DeleteResolution::DeletedLocal(deleted))
            }
            LocalRecordReconcileOutcome::Ambiguous(candidates) => {
                Err(CloudError::MachineAmbiguous(format!(
                    "{} matched multiple backend machines ({})",
                    record.machine_id,
                    candidates.join(", ")
                )))
            }
            LocalRecordReconcileOutcome::KeptUncertain => {
                Err(CloudError::ProviderUnconfirmed(format!(
                    "could not reconcile local-only cloud machine {} before delete",
                    record.machine_id
                )))
            }
        }
    }
    pub(super) async fn reconcile_local_record(
        &self,
        machine_id: &str,
        intent: LocalRecordReconcileIntent,
        provisioning: &dyn CloudProvisioningPort,
    ) -> Result<LocalRecordReconcileOutcome, CloudError> {
        let record = self.find_record(machine_id)?;
        if record.provider_machine_id().is_some() {
            return Ok(LocalRecordReconcileOutcome::AlreadyBound(record));
        }
        let backend_machines = provisioning.list_backend_machines().await?;
        self.apply_local_reconcile_from_backend(
            &record.machine_id,
            &backend_machines,
            intent,
            LocalReconcilePolicy::default(),
        )
    }

    pub(super) fn apply_local_reconcile_from_backend(
        &self,
        machine_id: &str,
        backend_machines: &[BackendMachineEntry],
        intent: LocalRecordReconcileIntent,
        policy: LocalReconcilePolicy,
    ) -> Result<LocalRecordReconcileOutcome, CloudError> {
        let all_records = self.list_records();
        let record = all_records
            .iter()
            .find(|r| r.machine_id == machine_id)
            .cloned()
            .ok_or_else(|| CloudError::MachineNotFound(machine_id.to_owned()))?;
        let now_ms = self.clock.now_ms();
        match plan_local_record_reconcile(
            &record,
            &all_records,
            backend_machines,
            now_ms,
            intent,
            policy,
        ) {
            LocalRecordReconcileAction::AlreadyBound => {
                Ok(LocalRecordReconcileOutcome::AlreadyBound(record))
            }
            LocalRecordReconcileAction::Adopt { entry, .. } => {
                let updated = self.update_record(&record.machine_id, |rec| {
                    apply_local_record_adoption(rec, &entry, now_ms)
                })?;
                Ok(LocalRecordReconcileOutcome::Adopted(updated))
            }
            LocalRecordReconcileAction::DeleteLocal | LocalRecordReconcileAction::CleanUpLocal => {
                let deleted = self.update_record(&record.machine_id, |rec| {
                    apply_local_record_delete(rec, now_ms)
                })?;
                Ok(LocalRecordReconcileOutcome::DeletedLocal(deleted))
            }
            LocalRecordReconcileAction::KeepUncertain => {
                Ok(LocalRecordReconcileOutcome::KeptUncertain)
            }
            LocalRecordReconcileAction::Ambiguous {
                candidate_backend_ids,
            } => Ok(LocalRecordReconcileOutcome::Ambiguous(
                candidate_backend_ids,
            )),
        }
    }

    /// Reconcile every local-only / uncertain-create cloud record against the backend machine list.
    pub async fn reconcile_local_records(
        &self,
        provisioning: &dyn CloudProvisioningPort,
    ) -> Result<LocalReconcileReport, CloudError> {
        self.reconcile_local_records_with_policy(provisioning, LocalReconcilePolicy::default())
            .await
    }

    pub async fn reconcile_local_records_with_policy(
        &self,
        provisioning: &dyn CloudProvisioningPort,
        policy: LocalReconcilePolicy,
    ) -> Result<LocalReconcileReport, CloudError> {
        let local_only_ids: Vec<String> = self
            .list_records()
            .into_iter()
            .filter(|rec| rec.provider_machine_id().is_none())
            .map(|rec| rec.machine_id)
            .collect();
        if local_only_ids.is_empty() {
            return Ok(LocalReconcileReport::default());
        }
        let backend_machines = provisioning.list_backend_machines().await?;
        let mut report = LocalReconcileReport::default();
        for machine_id in local_only_ids {
            match self.apply_local_reconcile_from_backend(
                &machine_id,
                &backend_machines,
                LocalRecordReconcileIntent::ReconcileCreate,
                policy,
            )? {
                LocalRecordReconcileOutcome::AlreadyBound(_) => {}
                LocalRecordReconcileOutcome::Adopted(rec) => {
                    report.adopted_ids.push(rec.machine_id)
                }
                LocalRecordReconcileOutcome::DeletedLocal(rec) => {
                    report.cleaned_up_ids.push(rec.machine_id)
                }
                LocalRecordReconcileOutcome::KeptUncertain => {
                    report.kept_uncertain_ids.push(machine_id)
                }
                LocalRecordReconcileOutcome::Ambiguous(_) => report.ambiguous_ids.push(machine_id),
            }
        }
        Ok(report)
    }

    pub async fn delete(
        &self,
        selector: &str,
        confirm: bool,
        provisioning: &dyn CloudProvisioningPort,
    ) -> Result<CloudReceipt, CloudError> {
        self.delete_with_mode(
            selector,
            CloudDeleteMode::Permanent,
            None,
            confirm,
            provisioning,
        )
        .await
    }

    pub async fn delete_with_mode(
        &self,
        selector: &str,
        mode: CloudDeleteMode,
        archive_id: Option<&str>,
        confirm: bool,
        provisioning: &dyn CloudProvisioningPort,
    ) -> Result<CloudReceipt, CloudError> {
        let record = self.find_record(selector)?;
        if !confirm {
            let warning = match mode {
                CloudDeleteMode::Permanent => format!(
                    "Deleting cloud machine '{}' ({}) permanently destroys any unpushed work and its retained archive. Re-run with --confirm to proceed.",
                    record.name, record.machine_id
                ),
                CloudDeleteMode::Recoverable => format!(
                    "Recoverably deleting cloud machine '{}' ({}) releases its live volume while retaining its verified owner-bound essentials archive until the configured purge date. Re-run with --confirm to proceed.",
                    record.name, record.machine_id
                ),
            };
            return Err(CloudError::DeleteConfirmationRequired {
                machine_id: record.machine_id.clone(),
                warning,
            });
        }
        let (record, provider_machine_id) =
            match self.resolve_delete_target(&record, provisioning).await? {
                DeleteResolution::DeletedLocal(deleted) => {
                    return Ok(deleted.receipt(self.clock.now_ms(), None));
                }
                DeleteResolution::Backend {
                    record,
                    provider_machine_id,
                } => (record, provider_machine_id),
            };
        let request_id =
            self.begin_pending_request(&record.machine_id, PendingProvisioningOp::Delete)?;
        let delete_receipt = match provisioning
            .delete_with_mode(&provider_machine_id, &request_id, mode, archive_id)
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
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
        };
        let now_ms = self.clock.now_ms();
        let updated = self.update_record(&record.machine_id, |rec| {
            let mut next = transition(rec, LifecycleEvent::Deleted, now_ms)?;
            next.pending_request = None;
            next.ownership.auxiliary_resource_ids.insert(
                AUX_KEY_STORAGE_TIER.into(),
                delete_receipt.storage_tier.as_str().into(),
            );
            if let Some(arc_id) = delete_receipt.archive_id.as_ref() {
                next.ownership
                    .auxiliary_resource_ids
                    .insert(AUX_KEY_RECOVERABLE_ARCHIVE_ID.into(), arc_id.clone());
            } else {
                next.ownership
                    .auxiliary_resource_ids
                    .remove(AUX_KEY_RECOVERABLE_ARCHIVE_ID);
            }
            if let Some(purge_at) = delete_receipt.purge_at {
                next.ownership
                    .auxiliary_resource_ids
                    .insert(AUX_KEY_PURGE_AT.into(), purge_at.to_string());
            }
            Ok(next)
        })?;
        let notice = match delete_receipt.storage_tier {
            CloudStorageTier::RecoverableDeleted => Some(format!(
                "Live volume released; encrypted essentials archive retained ({}) for recreate.",
                delete_receipt.archive_id.as_deref().unwrap_or("verified")
            )),
            _ => delete_receipt
                .snapshot_residual
                .as_ref()
                .filter(|s| s.snapshots_remaining > 0)
                .map(|s| {
                    format!(
                        "Archive erased; {} provider snapshot(s) remain under bounded provider retention.",
                        s.snapshots_remaining
                    )
                }),
        };
        Ok(updated.receipt(now_ms, notice))
    }
}
