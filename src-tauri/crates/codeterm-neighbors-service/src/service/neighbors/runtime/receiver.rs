use super::*;
use crate::prelude::*;
use codeterm_neighbors::adapter::ReceiverStatus;
use std::collections::BTreeSet;
pub mod friends;
pub mod selection;
pub mod startup;
pub mod stop;

pub trait NeighborsExt {
    fn receiver_membership_expired(status: &ReceiverStatus, lapsed: &BTreeSet<String>) -> bool;
    #[cfg(test)]
    fn receiver_view(status: &ReceiverStatus, lapsed: &BTreeSet<String>) -> Value;
    fn receiver_state(
        status: &ReceiverStatus,
        lapsed: &BTreeSet<String>,
        installations: &[codeterm_mesh::types::AccountInstallation],
    ) -> codeterm_neighbors::responses::ReceiverState;
    fn neighbor_lapsed_installations(&self) -> BTreeSet<String>;
    fn neighbor_receiver_installations(&self) -> Vec<codeterm_mesh::types::AccountInstallation>;
}

impl NeighborsExt for ServiceContext {
    /// An installation whose signed membership expired receives nothing until Domios renews it there, so it is never shown receiving.
    fn receiver_membership_expired(status: &ReceiverStatus, lapsed: &BTreeSet<String>) -> bool {
        !status.is_me
            && status.receiving_enabled
            && status
                .receiver_installation_id
                .as_ref()
                .is_some_and(|id| lapsed.contains(id))
    }

    #[cfg(test)]
    fn receiver_view(status: &ReceiverStatus, lapsed: &BTreeSet<String>) -> Value {
        json!(Self::receiver_state(status, lapsed, &[]))
    }

    fn receiver_state(
        status: &ReceiverStatus,
        lapsed: &BTreeSet<String>,
        installations: &[codeterm_mesh::types::AccountInstallation],
    ) -> codeterm_neighbors::responses::ReceiverState {
        let expired = Self::receiver_membership_expired(status, lapsed);
        let installation = installations.iter().find(|row| {
            Some(row.installation_id.as_str()) == status.receiver_installation_id.as_deref()
        });
        codeterm_neighbors::responses::ReceiverState {
            receiving_here: status.receiving_here(),
            receiver_machine_label: status.receiver_machine_label.clone(),
            receiver_installation_id: status.receiver_installation_id.clone(),
            receiver_instance_id: installation.and_then(|row| row.instance_id.clone()),
            receiver_online: if status.is_me {
                Some(true)
            } else {
                installation.map(|row| row.online && row.active && !row.signed_out && !expired)
            },
            receiving_enabled: status.receiving_enabled && !expired,
            revision: status.receiver_revision,
            is_me: status.is_me,
            not_receiving_reason: expired
                .then_some(codeterm_neighbors::responses::NotReceivingReason::MembershipExpired),
        }
    }

    /// The account installations `mesh peers` reports as Membership expired.
    fn neighbor_lapsed_installations(&self) -> BTreeSet<String> {
        let installations = self.neighbor_receiver_installations();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        lapsed_installations(&installations, now)
    }

    fn neighbor_receiver_installations(&self) -> Vec<codeterm_mesh::types::AccountInstallation> {
        self.try_ext::<crate::MeshStateHandle>()
            .map(|mesh| mesh.lock().peers_response().installations)
            .unwrap_or_default()
    }
}

pub fn lapsed_installations(
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
pub mod tests;
