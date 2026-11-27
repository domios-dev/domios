use async_trait::async_trait;
use codeterm_account::account_mesh::{
    cloud_provisioning::{
        ProvisionedMachineReply, WireCloudDeleteMode, WireCloudSleepMode, WireCloudStorageTier,
        WireCloudStorageUsageSummary, WireProviderSnapshotResidual, WireWarmBootstrapPhase,
    },
    AccountMeshRefusal,
};
use codeterm_neighbors_service::service::account_mesh::NeighborsExt;
use codeterm_service_core::service::cloud_machines::{
    CloudArchiveReceipt, CloudCheckoutSession, CloudColdWakeParams, CloudDeleteMode,
    CloudDeleteReceipt, CloudError, CloudMachineStorageUsage, CloudPlanAllowances,
    CloudPromoRedemption, CloudProvisioningPort, CloudRate, CloudSleepMode, CloudStorageTier,
    CloudStorageUsageSummary, CloudUsagePack, CloudUsageSize, CloudUsageSummary,
    CloudVolumeResizeReply, ProviderSnapshotResidual, ProvisionedLease, ProvisionedMachine,
    ProvisioningCreateRequest,
};

pub(super) struct AccountCloudProvisioningAdapter {
    pub(super) svc: codeterm_service_core::service::ServiceState,
}

fn map_sleep_mode(mode: WireCloudSleepMode) -> CloudSleepMode {
    match mode {
        WireCloudSleepMode::Pause => CloudSleepMode::Pause,
        WireCloudSleepMode::Stop => CloudSleepMode::Stop,
        WireCloudSleepMode::StopBilled => CloudSleepMode::StopBilled,
        WireCloudSleepMode::DeleteRecreate => CloudSleepMode::DeleteRecreate,
    }
}

fn map_storage_tier(tier: WireCloudStorageTier) -> CloudStorageTier {
    match tier {
        WireCloudStorageTier::Hot => CloudStorageTier::Hot,
        WireCloudStorageTier::Evicting => CloudStorageTier::Evicting,
        WireCloudStorageTier::Cold => CloudStorageTier::Cold,
        WireCloudStorageTier::RecoverableDeleted => CloudStorageTier::RecoverableDeleted,
        WireCloudStorageTier::Purged => CloudStorageTier::Purged,
    }
}

fn map_snapshot_residual(r: WireProviderSnapshotResidual) -> ProviderSnapshotResidual {
    ProviderSnapshotResidual {
        provider: r.provider,
        snapshots_remaining: r.snapshots_remaining,
        earliest_expiry_at: r.earliest_expiry_at,
        latest_expiry_at: r.latest_expiry_at,
        auto_backup_enabled: r.auto_backup_enabled,
        individual_delete_supported: r.individual_delete_supported,
        erasure_state: r.erasure_state,
        checked_at: r.checked_at,
    }
}

fn map_storage_summary(s: WireCloudStorageUsageSummary) -> CloudStorageUsageSummary {
    CloudStorageUsageSummary {
        hot_gb_month_rate_cents: s.hot_gb_month_rate_cents,
        hot_days: s.hot_days,
        purge_days: s.purge_days,
        purge_warning_days: s.purge_warning_days,
        cold_archive_included: s.cold_archive_included,
        total_allocated_gb: s.total_allocated_gb,
        total_used_bytes: s.total_used_bytes,
        total_hot_monthly_cost_cents: s.total_hot_monthly_cost_cents,
        machines: s
            .machines
            .into_iter()
            .map(|m| CloudMachineStorageUsage {
                machine_id: m.machine_id,
                create_request_id: m.create_request_id,
                name: m.name,
                created_at_ms: m.created_at.map(|ts| {
                    if ts < 100_000_000_000 {
                        ts.saturating_mul(1_000)
                    } else {
                        ts
                    }
                }),
                tier: map_storage_tier(m.tier),
                allocated_gb: m.allocated_gb,
                used_bytes: m.used_bytes,
                used_gb: m.used_gb,
                hot_monthly_cost_cents: m.hot_monthly_cost_cents,
                hot_accrued_cents: m.hot_accrued_cents,
                cold_included: m.cold_included,
                stopped_at: m.stopped_at,
                hot_until_at: m.hot_until_at,
                cold_since_at: m.cold_since_at,
                purge_at: m.purge_at,
                archive_id: m.archive_id,
                archive_bytes: m.archive_bytes,
                archive_generation: m.archive_generation,
                disk_full: m.disk_full,
                volume_upgrade_max_gb: m.volume_upgrade_max_gb,
                snapshot_residual: m.snapshot_residual.map(map_snapshot_residual),
            })
            .collect(),
    }
}

pub(super) fn map_account_refusal(refusal: AccountMeshRefusal) -> CloudError {
    match refusal {
        AccountMeshRefusal::CloudMachineLimit {
            limit,
            active_count,
        } => CloudError::LimitReached {
            limit,
            active_count,
        },
        AccountMeshRefusal::LeaseRequired => CloudError::LeaseRequired,
        AccountMeshRefusal::CreditsExhausted => CloudError::CreditsExhausted,
        AccountMeshRefusal::PromoInvalidCode => CloudError::PromoInvalidCode,
        AccountMeshRefusal::PromoExpired => CloudError::PromoExpired,
        AccountMeshRefusal::PromoDisabled => CloudError::PromoDisabled,
        AccountMeshRefusal::PromoExhausted => CloudError::PromoExhausted,
        AccountMeshRefusal::PromoSpendCapExceeded => CloudError::PromoSpendCapExceeded,
        AccountMeshRefusal::CheckoutBelowMin => CloudError::CheckoutBelowMin,
        AccountMeshRefusal::CheckoutAboveMax => CloudError::CheckoutAboveMax,
        AccountMeshRefusal::CheckoutTerritory => CloudError::CheckoutTerritory,
        AccountMeshRefusal::NotFound => CloudError::MachineNotFound("cloud machine".into()),
        AccountMeshRefusal::Conflict | AccountMeshRefusal::Replay => {
            CloudError::Conflict(format!("{refusal:?}"))
        }
        AccountMeshRefusal::MachineBusy => CloudError::MachineBusy {
            machine_id: "cloud machine".into(),
            reason: "work is currently running on this machine".into(),
        },
        AccountMeshRefusal::RateLimited {
            retry_after_seconds,
        } => CloudError::RateLimited {
            retry_after_seconds,
        },
        AccountMeshRefusal::ProviderRefused => {
            CloudError::ProviderRefused("cloud provider refused operation".into())
        }
        AccountMeshRefusal::ProviderUnconfirmed => {
            CloudError::ProviderUnconfirmed("cloud provisioning outcome unconfirmed".into())
        }
        AccountMeshRefusal::ProviderUnavailable
        | AccountMeshRefusal::Unavailable
        | AccountMeshRefusal::AuthorityUnavailable
        | AccountMeshRefusal::Offline => CloudError::ProviderUnavailable(format!("{refusal:?}")),
        other => CloudError::ProviderRefused(format!("{other:?}")),
    }
}

pub(super) fn map_checkout_refusal(refusal: AccountMeshRefusal) -> CloudError {
    match refusal {
        AccountMeshRefusal::Invalid => CloudError::CheckoutInvalid,
        AccountMeshRefusal::PromoSpendCapExceeded => CloudError::PromoSpendCapExceeded,
        AccountMeshRefusal::CheckoutBelowMin => CloudError::CheckoutBelowMin,
        AccountMeshRefusal::CheckoutAboveMax => CloudError::CheckoutAboveMax,
        AccountMeshRefusal::CheckoutTerritory => CloudError::CheckoutTerritory,
        AccountMeshRefusal::RateLimited {
            retry_after_seconds,
        } => CloudError::RateLimited {
            retry_after_seconds,
        },
        AccountMeshRefusal::ProviderUnavailable
        | AccountMeshRefusal::Unavailable
        | AccountMeshRefusal::AuthorityUnavailable
        | AccountMeshRefusal::Offline => CloudError::CheckoutUnavailable(format!("{refusal:?}")),
        other => map_account_refusal(other),
    }
}

fn map_provisioned_reply(reply: ProvisionedMachineReply) -> Result<ProvisionedMachine, CloudError> {
    let endpoint = reply.validated_endpoint().ok_or_else(|| {
        CloudError::ProviderUnavailable("backend returned an invalid cloud machine endpoint".into())
    })?;
    Ok(ProvisionedMachine {
        machine_id: reply.machine_id,
        provider: reply.provider,
        endpoint,
        size: reply.size,
        rate: CloudRate {
            amount: reply.rate.amount,
            currency: reply.rate.currency,
            credits_per_hour: Some(reply.rate.credits_per_hour),
        },
        sleep_mode: map_sleep_mode(reply.sleep_mode),
        lease_id: reply.lease_id,
        lease_expires_at_ms: reply.lease_expires_at.saturating_mul(1_000),
    })
}

#[async_trait]
impl CloudProvisioningPort for AccountCloudProvisioningAdapter {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .create_machine(
                &request.size,
                &request.controller_key,
                &request.creation_psk,
                &request.request_id,
            )
            .await
            .map_err(map_account_refusal)?;
        map_provisioned_reply(reply)
    }

    async fn wake(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .wake_machine(machine_id, request_id)
            .await
            .map_err(map_account_refusal)?;
        map_provisioned_reply(reply)
    }

    async fn sleep(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<CloudSleepMode, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .sleep_machine(machine_id, request_id)
            .await
            .map_err(map_account_refusal)?;
        Ok(map_sleep_mode(reply.sleep_mode))
    }

    async fn archive_now(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<CloudArchiveReceipt, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .archive_machine(machine_id, request_id)
            .await
            .map_err(map_account_refusal)?;
        Ok(CloudArchiveReceipt {
            machine_id: reply.machine_id,
            state: reply.state,
            storage_tier: map_storage_tier(reply.storage_tier),
            archive_id: reply.archive_id,
            cold_since_at: reply.cold_since_at,
            purge_at: reply.purge_at,
            archive_generation: reply.archive_generation,
            already_archived: reply.already_archived,
            snapshot_residual: reply.snapshot_residual.map(map_snapshot_residual),
        })
    }

    async fn delete(&self, machine_id: &str, request_id: &str) -> Result<(), CloudError> {
        self.delete_with_mode(machine_id, request_id, CloudDeleteMode::Permanent, None)
            .await
            .map(|_| ())
    }

    async fn delete_with_mode(
        &self,
        machine_id: &str,
        request_id: &str,
        mode: CloudDeleteMode,
        archive_id: Option<&str>,
    ) -> Result<CloudDeleteReceipt, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let wire_mode = match mode {
            CloudDeleteMode::Permanent => None,
            CloudDeleteMode::Recoverable => Some(WireCloudDeleteMode::Recoverable),
        };
        let reply = controller
            .delete_machine_with_mode(machine_id, request_id, wire_mode, archive_id)
            .await
            .map_err(map_account_refusal)?;
        Ok(CloudDeleteReceipt {
            machine_id: reply.machine_id,
            storage_tier: reply
                .storage_tier
                .map(map_storage_tier)
                .unwrap_or(match mode {
                    CloudDeleteMode::Permanent => CloudStorageTier::Purged,
                    CloudDeleteMode::Recoverable => CloudStorageTier::RecoverableDeleted,
                }),
            archive_id: reply.archive_id,
            purge_at: reply.purge_at,
            snapshot_residual: reply.snapshot_residual.map(map_snapshot_residual),
        })
    }

    async fn wake_cold(
        &self,
        machine_id: &str,
        request_id: &str,
        params: &CloudColdWakeParams,
    ) -> Result<ProvisionedMachine, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .wake_machine_cold(
                machine_id,
                request_id,
                &params.controller_key,
                &params.creation_psk,
                params.archive_id.as_deref(),
            )
            .await
            .map_err(map_account_refusal)?;
        map_provisioned_reply(reply)
    }

    async fn resize_volume(
        &self,
        machine_id: &str,
        new_size_gb: u32,
        request_id: &str,
    ) -> Result<CloudVolumeResizeReply, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .resize_volume(machine_id, new_size_gb, true, request_id)
            .await
            .map_err(map_account_refusal)?;
        Ok(CloudVolumeResizeReply {
            machine_id: reply.machine_id,
            allocated_gb: reply.allocated_gb,
            needs_restart: reply.needs_restart,
            hot_monthly_cost_cents: reply.hot_monthly_cost_cents,
        })
    }

    async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        request_id: &str,
    ) -> Result<ProvisionedLease, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .renew_lease(machine_id, lease_id, request_id)
            .await
            .map_err(map_account_refusal)?;
        Ok(ProvisionedLease {
            machine_id: reply.machine_id,
            lease_id: reply.lease_id,
            lease_expires_at_ms: reply.lease_expires_at.saturating_mul(1_000),
        })
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .cloud_usage()
            .await
            .map_err(map_account_refusal)?;
        Ok(CloudUsageSummary {
            used: reply.used,
            allowance: reply.allowance,
            purchased: reply.purchased,
            remaining: reply.remaining,
            cap: reply.cap,
            burn_per_size: reply.burn_per_size,
            currency: reply.currency,
            period_end_at: reply.period_end_at,
            custom_min_cents: reply.custom_min_cents,
            custom_max_cents: reply.custom_max_cents,
            plan: reply.plan,
            sizes: reply.sizes.map(|sizes| {
                sizes
                    .into_iter()
                    .map(|size| CloudUsageSize {
                        id: size.id,
                        cpu: size.cpu,
                        ram_gb: size.ram_gb,
                        hourly_rate_cents: size.hourly_rate_cents,
                        credits_per_hour: size.credits_per_hour,
                    })
                    .collect()
            }),
            packs: reply.packs.map(|packs| {
                packs
                    .into_iter()
                    .map(|pack| CloudUsagePack {
                        pack_id: pack.pack_id,
                        price_cents: pack.price_cents,
                        hours: pack.hours,
                    })
                    .collect()
            }),
            purchased_expiry_days: reply.purchased_expiry_days,
            allowance_hours: reply.allowance_hours.map(|hours| CloudPlanAllowances {
                free: hours.free,
                pro: hours.pro,
                max: hours.max,
                sponsor: hours.sponsor,
            }),
            storage: reply.storage.map(map_storage_summary),
        })
    }

    async fn redeem_promo(&self, code: &str) -> Result<CloudPromoRedemption, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let reply = controller
            .redeem_promo(code)
            .await
            .map_err(map_account_refusal)?;
        Ok(CloudPromoRedemption {
            schema_version: reply.schema_version,
            status: reply.status,
            already_redeemed: reply.already_redeemed,
            hours: reply.hours,
            expires_at: reply.expires_at,
            remaining: reply.remaining,
            purchased: reply.purchased,
        })
    }

    async fn create_checkout(
        &self,
        pack_id: &str,
        amount_cents: Option<u64>,
    ) -> Result<CloudCheckoutSession, CloudError> {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::CheckoutUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_checkout_refusal)?;
        let reply = controller
            .create_checkout(pack_id, amount_cents)
            .await
            .map_err(map_checkout_refusal)?;
        Ok(CloudCheckoutSession {
            url: reply.url,
            session_id: reply.session_id,
            pack_id: reply.pack_id,
            credits: reply.credits,
            amount_cents: reply.amount_cents,
            currency: reply.currency,
        })
    }

    async fn record_warm_bootstrap_phases(
        &self,
        machine_id: &str,
        phase: codeterm_service_core::service::cloud_machines::WarmBootstrapPhaseEvent,
        request_id: &str,
    ) -> Result<
        Option<codeterm_service_core::service::cloud_machines::BackendCreatePhases>,
        CloudError,
    > {
        let mesh = self
            .svc
            .account_mesh()
            .await
            .map_err(|_| CloudError::ProviderUnavailable("account mesh unavailable".into()))?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(map_account_refusal)?;
        let wire_phase = match phase {
            codeterm_service_core::service::cloud_machines::WarmBootstrapPhaseEvent::SeedStarted => {
                WireWarmBootstrapPhase::SeedStarted
            }
            codeterm_service_core::service::cloud_machines::WarmBootstrapPhaseEvent::WarmConsoleReady => {
                WireWarmBootstrapPhase::WarmConsoleReady
            }
        };
        let reply = controller
            .record_warm_bootstrap_phases(machine_id, wire_phase, request_id)
            .await
            .map_err(map_account_refusal)?;
        let phases = reply.last_create_phases;
        Ok(Some(
            codeterm_service_core::service::cloud_machines::BackendCreatePhases {
                requested: phases.requested,
                allocated: phases.allocated,
                ipv4_bound: phases.ipv4_bound,
                running: phases.running,
                enrolled: phases.enrolled,
                peer_ready: phases.peer_ready,
                seed_started: phases.seed_started,
                warm_console_ready: phases.warm_console_ready,
                warm_bootstrap_delta_secs: reply.warm_bootstrap_delta_secs,
            },
        ))
    }
}
