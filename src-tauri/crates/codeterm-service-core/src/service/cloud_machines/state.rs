use std::collections::BTreeMap;

use super::idle_drain::IdleSleepAction;
use super::storage::CloudStorageTier;
use super::types::{
    CloudError, CloudLifecycleState, CloudMachineRecord, CloudOwnershipReceipt, CloudRate,
    CloudSleepMode, CloudUsageSummary,
};

pub const LABEL_MANAGED: &str = "domios.managed";
pub const LABEL_CONTROLLER: &str = "domios.controller";
pub const LABEL_MACHINE_ID: &str = "domios.machine_id";
pub const LABEL_NAME: &str = "domios.name";
pub const LABEL_SIZE: &str = "domios.size";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleEvent {
    ProviderCreated {
        provider_machine_id: String,
    },
    EnrollmentApplyUnconfirmed {
        installation_id: String,
    },
    EnrolledReady {
        installation_id: String,
        peer_instance_id: Option<String>,
    },
    DrainStarted,
    DrainAbortedBusy,
    ProviderStopped,
    WakeRequested,
    WakeReady,
    WakeFailed(String),
    CreditsExhausted,
    CreditsRestored,
    Failed(String),
    Deleted,
}

impl LifecycleEvent {
    fn name(&self) -> &'static str {
        match self {
            Self::ProviderCreated { .. } => "provider_created",
            Self::EnrollmentApplyUnconfirmed { .. } => "enrollment_apply_unconfirmed",
            Self::EnrolledReady { .. } => "enrolled_ready",
            Self::DrainStarted => "drain_started",
            Self::DrainAbortedBusy => "drain_aborted_busy",
            Self::ProviderStopped => "provider_stopped",
            Self::WakeRequested => "wake_requested",
            Self::WakeReady => "wake_ready",
            Self::WakeFailed(_) => "wake_failed",
            Self::CreditsExhausted => "credits_exhausted",
            Self::CreditsRestored => "credits_restored",
            Self::Failed(_) => "failed",
            Self::Deleted => "deleted",
        }
    }
}

fn settle_awake_interval(record: &mut CloudMachineRecord, now_ms: u64) {
    if let Some(since) = record.last_awake_at_ms.take() {
        let delta = now_ms.saturating_sub(since) / 1_000;
        record.accumulated_awake_secs = record.accumulated_awake_secs.saturating_add(delta);
    }
}

pub fn transition(
    record: &CloudMachineRecord,
    event: LifecycleEvent,
    now_ms: u64,
) -> Result<CloudMachineRecord, CloudError> {
    use CloudLifecycleState::*;
    let mut next = record.clone();
    match (record.state, &event) {
        (
            Creating,
            LifecycleEvent::ProviderCreated {
                provider_machine_id,
            },
        ) => {
            next.ownership.provider_machine_id = provider_machine_id.clone();
            next.state = Enrolling;
            next.last_error = None;
        }
        (Enrolling, LifecycleEvent::EnrollmentApplyUnconfirmed { installation_id }) => {
            next.installation_id = Some(installation_id.clone());
            next.apply_unconfirmed = true;
            next.last_error = Some("enrollment_apply_unconfirmed".into());
        }
        (
            Enrolling,
            LifecycleEvent::EnrolledReady {
                installation_id,
                peer_instance_id,
            },
        ) => {
            next.installation_id = Some(installation_id.clone());
            if peer_instance_id.is_some() {
                next.peer_instance_id = peer_instance_id.clone();
            }
            next.apply_unconfirmed = false;
            next.state = Awake;
            next.last_awake_at_ms = Some(now_ms);
            next.last_owner_interaction_at_ms = Some(now_ms);
            next.idle_since_ms = None;
            next.exhaustion_pending = false;
            next.last_error = None;
        }
        (Awake, LifecycleEvent::DrainStarted) => {
            next.state = Draining;
        }
        (Draining, LifecycleEvent::DrainAbortedBusy) => {
            next.state = Awake;
            next.idle_since_ms = None;
        }
        (Awake | Draining, LifecycleEvent::ProviderStopped) => {
            settle_awake_interval(&mut next, now_ms);
            next.state = Asleep;
            next.lease_id = None;
            next.lease_expires_at_ms = None;
            next.exhaustion_pending = false;
            next.idle_since_ms = None;
            next.last_error = None;
        }
        (Asleep | Failed, LifecycleEvent::WakeRequested) => {
            next.state = Waking;
            next.last_error = None;
        }
        (Waking, LifecycleEvent::WakeReady) => {
            next.state = Awake;
            next.last_awake_at_ms = Some(now_ms);
            next.last_owner_interaction_at_ms = Some(now_ms);
            next.idle_since_ms = None;
            next.exhaustion_pending = false;
            next.last_error = None;
        }
        (Waking, LifecycleEvent::WakeFailed(reason)) => {
            next.state = Asleep;
            next.last_error = Some(reason.clone());
        }
        (Awake | Draining | Asleep | Waking, LifecycleEvent::CreditsExhausted) => {
            settle_awake_interval(&mut next, now_ms);
            next.state = OutOfCredits;
            next.lease_id = None;
            next.lease_expires_at_ms = None;
            next.exhaustion_pending = false;
            next.idle_since_ms = None;
            next.last_error = Some("Out of credits: top up to wake".into());
        }
        (OutOfCredits, LifecycleEvent::CreditsRestored) => {
            next.state = Asleep;
            next.last_error = None;
        }
        (
            Creating | Enrolling | Awake | Draining | Asleep | Waking,
            LifecycleEvent::Failed(reason),
        ) => {
            settle_awake_interval(&mut next, now_ms);
            next.state = Failed;
            next.idle_since_ms = None;
            next.last_error = Some(reason.clone());
        }
        (state, LifecycleEvent::Deleted) if state != Deleted => {
            settle_awake_interval(&mut next, now_ms);
            next.state = Deleted;
            next.lease_id = None;
            next.lease_expires_at_ms = None;
            next.exhaustion_pending = false;
            next.idle_since_ms = None;
            next.last_error = None;
        }
        (from, ev) => {
            return Err(CloudError::InvalidTransition {
                from,
                event: ev.name(),
            });
        }
    }
    Ok(next)
}

pub fn can_issue_enrollment_apply(record: &CloudMachineRecord) -> bool {
    record.state == CloudLifecycleState::Enrolling && !record.apply_unconfirmed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendDrainingTruth {
    Awake,
    Asleep,
    OutOfCredits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainingReconcileIntent {
    /// Background/supervisor recovery: finish sleeping if the backend is still awake so billing stops.
    FinishDrainOrRecover,
    /// Explicit wake request: keep awake if backend is already awake, otherwise settle before waking.
    PrepareForWake,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainingRecoveryPlan {
    NotDraining,
    FinishAsleep,
    FinishOutOfCredits,
    ResumeSleep,
    ReopenAwake,
}

pub fn plan_draining_recovery(
    state: CloudLifecycleState,
    truth: BackendDrainingTruth,
    intent: DrainingReconcileIntent,
    sleep_action: IdleSleepAction,
) -> DrainingRecoveryPlan {
    if state != CloudLifecycleState::Draining {
        return DrainingRecoveryPlan::NotDraining;
    }
    match truth {
        BackendDrainingTruth::Asleep => DrainingRecoveryPlan::FinishAsleep,
        BackendDrainingTruth::OutOfCredits => DrainingRecoveryPlan::FinishOutOfCredits,
        BackendDrainingTruth::Awake => match intent {
            DrainingReconcileIntent::PrepareForWake => DrainingRecoveryPlan::ReopenAwake,
            DrainingReconcileIntent::FinishDrainOrRecover => match sleep_action {
                IdleSleepAction::KeepUnderLease => DrainingRecoveryPlan::ReopenAwake,
                IdleSleepAction::StopProvider | IdleSleepAction::DeleteForRecreate => {
                    DrainingRecoveryPlan::ResumeSleep
                }
            },
        },
    }
}

pub fn apply_draining_truth(
    record: &CloudMachineRecord,
    truth: BackendDrainingTruth,
    now_ms: u64,
) -> Result<CloudMachineRecord, CloudError> {
    if record.state != CloudLifecycleState::Draining {
        return Err(CloudError::InvalidTransition {
            from: record.state,
            event: "draining_reconcile",
        });
    }
    match truth {
        BackendDrainingTruth::Asleep => {
            let mut next = transition(record, LifecycleEvent::ProviderStopped, now_ms)?;
            next.pending_request = None;
            Ok(next)
        }
        BackendDrainingTruth::OutOfCredits => {
            let mut next = transition(record, LifecycleEvent::CreditsExhausted, now_ms)?;
            next.pending_request = None;
            Ok(next)
        }
        BackendDrainingTruth::Awake => {
            let mut next = transition(record, LifecycleEvent::DrainAbortedBusy, now_ms)?;
            if next.last_awake_at_ms.is_none() {
                next.last_awake_at_ms = Some(now_ms);
            }
            next.exhaustion_pending = false;
            next.pending_request = None;
            next.last_error = None;
            Ok(next)
        }
    }
}

pub fn usage_reports_credits_exhausted(usage: &CloudUsageSummary) -> bool {
    let tracked =
        usage.cap > 0.0 || usage.allowance > 0.0 || usage.purchased > 0.0 || usage.used > 0.0;
    tracked && usage.remaining <= 0.0
}

pub fn infer_backend_truth_from_usage(
    record: &CloudMachineRecord,
    usage: &CloudUsageSummary,
) -> Option<BackendDrainingTruth> {
    let storage = usage.storage.as_ref()?;
    let backend_id = record.backend_machine_id();
    let entry = storage
        .machines
        .iter()
        .find(|m| m.machine_id == backend_id || m.machine_id == record.machine_id)?;
    let stopped = entry.stopped_at.is_some() || !matches!(entry.tier, CloudStorageTier::Hot);
    if !stopped {
        return None;
    }
    if record.exhaustion_pending || usage_reports_credits_exhausted(usage) {
        Some(BackendDrainingTruth::OutOfCredits)
    } else {
        Some(BackendDrainingTruth::Asleep)
    }
}

pub fn ownership_labels(
    controller_id: &str,
    machine_id: &str,
    name: &str,
    size: &str,
    extra: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut labels = extra.clone();
    labels.insert(LABEL_MANAGED.into(), "true".into());
    labels.insert(LABEL_CONTROLLER.into(), controller_id.into());
    labels.insert(LABEL_MACHINE_ID.into(), machine_id.into());
    labels.insert(LABEL_NAME.into(), name.into());
    labels.insert(LABEL_SIZE.into(), size.into());
    labels
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservedProviderState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Paused,
    Deleting,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedProviderMachine {
    pub provider_machine_id: String,
    pub labels: BTreeMap<String, String>,
    pub state: ObservedProviderState,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReconcileOutcome {
    pub records: Vec<CloudMachineRecord>,
    pub adopted_ids: Vec<String>,
    pub updated_ids: Vec<String>,
}

pub fn reconcile_provider_machines(
    stored: &[CloudMachineRecord],
    observed: &[ObservedProviderMachine],
    controller_id: &str,
    provider: &str,
    sleep_mode: CloudSleepMode,
    default_rate: Option<CloudRate>,
    now_ms: u64,
) -> ReconcileOutcome {
    let mut records = stored.to_vec();
    let mut adopted_ids = Vec::new();
    let mut updated_ids = Vec::new();

    for remote in observed {
        if remote.labels.get(LABEL_MANAGED).map(String::as_str) != Some("true")
            || remote.labels.get(LABEL_CONTROLLER).map(String::as_str) != Some(controller_id)
        {
            continue;
        }
        let label_machine_id = remote.labels.get(LABEL_MACHINE_ID).map(String::as_str);
        let existing_idx = records.iter().position(|rec| {
            rec.provider == provider
                && rec.state != CloudLifecycleState::Deleted
                && (rec.ownership.provider_machine_id == remote.provider_machine_id
                    || label_machine_id.is_some_and(|id| rec.machine_id == id))
        });

        let mapped_state = match remote.state {
            ObservedProviderState::Running => CloudLifecycleState::Awake,
            ObservedProviderState::Starting => CloudLifecycleState::Waking,
            ObservedProviderState::Stopping => CloudLifecycleState::Draining,
            ObservedProviderState::Stopped | ObservedProviderState::Paused => {
                CloudLifecycleState::Asleep
            }
            ObservedProviderState::Deleting => CloudLifecycleState::Deleted,
            ObservedProviderState::Failed | ObservedProviderState::Unknown => {
                CloudLifecycleState::Failed
            }
        };

        if let Some(idx) = existing_idx {
            let rec = &mut records[idx];
            if rec.ownership.provider_machine_id.is_empty() {
                rec.ownership.provider_machine_id = remote.provider_machine_id.clone();
            }
            if rec.state == CloudLifecycleState::OutOfCredits {
                continue;
            }
            if rec.state == CloudLifecycleState::Enrolling
                && matches!(
                    remote.state,
                    ObservedProviderState::Running | ObservedProviderState::Starting
                )
            {
                continue;
            }
            if rec.state != mapped_state {
                if rec.state == CloudLifecycleState::Awake
                    && mapped_state != CloudLifecycleState::Awake
                {
                    settle_awake_interval(rec, now_ms);
                }
                if mapped_state == CloudLifecycleState::Awake && rec.last_awake_at_ms.is_none() {
                    rec.last_awake_at_ms = Some(now_ms);
                    if rec.last_owner_interaction_at_ms.is_none() {
                        rec.last_owner_interaction_at_ms = Some(now_ms);
                    }
                }
                rec.state = mapped_state;
                updated_ids.push(rec.machine_id.clone());
            }
        } else if mapped_state != CloudLifecycleState::Deleted {
            let machine_id = label_machine_id
                .map(str::to_owned)
                .unwrap_or_else(|| format!("cloud-{}", remote.provider_machine_id));
            let name = remote
                .labels
                .get(LABEL_NAME)
                .cloned()
                .unwrap_or_else(|| machine_id.clone());
            let size = remote
                .labels
                .get(LABEL_SIZE)
                .cloned()
                .unwrap_or_else(|| "small".into());
            let initial_state = if mapped_state == CloudLifecycleState::Awake {
                CloudLifecycleState::Enrolling
            } else {
                mapped_state
            };
            records.push(CloudMachineRecord {
                machine_id: machine_id.clone(),
                name,
                provider: provider.into(),
                size,
                state: initial_state,
                sleep_mode,
                installation_id: None,
                peer_instance_id: None,
                endpoint: None,
                rate_per_hour: default_rate.clone(),
                created_at_ms: now_ms,
                last_awake_at_ms: (initial_state == CloudLifecycleState::Awake).then_some(now_ms),
                last_owner_interaction_at_ms: (initial_state == CloudLifecycleState::Awake)
                    .then_some(now_ms),
                accumulated_awake_secs: 0,
                period_start_ms: now_ms,
                lease_id: None,
                lease_expires_at_ms: None,
                exhaustion_pending: false,
                idle_since_ms: None,
                apply_unconfirmed: false,
                pending_request: None,
                last_error: None,
                ownership: CloudOwnershipReceipt {
                    provider_machine_id: remote.provider_machine_id.clone(),
                    floating_ip_id: None,
                    auxiliary_resource_ids: BTreeMap::new(),
                    controller_id: controller_id.into(),
                    labels: remote.labels.clone(),
                },
            });
            adopted_ids.push(machine_id);
        }
    }

    ReconcileOutcome {
        records,
        adopted_ids,
        updated_ids,
    }
}
