use super::controller::CloudMachineController;
use super::ports::{CloudPeerControlPort, CloudProvisioningPort};
use super::types::{CloudError, CloudMachineRecord, CloudReceipt};
use super::warm_bootstrap::{
    CloudCreatePhases, CloudWarmBootstrapSummary, MachineWarmBootstrapEntry,
    PrepareProviderSignInRequest, PrepareProviderSignInResponse, WarmBootstrapPhaseEvent,
    WarmBootstrapStatus,
};

impl CloudMachineController {
    /// Directory root beside the machine store (isolated per tempdir in tests, data dir in production).
    pub fn warm_state_dir(&self) -> std::path::PathBuf {
        self.warm_ledger_store
            .path()
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| codeterm_paths::data_dir().join("mesh"))
    }

    pub(super) fn persist_warm_ledger_locked(
        &self,
        entries: &std::collections::BTreeMap<String, MachineWarmBootstrapEntry>,
    ) {
        if let Err(error) = self.warm_ledger_store.save(entries) {
            log::warn!("[cloud] failed to persist warm bootstrap ledger: {error}");
        }
    }

    pub(super) fn update_warm_entry(
        &self,
        machine_id: &str,
        mutate: impl FnOnce(&mut MachineWarmBootstrapEntry),
    ) -> MachineWarmBootstrapEntry {
        let mut ledger = self.warm_ledger.lock();
        let entry = ledger.entry(machine_id.to_owned()).or_default();
        mutate(entry);
        let cloned = entry.clone();
        self.persist_warm_ledger_locked(&ledger);
        cloned
    }

    pub(super) fn receipt_for_record(
        &self,
        record: &CloudMachineRecord,
        now_ms: u64,
        warning: Option<String>,
    ) -> CloudReceipt {
        let entry = self.warm_ledger.lock().get(&record.machine_id).cloned();
        match entry {
            Some(entry) => {
                record.receipt_with_warm(now_ms, warning, entry.create_phases, entry.warm_bootstrap)
            }
            None => record.receipt(now_ms, warning),
        }
    }

    pub fn bootstrap_status(
        &self,
        selector: &str,
    ) -> Result<MachineWarmBootstrapEntry, CloudError> {
        let record = self.find_record(selector)?;
        Ok(self
            .warm_ledger
            .lock()
            .get(&record.machine_id)
            .cloned()
            .unwrap_or_default())
    }

    /// Marks a verified recovery revision on a cloud machine so fresh seeding never runs over restored state.
    pub fn mark_recovery_revision(
        &self,
        selector: &str,
        revision: impl Into<String>,
    ) -> Result<MachineWarmBootstrapEntry, CloudError> {
        let record = self.find_record(selector)?;
        let rev = revision.into();
        Ok(self.update_warm_entry(&record.machine_id, |entry| {
            entry.recovery_revision = Some(rev);
        }))
    }

    /// Records human provider sign-in timings separately from the automated warm-bootstrap delta.
    pub fn record_provider_signin_timing(
        &self,
        selector: &str,
        started_at_ms: Option<u64>,
        completed_at_ms: Option<u64>,
    ) -> Result<CloudCreatePhases, CloudError> {
        let record = self.find_record(selector)?;
        let entry = self.update_warm_entry(&record.machine_id, |entry| {
            let phases = entry
                .create_phases
                .get_or_insert_with(CloudCreatePhases::default);
            if let Some(started) = started_at_ms {
                phases.provider_signin_started_at_ms.get_or_insert(started);
            }
            if let Some(completed) = completed_at_ms {
                phases.provider_signin_completed_at_ms = Some(completed);
            }
        });
        Ok(entry.create_phases.unwrap_or_default())
    }

    pub async fn prepare_provider_signin(
        &self,
        selector: &str,
        request: &PrepareProviderSignInRequest,
        peer_control: &dyn CloudPeerControlPort,
    ) -> Result<PrepareProviderSignInResponse, CloudError> {
        let record = self.find_record(selector)?;
        peer_control.prepare_provider_signin(&record, request).await
    }

    /// Runs post-enrollment warm bootstrap once on a fresh create (never over a verified recovery revision),
    /// records `seed_started` and `warm_console_ready` authoritatively via the backend callable (strictly gated
    /// on verified readable owner context), and mirrors backend timestamps onto the desktop record without
    /// computing `warm_bootstrap_delta_secs` from client wall clock alone.
    pub(super) async fn run_post_enrollment_warm_bootstrap(
        &self,
        record: &CloudMachineRecord,
        creation_generation: &str,
        provisioning: &dyn CloudProvisioningPort,
        peer_control: &dyn CloudPeerControlPort,
    ) {
        let existing_recovery = self
            .warm_ledger
            .lock()
            .get(&record.machine_id)
            .and_then(|e| e.recovery_revision.clone());
        if existing_recovery.is_some() {
            return;
        }

        let seed_req_id = super::controller::mint_request_id();
        let seed_result = match record.provider_machine_id() {
            Some(provider_id) => {
                provisioning
                    .record_warm_bootstrap_phases(
                        provider_id,
                        WarmBootstrapPhaseEvent::SeedStarted,
                        &seed_req_id,
                    )
                    .await
            }
            None => Ok(None),
        };
        match seed_result {
            Ok(Some(backend_phases)) => {
                self.update_warm_entry(&record.machine_id, |entry| {
                    entry.creation_generation = Some(creation_generation.to_owned());
                    let phases = entry
                        .create_phases
                        .get_or_insert_with(CloudCreatePhases::default);
                    phases.apply_backend_phases(&backend_phases);
                });
            }
            Ok(None) => {
                self.update_warm_entry(&record.machine_id, |entry| {
                    entry.creation_generation = Some(creation_generation.to_owned());
                });
            }
            Err(error) => {
                log::debug!(
                    "[cloud] backend seed_started phase write failed for {}: {error}",
                    record.machine_id
                );
                self.update_warm_entry(&record.machine_id, |entry| {
                    entry.creation_generation = Some(creation_generation.to_owned());
                });
            }
        }

        let apply_result = peer_control
            .apply_warm_bootstrap(record, creation_generation)
            .await;

        let Some(result) = (match apply_result {
            Ok(Some(result)) => Some(result),
            Ok(None) => return,
            Err(error) => {
                log::warn!(
                    "[cloud] post-enrollment warm bootstrap failed for {}: {error}",
                    record.machine_id
                );
                self.update_warm_entry(&record.machine_id, |entry| {
                    entry.warm_bootstrap = Some(CloudWarmBootstrapSummary {
                        schema_version: super::warm_bootstrap::WARM_BOOTSTRAP_SCHEMA_VERSION,
                        status: WarmBootstrapStatus::Failed,
                        reject_reason: None,
                        recovery_revision: None,
                        creation_generation: creation_generation.to_owned(),
                        readable_owner_context: false,
                        wake_up_verified: false,
                        applied_counts: Default::default(),
                        excluded: Vec::new(),
                        warnings: vec![error.to_string()],
                        general_tab_id: None,
                        providers: Vec::new(),
                    });
                });
                return;
            }
        }) else {
            return;
        };

        // Red line: never record warm_console_ready unless target verified readable owner context.
        let backend_ready_phases = if result.readable_owner_context && result.wake_up_verified {
            if let Some(provider_id) = record.provider_machine_id() {
                let ready_req_id = super::controller::mint_request_id();
                match provisioning
                    .record_warm_bootstrap_phases(
                        provider_id,
                        WarmBootstrapPhaseEvent::WarmConsoleReady,
                        &ready_req_id,
                    )
                    .await
                {
                    Ok(phases) => phases,
                    Err(error) => {
                        log::debug!(
                            "[cloud] backend warm_console_ready phase write failed for {}: {error}",
                            record.machine_id
                        );
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        let summary = CloudWarmBootstrapSummary::from(&result);
        self.update_warm_entry(&record.machine_id, |entry| {
            entry.creation_generation = Some(creation_generation.to_owned());
            if let Some(rev) = &result.recovery_revision {
                entry.recovery_revision = Some(rev.clone());
            }
            if let Some(backend_phases) = &backend_ready_phases {
                let phases = entry
                    .create_phases
                    .get_or_insert_with(CloudCreatePhases::default);
                phases.apply_backend_phases(backend_phases);
            }
            entry.warm_bootstrap = Some(summary);
        });
    }
}
