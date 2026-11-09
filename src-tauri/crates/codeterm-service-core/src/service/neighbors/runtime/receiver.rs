use super::*;
use codeterm_neighbors::adapter::ReceiverStatus;
use std::collections::BTreeSet;
pub(super) mod friends;
pub(super) mod selection;
mod startup;
mod stop;

impl ServiceContext {
    /// An installation whose signed membership expired receives nothing until Domios renews it there, so it is never shown receiving.
    pub(super) fn receiver_membership_expired(
        status: &ReceiverStatus,
        lapsed: &BTreeSet<String>,
    ) -> bool {
        !status.is_me
            && status.receiving_enabled
            && status
                .receiver_installation_id
                .as_ref()
                .is_some_and(|id| lapsed.contains(id))
    }

    #[cfg(test)]
    pub(super) fn receiver_view(status: &ReceiverStatus, lapsed: &BTreeSet<String>) -> Value {
        json!(Self::receiver_state(status, lapsed))
    }

    pub(super) fn receiver_state(
        status: &ReceiverStatus,
        lapsed: &BTreeSet<String>,
    ) -> codeterm_neighbors::responses::ReceiverState {
        let expired = Self::receiver_membership_expired(status, lapsed);
        codeterm_neighbors::responses::ReceiverState {
            receiving_here: status.receiving_here(),
            receiver_machine_label: status.receiver_machine_label.clone(),
            receiving_enabled: status.receiving_enabled && !expired,
            revision: status.receiver_revision,
            is_me: status.is_me,
            not_receiving_reason: expired
                .then_some(codeterm_neighbors::responses::NotReceivingReason::MembershipExpired),
        }
    }

    /// The account installations `mesh peers` reports as Membership expired.
    pub(super) fn neighbor_lapsed_installations(&self) -> BTreeSet<String> {
        let Some(mesh) = self.try_ext::<crate::MeshStateHandle>() else {
            return BTreeSet::new();
        };
        let installations = mesh.lock().peers_response().installations;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        lapsed_installations(&installations, now)
    }
}

pub(super) fn lapsed_installations(
    installations: &[codeterm_mesh::types::AccountInstallation],
    now: u64,
) -> BTreeSet<String> {
    installations
        .iter()
        .filter(|row| {
            row.active && !row.signed_out && row.membership_expires_at.is_some_and(|at| at <= now)
        })
        .map(|row| row.installation_id.clone())
        .collect()
}

#[cfg(test)]
mod tests;
