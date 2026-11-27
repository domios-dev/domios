use std::collections::{HashMap, HashSet};

use codeterm_account::account_mesh::wire::{
    BatchRemoveRowStatus, Eligibility, MeshManagementRow, RegistryPresence,
};
use serde::Serialize;

use super::remove::{disambiguated_labels, is_row_online};

pub const SECONDS_PER_DAY: u64 = 86_400;
/// Unreachable machines without an explicit `--older-than` flag are considered stale after 7 days.
pub const DEFAULT_STALE_UNREACHABLE_DAYS: u64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleReason {
    SignedOut,
    MembershipExpired,
    Unreachable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleCandidate {
    pub row: MeshManagementRow,
    pub label: String,
    pub reason: StaleReason,
    pub last_activity_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoveStaleRowStatus {
    WouldRemove,
    Removed,
    AlreadyRemoved,
    RefusedSelf,
    RefusedOnline,
    NotFound,
}

impl From<BatchRemoveRowStatus> for RemoveStaleRowStatus {
    fn from(status: BatchRemoveRowStatus) -> Self {
        match status {
            BatchRemoveRowStatus::Removed => Self::Removed,
            BatchRemoveRowStatus::AlreadyRemoved => Self::AlreadyRemoved,
            BatchRemoveRowStatus::RefusedSelf => Self::RefusedSelf,
            BatchRemoveRowStatus::RefusedOnline => Self::RefusedOnline,
            BatchRemoveRowStatus::NotFound => Self::NotFound,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RemoveStaleRowView {
    pub installation_id: String,
    pub display_name: Option<String>,
    pub reason: Option<StaleReason>,
    pub status: RemoveStaleRowStatus,
    pub slot_revision: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RemoveStaleResultView {
    pub dry_run: bool,
    pub matched: usize,
    pub removed: usize,
    pub already_removed: usize,
    pub skipped: usize,
    pub rows: Vec<RemoveStaleRowView>,
}

/// Best observed last-activity timestamp (Unix seconds) for a non-online registry row.
pub fn last_activity_at(row: &MeshManagementRow, now: u64) -> u64 {
    let endpoint_at = row.endpoint.as_ref().map_or(0, |ep| ep.expires_at);
    let expired_membership_at = row
        .membership_expires_at
        .filter(|&at| at <= now)
        .unwrap_or(0);
    row.authenticated_at
        .max(endpoint_at)
        .max(expired_membership_at)
}

/// Classifies why a non-online active row is stale, or `None` if it is ineligible (self, revoked, or online).
pub fn classify_stale_reason(
    row: &MeshManagementRow,
    local_installation_id: &str,
    connected_live: bool,
    now: u64,
) -> Option<StaleReason> {
    if row.installation_id == local_installation_id {
        return None;
    }
    if !row.active || row.eligibility == Eligibility::Revoked {
        return None;
    }
    if is_row_online(row, connected_live, now) {
        return None;
    }
    if row.presence == RegistryPresence::SignedOut {
        return Some(StaleReason::SignedOut);
    }
    if row.membership_expires_at.is_some_and(|at| at <= now) {
        return Some(StaleReason::MembershipExpired);
    }
    Some(StaleReason::Unreachable)
}

/// Pure selection model for stale account-mesh installations.
///
/// Never selects `local_installation_id`, already-revoked rows, or online machines.
/// Without `older_than_days`, selects all signed-out and expired rows plus unreachable rows
/// inactive for at least `DEFAULT_STALE_UNREACHABLE_DAYS`.
/// With `older_than_days`, selects any non-online active row whose last activity is at least
/// `older_than_days` days old.
pub fn select_stale_installations(
    rows: &[MeshManagementRow],
    local_installation_id: &str,
    connected_live_ids: &HashSet<String>,
    now: u64,
    older_than_days: Option<u64>,
) -> Vec<StaleCandidate> {
    let labels = disambiguated_labels(rows);
    let default_unreachable_secs = DEFAULT_STALE_UNREACHABLE_DAYS.saturating_mul(SECONDS_PER_DAY);
    let explicit_secs = older_than_days.map(|days| days.saturating_mul(SECONDS_PER_DAY));

    let mut seen = HashMap::new();
    for row in rows {
        seen.insert(row.installation_id.as_str(), row);
    }

    let mut selected = Vec::new();
    for row in seen.values() {
        let connected_live = connected_live_ids.contains(&row.installation_id);
        let Some(reason) = classify_stale_reason(row, local_installation_id, connected_live, now)
        else {
            continue;
        };
        let last_seen = last_activity_at(row, now);
        let age = now.saturating_sub(last_seen);
        let include = match explicit_secs {
            Some(threshold) => age >= threshold,
            None => match reason {
                StaleReason::SignedOut | StaleReason::MembershipExpired => true,
                StaleReason::Unreachable => age >= default_unreachable_secs,
            },
        };
        if !include {
            continue;
        }
        let label = labels
            .get(&row.installation_id)
            .cloned()
            .or_else(|| row.display_name.clone())
            .unwrap_or_else(|| row.installation_id.clone());
        selected.push(StaleCandidate {
            row: (*row).clone(),
            label,
            reason,
            last_activity_at: last_seen,
        });
    }

    selected.sort_by(|a, b| {
        a.last_activity_at
            .cmp(&b.last_activity_at)
            .then_with(|| a.row.installation_id.cmp(&b.row.installation_id))
    });
    selected
}

#[cfg(test)]
mod tests;
