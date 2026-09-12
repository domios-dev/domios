//! Execution-ended and diary-finalization are two different facts about one
//! dreaming run. Each is tied to the worker / run / diary identity that
//! produced it, and neither is ever derived from prose in a record body.

use serde::{Deserialize, Serialize};

/// Why a worker's execution ended. Says nothing about its artifact.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionEnd {
    /// The worker itself reported that the run ended.
    Reported,
    /// The process ended without an explicit report.
    ProcessEnded { detail: String },
    /// The run hit its wall-clock deadline.
    TimedOut { detail: String },
    /// An operator or a revocation cancelled the run.
    Cancelled,
    /// The daemon restarted while the run was live, so the process is gone.
    Orphaned,
}

impl ExecutionEnd {
    pub fn is_reported(&self) -> bool {
        matches!(self, Self::Reported)
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::ProcessEnded { detail } | Self::TimedOut { detail } => Some(detail.as_str()),
            _ => None,
        }
    }
}

/// What a worker claimed about its own diary. A claim is an input to
/// reconciliation, never a conclusion.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinalizationClaim {
    Succeeded,
    Partial,
    Failed,
}

impl FinalizationClaim {
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "succeeded" | "success" | "ok" => Some(Self::Succeeded),
            "partial" => Some(Self::Partial),
            "failed" | "failure" | "error" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Durable incompleteness evidence read back from a stored diary record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiaryEvidence {
    pub diary_id: String,
    /// The insight block is still the skeleton placeholder, or is empty.
    pub placeholder_insight: bool,
    /// Log field names whose value is still a pending / not-run / timeout marker.
    pub pending_fields: Vec<String>,
}

impl DiaryEvidence {
    pub fn is_complete(&self) -> bool {
        !self.placeholder_insight && self.pending_fields.is_empty()
    }

    /// Concrete, actionable description of what is missing. Invents nothing.
    pub fn describe_incompleteness(&self) -> String {
        let mut parts = Vec::new();
        if self.placeholder_insight {
            parts.push("insight is still the skeleton placeholder".to_string());
        }
        if !self.pending_fields.is_empty() {
            parts.push(format!(
                "pending fields: {}",
                self.pending_fields.join(", ")
            ));
        }
        if parts.is_empty() {
            return format!("diary {} is complete", self.diary_id);
        }
        format!("diary {} incomplete — {}", self.diary_id, parts.join("; "))
    }
}

/// Leading token the REM skeleton diary writes before any real insight exists.
const PLACEHOLDER_INSIGHT_TOKEN: &str = "wip";
/// Log-field values that mean "this stage produced no verified result".
const PENDING_FIELD_VALUES: [&str; 4] = ["pending", "not_run", "not run", "timeout"];

/// Read incompleteness evidence out of a stored diary body.
///
/// Placeholder and pending markers prove a record is *incomplete*. They never
/// imply the owning run is live — liveness comes from the worker registry.
pub fn diary_evidence(diary_id: &str, body: &str) -> DiaryEvidence {
    DiaryEvidence {
        diary_id: diary_id.to_string(),
        placeholder_insight: insight_is_placeholder(body),
        pending_fields: pending_log_fields(body),
    }
}

fn tagged_block(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = body.find(&open)? + open.len();
    let end = body[start..].find(&close)? + start;
    Some(body[start..end].to_string())
}

fn insight_is_placeholder(body: &str) -> bool {
    let Some(insight) = tagged_block(body, "insight") else {
        // No insight block: the whole body is the insight, so only an empty
        // body reads as a placeholder.
        return body.trim().is_empty();
    };
    let text = insight.trim();
    if text.is_empty() {
        return true;
    }
    // Only a leading WIP token marks the skeleton; prose that merely mentions
    // dreaming is a real insight.
    text.split_whitespace()
        .next()
        .map(|token| token.trim_end_matches([':', '-', '.']).to_ascii_lowercase())
        .is_some_and(|token| token == PLACEHOLDER_INSIGHT_TOKEN)
}

fn pending_log_fields(body: &str) -> Vec<String> {
    let Some(log) = tagged_block(body, "log") else {
        return Vec::new();
    };
    let mut pending = Vec::new();
    for line in log.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            continue;
        }
        let value = value.trim();
        let lowered = value.to_ascii_lowercase();
        let unfilled = value.is_empty()
            || value.starts_with('<')
            || PENDING_FIELD_VALUES.iter().any(|marker| &lowered == marker);
        if unfilled && !pending.iter().any(|existing| existing == name) {
            pending.push(name.to_string());
        }
    }
    pending
}

/// The diary-finalization fact stored against one worker run.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DiaryFinalization {
    /// No finalization evidence has been recorded for this run.
    #[default]
    Pending,
    Succeeded {
        diary_id: String,
    },
    Partial {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diary_id: Option<String>,
        reason: String,
    },
    Failed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diary_id: Option<String>,
        reason: String,
    },
}

impl DiaryFinalization {
    pub fn diary_id(&self) -> Option<&str> {
        match self {
            Self::Pending => None,
            Self::Succeeded { diary_id } => Some(diary_id.as_str()),
            Self::Partial { diary_id, .. } | Self::Failed { diary_id, .. } => diary_id.as_deref(),
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Partial { reason, .. } | Self::Failed { reason, .. } => Some(reason.as_str()),
            _ => None,
        }
    }

    pub fn is_verified(&self) -> bool {
        matches!(self, Self::Succeeded { .. })
    }
}

/// Reconcile one run's finalization from its explicit claim and the durable
/// evidence in the diary it named.
///
/// A claim never outranks the stored record: a `Succeeded` claim over a diary
/// that still holds placeholder or pending fields reconciles to `Partial` with
/// the concrete evidence as its reason. A self-reported failure is never
/// upgraded, and no final content is invented for a record that lacks it.
pub fn reconcile_finalization(
    claim: Option<FinalizationClaim>,
    claimed_reason: Option<&str>,
    evidence: Option<&DiaryEvidence>,
) -> DiaryFinalization {
    let claimed = |fallback: &str| -> String {
        claimed_reason
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| fallback.to_string())
    };

    match (claim, evidence) {
        (Some(FinalizationClaim::Failed), evidence) => DiaryFinalization::Failed {
            diary_id: evidence.map(|e| e.diary_id.clone()),
            reason: with_evidence(&claimed("worker reported finalization failed"), evidence),
        },
        (claim, Some(evidence)) if !evidence.is_complete() => DiaryFinalization::Partial {
            diary_id: Some(evidence.diary_id.clone()),
            reason: {
                // Keep what the run said AND what the record shows; a
                // contradicted success claim is itself part of the reason.
                let base = match claim {
                    Some(FinalizationClaim::Succeeded) => format!(
                        "{}; reported as finalized but the stored record disagrees",
                        claimed("finalization not verified")
                    ),
                    _ => claimed("finalization did not complete"),
                };
                with_evidence(&base, Some(evidence))
            },
        },
        (Some(FinalizationClaim::Partial), evidence) => DiaryFinalization::Partial {
            diary_id: evidence.map(|e| e.diary_id.clone()),
            reason: claimed("worker reported partial finalization"),
        },
        (_, Some(evidence)) => DiaryFinalization::Succeeded {
            diary_id: evidence.diary_id.clone(),
        },
        (Some(FinalizationClaim::Succeeded), None) => DiaryFinalization::Failed {
            diary_id: None,
            reason: "finalization was reported without a diary record to verify it".to_string(),
        },
        (None, None) => DiaryFinalization::Pending,
    }
}

fn with_evidence(reason: &str, evidence: Option<&DiaryEvidence>) -> String {
    match evidence {
        Some(evidence) if !evidence.is_complete() => {
            format!("{reason} ({})", evidence.describe_incompleteness())
        }
        _ => reason.to_string(),
    }
}

/// What a dreaming run is truthfully reporting right now.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DreamRunStatus {
    /// Execution has not ended. Derived from the worker registry only.
    Running,
    /// Execution ended; no finalization evidence exists for this run.
    FinalizationPending,
    Finalized {
        diary_id: String,
    },
    PartiallyFinalized {
        reason: String,
    },
    FinalizationFailed {
        reason: String,
    },
}

/// Compose the surfaced status out of the two independent facts. Ending
/// execution — by report, exit, timeout, cancel or orphan — never finalizes an
/// artifact on its own, so a run whose execution ended without finalization
/// evidence surfaces as pending rather than as a completion.
pub fn run_status(execution_ended: bool, finalization: &DiaryFinalization) -> DreamRunStatus {
    if !execution_ended {
        return DreamRunStatus::Running;
    }
    match finalization {
        DiaryFinalization::Pending => DreamRunStatus::FinalizationPending,
        DiaryFinalization::Succeeded { diary_id } => DreamRunStatus::Finalized {
            diary_id: diary_id.clone(),
        },
        DiaryFinalization::Partial { reason, .. } => DreamRunStatus::PartiallyFinalized {
            reason: reason.clone(),
        },
        DiaryFinalization::Failed { reason, .. } => DreamRunStatus::FinalizationFailed {
            reason: reason.clone(),
        },
    }
}

/// How one stored diary record should be presented.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DiaryRecordState {
    Complete,
    /// Incomplete, and a live run owns it by worker/diary identity.
    IncompleteLive {
        worker_id: String,
    },
    /// Incomplete with no live owner: recoverable history, not live work.
    IncompleteRecoverable {
        reason: String,
    },
}

impl DiaryRecordState {
    pub fn is_live(&self) -> bool {
        matches!(self, Self::IncompleteLive { .. })
    }
}

/// Classify a stored diary record.
///
/// `live_owner` must be resolved from the worker registry by diary identity.
/// Placeholder prose in the body is evidence of incompleteness and is never
/// evidence that a run is live.
pub fn classify_diary_record(
    evidence: &DiaryEvidence,
    live_owner: Option<&str>,
) -> DiaryRecordState {
    if evidence.is_complete() {
        return DiaryRecordState::Complete;
    }
    match live_owner {
        Some(worker_id) => DiaryRecordState::IncompleteLive {
            worker_id: worker_id.to_string(),
        },
        None => DiaryRecordState::IncompleteRecoverable {
            reason: evidence.describe_incompleteness(),
        },
    }
}
