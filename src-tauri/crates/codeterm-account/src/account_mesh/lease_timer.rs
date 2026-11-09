use super::lifecycle::AccountMesh;
use super::state::{AccountMeshRefusal, EnrollTrigger};
use std::{sync::Arc, time::Duration};

impl AccountMesh {
    pub(super) fn lease_due(&self, result: &Result<(), AccountMeshRefusal>) -> Option<u64> {
        let now = self.now();
        self.with(|inner| {
            if inner.auth_parked.is_some()
                || inner.terminal_parked
                || inner.state.last_refusal == Some(AccountMeshRefusal::ReauthRequired)
            {
                return None;
            }
            if matches!(result, Err(AccountMeshRefusal::EnrollmentBudget { .. })) {
                return inner.retry.deadline;
            }
            let membership_due = inner.membership_renewal_at();
            if membership_due.is_none()
                && matches!(
                    result,
                    Err(AccountMeshRefusal::MachineLimit { .. }
                        | AccountMeshRefusal::Unauthenticated
                        | AccountMeshRefusal::ReauthRequired
                        | AccountMeshRefusal::VerificationUnavailable
                        | AccountMeshRefusal::InstallationUnverified
                        | AccountMeshRefusal::CutoffAfterCurrentSession
                        | AccountMeshRefusal::AppCheck
                        | AccountMeshRefusal::EnrollmentBudget { .. }
                        | AccountMeshRefusal::SignedOut
                        | AccountMeshRefusal::PlanLimitLocked)
                )
            {
                return None;
            }
            let retry_due = if result.is_err() {
                inner.retry.deadline.or(Some(now.saturating_add(300)))
            } else {
                None
            };
            [membership_due, retry_due]
                .into_iter()
                .flatten()
                .map(|due| {
                    if due <= now {
                        retry_due.unwrap_or(now.saturating_add(1))
                    } else {
                        due
                    }
                })
                .min()
        })
    }

    pub(super) fn schedule_lease(
        self: &Arc<Self>,
        epoch: u64,
        result: &Result<(), AccountMeshRefusal>,
    ) {
        if !self.config.schedule_leases {
            return;
        }
        let Some(due) = self.lease_due(result) else {
            return;
        };
        let due = self.with(|inner| {
            if inner.rejoin_required {
                inner
                    .rejoin_window
                    .as_ref()
                    .map_or(due, |window| due.min(window.expires_at()))
            } else {
                due
            }
        });
        let Some(lease) = self.with(|inner| {
            if !inner.is_current(epoch) {
                return None;
            }
            inner.lease += 1;
            Some(inner.lease)
        }) else {
            return;
        };
        let mesh = Arc::downgrade(self);
        let mut closed = self.closed.subscribe();
        if matches!(result, Err(AccountMeshRefusal::EnrollmentBudget { .. })) {
            let remaining = due.saturating_sub(self.now());
            let Some(deadline) =
                tokio::time::Instant::now().checked_add(Duration::from_secs(remaining))
            else {
                return;
            };
            tokio::spawn(async move {
                tokio::select! {
                    _ = closed.changed() => return,
                    _ = tokio::time::sleep_until(deadline) => {}
                }
                let Some(mesh) = mesh.upgrade() else {
                    return;
                };
                if mesh.with(|inner| inner.lease == lease && inner.is_current(epoch)) {
                    let _ = mesh.refresh(EnrollTrigger::Lease).await;
                }
            });
            return;
        }
        tokio::spawn(async move {
            loop {
                let Some(mesh) = mesh.upgrade() else {
                    return;
                };
                if !mesh.with(|inner| inner.lease == lease && inner.is_current(epoch)) {
                    return;
                }
                let remaining = due.saturating_sub(mesh.now());
                if remaining == 0 {
                    if mesh.with(|inner| {
                        if inner.lease != lease || !inner.is_current(epoch) {
                            return false;
                        }
                        inner.retry.deadline = Some(mesh.now());
                        true
                    }) {
                        let _ = mesh.refresh(EnrollTrigger::Lease).await;
                    }
                    return;
                }
                // A local drift check bounds suspend recovery without contacting the registry before due.
                drop(mesh);
                tokio::select! {
                    _ = closed.changed() => return,
                    _ = tokio::time::sleep(Duration::from_secs(remaining.min(60))) => {}
                }
            }
        });
    }
}
