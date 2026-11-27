use std::future::Future;

use super::*;

struct LeaseHours {
    account: String,
    machine_id: String,
    warnings: WireCloudCreditWarnings,
}

pub(super) fn needs_usage_refresh(error: &CloudError) -> bool {
    matches!(
        error,
        CloudError::WakeDeadlineExceeded { .. }
            | CloudError::ProviderUnconfirmed(_)
            | CloudError::CreditsExhausted
            | CloudError::LeaseRequired
    )
}

impl AccountCloudProvisioningAdapter {
    pub(super) async fn read_usage_reply(
        &self,
        expected_account: Option<&str>,
    ) -> Result<(String, CloudUsageReply), CloudError> {
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
        let account = controller.account_id().to_owned();
        if expected_account.is_some_and(|expected| expected != account.as_str()) {
            return Err(map_account_refusal(AccountMeshRefusal::Cancelled));
        }
        let reply = controller
            .cloud_usage()
            .await
            .map_err(map_account_refusal)?;
        Ok((account, reply))
    }

    pub(super) fn observe_granted_lease(
        &self,
        account: &str,
        machine_id: &str,
        warnings: Option<WireCloudCreditWarnings>,
    ) {
        let Some(warnings) = warnings else {
            return;
        };
        let lease = LeaseHours {
            account: account.into(),
            machine_id: machine_id.into(),
            warnings,
        };
        self.schedule_hours_refresh(Some(account.into()), Some(lease));
    }

    pub(super) fn refresh_hours_after_failure(&self, account: Option<String>) {
        self.schedule_hours_refresh(account, None);
    }

    fn schedule_hours_refresh(&self, account: Option<String>, lease: Option<LeaseHours>) {
        let adapter = Self {
            svc: self.svc.clone(),
        };
        tokio::spawn(async move {
            refresh_hours(
                &adapter.svc,
                lease,
                adapter.read_usage_reply(account.as_deref()),
            )
            .await;
        });
    }
}

async fn refresh_hours(
    svc: &codeterm_service_core::service::ServiceState,
    lease: Option<LeaseHours>,
    read_usage: impl Future<Output = Result<(String, CloudUsageReply), CloudError>>,
) {
    if let Some(lease) = &lease {
        let check_svc = svc.clone();
        let account = lease.account.clone();
        let warnings = lease.warnings;
        let pending = tokio::task::spawn_blocking(move || {
            check_svc.has_unseen_cloud_hours(&account, warnings)
        })
        .await
        .unwrap_or(true);
        if !pending {
            if warnings.pct80_at.is_none()
                && warnings.pct95_at.is_none()
                && warnings.exhausted_at.is_none()
            {
                observe(svc, lease.account.clone(), None, warnings, None).await;
            }
            return;
        }
    }

    match read_usage.await {
        Ok((account, mut reply)) => {
            if let Some(lease) = &lease {
                reply.warnings = Some(merge_warnings(lease.warnings, reply.warnings));
            }
            observe_usage(
                svc,
                &account,
                &reply,
                lease.as_ref().map(|lease| lease.machine_id.as_str()),
            )
            .await;
            let svc = svc.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let mut summary = map_usage_reply(reply);
                svc.cloud_machines().apply_usage_summary(&mut summary);
                svc.ui.emit(
                    "cloud-hours-changed",
                    serde_json::to_value(summary).unwrap_or_default(),
                );
            })
            .await;
        }
        Err(_) => {
            if let Some(lease) = lease {
                observe(
                    svc,
                    lease.account,
                    None,
                    lease.warnings,
                    Some(vec![(lease.machine_id, None)]),
                )
                .await;
            }
        }
    }
}

fn merge_warnings(
    lease: WireCloudCreditWarnings,
    usage: Option<WireCloudCreditWarnings>,
) -> WireCloudCreditWarnings {
    let Some(usage) = usage else { return lease };
    match lease.epoch.cmp(&usage.epoch) {
        std::cmp::Ordering::Greater => lease,
        std::cmp::Ordering::Less => usage,
        std::cmp::Ordering::Equal => WireCloudCreditWarnings {
            epoch: lease.epoch,
            pct80_at: lease.pct80_at.max(usage.pct80_at),
            pct95_at: lease.pct95_at.max(usage.pct95_at),
            exhausted_at: lease.exhausted_at.max(usage.exhausted_at),
        },
    }
}

pub(super) async fn observe_usage(
    svc: &codeterm_service_core::service::ServiceState,
    account: &str,
    reply: &CloudUsageReply,
    lease_machine: Option<&str>,
) {
    if let Some(warnings) = reply.warnings {
        let machines: Option<Vec<_>> = reply.storage.as_ref().map(|storage| {
            storage
                .machines
                .iter()
                .filter(|machine| {
                    !matches!(
                        machine.tier,
                        WireCloudStorageTier::Purged | WireCloudStorageTier::RecoverableDeleted
                    )
                })
                .map(|machine| (machine.machine_id.clone(), machine.name.clone()))
                .collect()
        });
        let machines = machines
            .filter(|machines| !machines.is_empty())
            .or_else(|| lease_machine.map(|id| vec![(id.to_owned(), None)]));
        observe(
            svc,
            account.to_owned(),
            Some(reply.remaining),
            warnings,
            machines,
        )
        .await;
    }
}

async fn observe(
    svc: &codeterm_service_core::service::ServiceState,
    account: String,
    remaining: Option<f64>,
    warnings: WireCloudCreditWarnings,
    machines: Option<Vec<(String, Option<String>)>>,
) {
    let svc = svc.clone();
    let _ = tokio::task::spawn_blocking(move || {
        svc.observe_cloud_hours(&account, remaining, warnings, machines);
    })
    .await;
}

#[cfg(test)]
mod tests;
