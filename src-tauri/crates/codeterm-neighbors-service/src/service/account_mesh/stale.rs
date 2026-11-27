use std::collections::{HashMap, HashSet};

use codeterm_account::account_mesh::wire::{
    BatchRemoveRowStatus, Eligibility, MeshManagementRow, RegistryPresence,
};
use codeterm_service_core::service::cloud_machines::{
    CloudLifecycleState, CloudMachineRecord, CloudStorageTier, AUX_KEY_STORAGE_TIER,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleSelection {
    pub candidates: Vec<StaleCandidate>,
    pub excluded_cloud_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoveStaleRowStatus {
    WouldRemove,
    Removed,
    AlreadyRemoved,
    RefusedSelf,
    RefusedOnline,
    RefusedCloud,
    Skipped,
    NotFound,
}

impl RemoveStaleRowStatus {
    pub fn from_batch_row(status: BatchRemoveRowStatus, reason: Option<&str>) -> Self {
        match (status, reason) {
            (BatchRemoveRowStatus::Removed, _) => Self::Removed,
            (BatchRemoveRowStatus::AlreadyRemoved, _) => Self::AlreadyRemoved,
            (BatchRemoveRowStatus::RefusedSelf, _) => Self::RefusedSelf,
            (BatchRemoveRowStatus::RefusedOnline, _) => Self::RefusedOnline,
            (BatchRemoveRowStatus::Skipped, Some("cloud_machine") | None) => Self::RefusedCloud,
            (BatchRemoveRowStatus::Skipped, Some(_)) => Self::Skipped,
            (BatchRemoveRowStatus::NotFound, _) => Self::NotFound,
        }
    }
}

impl From<BatchRemoveRowStatus> for RemoveStaleRowStatus {
    fn from(status: BatchRemoveRowStatus) -> Self {
        Self::from_batch_row(status, None)
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

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RemoveStaleResultView {
    pub dry_run: bool,
    pub matched: usize,
    pub removed: usize,
    pub already_removed: usize,
    pub skipped: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub excluded_cloud_count: usize,
    pub rows: Vec<RemoveStaleRowView>,
}

/// Returns true when a managed cloud machine record is live (any state other than `deleted`
/// and any storage tier other than `purged`), including awake, asleep, cold/archived, draining,
/// waking, creating, enrolling, out-of-credits, and failed records.
pub fn is_live_managed_cloud_record(record: &CloudMachineRecord) -> bool {
    if record.state == CloudLifecycleState::Deleted {
        return false;
    }
    let purged = record
        .ownership
        .auxiliary_resource_ids
        .get(AUX_KEY_STORAGE_TIER)
        .and_then(|raw| CloudStorageTier::from_aux_str(raw))
        == Some(CloudStorageTier::Purged);
    !purged
}

/// Collects every installation identifier backed by a live managed cloud machine record.
/// Uses structural record linkage (`installation_id`, `machine_id`, and verified
/// `(installation_id, peer_instance_id)` bindings); never matches machine names.
pub fn protected_cloud_installation_ids(
    records: &[CloudMachineRecord],
    pair_bindings: &[(String, String)],
) -> HashSet<String> {
    let mut peer_to_installations: HashMap<&str, Vec<&str>> = HashMap::new();
    for (installation_id, peer_instance_id) in pair_bindings {
        let inst = installation_id.trim();
        let peer = peer_instance_id.trim();
        if !inst.is_empty() && !peer.is_empty() {
            peer_to_installations.entry(peer).or_default().push(inst);
        }
    }

    let mut protected = HashSet::new();
    for record in records {
        if !is_live_managed_cloud_record(record) {
            continue;
        }
        if let Some(inst) = record
            .installation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            protected.insert(inst.to_owned());
        }
        let machine_id = record.machine_id.trim();
        if !machine_id.is_empty() {
            protected.insert(machine_id.to_owned());
        }
        if let Some(peer) = record
            .peer_instance_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            protected.insert(peer.to_owned());
            if let Some(installations) = peer_to_installations.get(peer) {
                for &inst in installations {
                    protected.insert(inst.to_owned());
                }
            }
        }
    }
    protected
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

/// Pure selection model for stale account-mesh installations, returning both eligible stale
/// candidates and live managed cloud installations that were excluded from stale cleanup.
///
/// Never selects `local_installation_id`, already-revoked rows, online machines, or any
/// installation in `protected_cloud_ids`.
pub fn select_stale_installations_with_cloud_exclusions(
    rows: &[MeshManagementRow],
    local_installation_id: &str,
    connected_live_ids: &HashSet<String>,
    protected_cloud_ids: &HashSet<String>,
    now: u64,
    older_than_days: Option<u64>,
) -> StaleSelection {
    let labels = disambiguated_labels(rows);
    let default_unreachable_secs = DEFAULT_STALE_UNREACHABLE_DAYS.saturating_mul(SECONDS_PER_DAY);
    let explicit_secs = older_than_days.map(|days| days.saturating_mul(SECONDS_PER_DAY));

    let mut seen = HashMap::new();
    for row in rows {
        seen.insert(row.installation_id.as_str(), row);
    }

    let mut selected = Vec::new();
    let mut excluded_cloud_ids = Vec::new();
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
        if protected_cloud_ids.contains(&row.installation_id) {
            excluded_cloud_ids.push(row.installation_id.clone());
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
    excluded_cloud_ids.sort();
    StaleSelection {
        candidates: selected,
        excluded_cloud_ids,
    }
}

/// Pure selection model for stale account-mesh installations.
///
/// Never selects `local_installation_id`, already-revoked rows, online machines, or live managed
/// cloud machines (`protected_cloud_ids`).
/// Without `older_than_days`, selects all signed-out and expired rows plus unreachable rows
/// inactive for at least `DEFAULT_STALE_UNREACHABLE_DAYS`.
/// With `older_than_days`, selects any non-online active row whose last activity is at least
/// `older_than_days` days old.
pub fn select_stale_installations(
    rows: &[MeshManagementRow],
    local_installation_id: &str,
    connected_live_ids: &HashSet<String>,
    protected_cloud_ids: &HashSet<String>,
    now: u64,
    older_than_days: Option<u64>,
) -> Vec<StaleCandidate> {
    select_stale_installations_with_cloud_exclusions(
        rows,
        local_installation_id,
        connected_live_ids,
        protected_cloud_ids,
        now,
        older_than_days,
    )
    .candidates
}

#[cfg(test)]
mod tests;
