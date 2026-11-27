use std::collections::HashSet;

use super::state::{transition, LifecycleEvent};
use super::storage::CloudStorageTier;
use super::types::{
    CloudError, CloudLifecycleState, CloudMachineRecord, CloudUsageSummary, PendingProvisioningOp,
};

pub const DEFAULT_CREATE_MATCH_WINDOW_MS: u64 = 10 * 60_000;
pub const DEFAULT_UNCERTAIN_CREATE_GRACE_MS: u64 = 5 * 60_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackendMachineEntry {
    pub provider_machine_id: String,
    pub create_request_id: Option<String>,
    pub name: Option<String>,
    pub created_at_ms: Option<u64>,
    pub provider: Option<String>,
    pub size: Option<String>,
    pub state: Option<CloudLifecycleState>,
    pub storage_tier: Option<CloudStorageTier>,
    pub stopped_at_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalReconcilePolicy {
    pub name_match_window_ms: u64,
    pub uncertain_create_grace_ms: u64,
}

impl Default for LocalReconcilePolicy {
    fn default() -> Self {
        Self {
            name_match_window_ms: DEFAULT_CREATE_MATCH_WINDOW_MS,
            uncertain_create_grace_ms: DEFAULT_UNCERTAIN_CREATE_GRACE_MS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalRecordReconcileIntent {
    /// Reconcile an uncertain create (post-write timeout or startup/periodic hygiene).
    ReconcileCreate,
    /// Reconcile a local-only record before executing an owner-confirmed delete.
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendMatchReason {
    CreateRequestId,
    NameAndTimeWindow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LocalRecordReconcileAction {
    /// Record already has a confirmed backend id.
    AlreadyBound,
    /// Matched a backend machine unambiguously; adopt its backend id.
    Adopt {
        entry: BackendMachineEntry,
        reason: BackendMatchReason,
    },
    /// No matching backend machine exists and caller confirmed deletion: remove locally only.
    DeleteLocal,
    /// No matching backend machine exists yet and still inside the create grace window.
    KeepUncertain,
    /// No matching backend machine exists and the uncertain-create grace window has elapsed.
    CleanUpLocal,
    /// Multiple candidates (or competing local-only records) matched; refuse silent adoption.
    Ambiguous { candidate_backend_ids: Vec<String> },
}

pub fn backend_machines_from_usage(usage: &CloudUsageSummary) -> Vec<BackendMachineEntry> {
    let Some(storage) = usage.storage.as_ref() else {
        return Vec::new();
    };
    storage
        .machines
        .iter()
        .filter(|m| !m.machine_id.trim().is_empty())
        .map(|m| BackendMachineEntry {
            provider_machine_id: m.machine_id.trim().to_owned(),
            create_request_id: m
                .create_request_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            name: m
                .name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            created_at_ms: m.created_at_ms,
            provider: None,
            size: None,
            state: None,
            storage_tier: Some(m.tier),
            stopped_at_ms: m.stopped_at,
        })
        .collect()
}

fn create_anchor_ms(record: &CloudMachineRecord) -> u64 {
    record
        .pending_request
        .as_ref()
        .filter(|p| p.op == PendingProvisioningOp::Create)
        .map(|p| p.started_at_ms)
        .unwrap_or(record.created_at_ms)
}

fn matches_name_and_window(
    record: &CloudMachineRecord,
    entry: &BackendMachineEntry,
    window_ms: u64,
) -> bool {
    let rec_name = record.name.trim();
    if rec_name.is_empty() {
        return false;
    }
    if let (Some(rec_req), Some(entry_req)) = (
        record.create_request_id(),
        entry
            .create_request_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    ) {
        if rec_req != entry_req {
            return false;
        }
    }
    let name_ok = entry
        .name
        .as_deref()
        .map(str::trim)
        .is_some_and(|n| !n.is_empty() && n.eq_ignore_ascii_case(rec_name));
    if !name_ok {
        return false;
    }
    let size_ok = entry
        .size
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none_or(|s| s.eq_ignore_ascii_case(&record.size));
    if !size_ok {
        return false;
    }
    let anchor_ms = create_anchor_ms(record);
    entry.created_at_ms.is_some_and(|ts| {
        ts.abs_diff(anchor_ms) <= window_ms || ts.abs_diff(record.created_at_ms) <= window_ms
    })
}

/// Pure reconciliation decision for a local cloud record against the backend machine list.
pub fn plan_local_record_reconcile(
    record: &CloudMachineRecord,
    all_records: &[CloudMachineRecord],
    backend_machines: &[BackendMachineEntry],
    now_ms: u64,
    intent: LocalRecordReconcileIntent,
    policy: LocalReconcilePolicy,
) -> LocalRecordReconcileAction {
    if record.provider_machine_id().is_some() {
        return LocalRecordReconcileAction::AlreadyBound;
    }

    let claimed_backend_ids: HashSet<&str> = all_records
        .iter()
        .filter(|rec| {
            rec.state != CloudLifecycleState::Deleted && rec.machine_id != record.machine_id
        })
        .filter_map(CloudMachineRecord::provider_machine_id)
        .collect();

    let candidates: Vec<&BackendMachineEntry> = backend_machines
        .iter()
        .filter(|entry| {
            let id = entry.provider_machine_id.trim();
            !id.is_empty()
                && !claimed_backend_ids.contains(id)
                && entry.storage_tier != Some(CloudStorageTier::Purged)
                && entry.state != Some(CloudLifecycleState::Deleted)
        })
        .collect();

    // Priority 1: exact creation request id match.
    if let Some(req_id) = record.create_request_id() {
        let req_matches: Vec<&BackendMachineEntry> = candidates
            .iter()
            .copied()
            .filter(|entry| {
                entry
                    .create_request_id
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|id| id == req_id)
            })
            .collect();
        match req_matches.as_slice() {
            [single] => {
                return LocalRecordReconcileAction::Adopt {
                    entry: (*single).clone(),
                    reason: BackendMatchReason::CreateRequestId,
                };
            }
            [] => {}
            many => {
                return LocalRecordReconcileAction::Ambiguous {
                    candidate_backend_ids: many
                        .iter()
                        .map(|m| m.provider_machine_id.trim().to_owned())
                        .collect(),
                };
            }
        }
    }

    // Priority 2: exact name + creation time window match, only if unambiguous on both sides.
    let name_matches: Vec<&BackendMachineEntry> = candidates
        .iter()
        .copied()
        .filter(|entry| matches_name_and_window(record, entry, policy.name_match_window_ms))
        .collect();
    match name_matches.as_slice() {
        [single] => {
            let competing_local = all_records.iter().any(|other| {
                other.state != CloudLifecycleState::Deleted
                    && other.machine_id != record.machine_id
                    && other.provider_machine_id().is_none()
                    && matches_name_and_window(other, single, policy.name_match_window_ms)
            });
            if competing_local {
                return LocalRecordReconcileAction::Ambiguous {
                    candidate_backend_ids: vec![single.provider_machine_id.trim().to_owned()],
                };
            }
            return LocalRecordReconcileAction::Adopt {
                entry: (*single).clone(),
                reason: BackendMatchReason::NameAndTimeWindow,
            };
        }
        [] => {}
        many => {
            return LocalRecordReconcileAction::Ambiguous {
                candidate_backend_ids: many
                    .iter()
                    .map(|m| m.provider_machine_id.trim().to_owned())
                    .collect(),
            };
        }
    }

    // Priority 3: nothing matched on the backend.
    match intent {
        LocalRecordReconcileIntent::Delete => LocalRecordReconcileAction::DeleteLocal,
        LocalRecordReconcileIntent::ReconcileCreate => {
            let anchor_ms = create_anchor_ms(record);
            if now_ms >= anchor_ms.saturating_add(policy.uncertain_create_grace_ms) {
                LocalRecordReconcileAction::CleanUpLocal
            } else {
                LocalRecordReconcileAction::KeepUncertain
            }
        }
    }
}

/// Apply an unambiguous backend match to a local-only record without marking it `Failed`.
pub fn apply_local_record_adoption(
    record: &CloudMachineRecord,
    entry: &BackendMachineEntry,
    _now_ms: u64,
) -> Result<CloudMachineRecord, CloudError> {
    let provider_id = entry.provider_machine_id.trim();
    if provider_id.is_empty() {
        return Err(CloudError::Internal(
            "cannot adopt empty backend machine id".into(),
        ));
    }
    let mut next = record.clone();
    next.ownership.provider_machine_id = provider_id.to_owned();
    if let Some(provider) = entry
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        next.provider = provider.to_owned();
    }
    if let Some(size) = entry
        .size
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        next.size = size.to_owned();
    }
    next.pending_request = None;
    next.last_error = None;

    let stopped = entry.stopped_at_ms.is_some()
        || entry.state == Some(CloudLifecycleState::Asleep)
        || matches!(
            entry.storage_tier,
            Some(
                CloudStorageTier::Cold
                    | CloudStorageTier::Evicting
                    | CloudStorageTier::RecoverableDeleted
            )
        );
    if matches!(
        record.state,
        CloudLifecycleState::Creating | CloudLifecycleState::Failed
    ) {
        next.state = if entry.state == Some(CloudLifecycleState::OutOfCredits) {
            CloudLifecycleState::OutOfCredits
        } else if stopped {
            CloudLifecycleState::Asleep
        } else {
            CloudLifecycleState::Enrolling
        };
    }
    Ok(next)
}

/// Transition a local-only record to `Deleted` locally when nothing exists on the backend.
pub fn apply_local_record_delete(
    record: &CloudMachineRecord,
    now_ms: u64,
) -> Result<CloudMachineRecord, CloudError> {
    let mut next = transition(record, LifecycleEvent::Deleted, now_ms)?;
    next.pending_request = None;
    next.last_error = None;
    Ok(next)
}
