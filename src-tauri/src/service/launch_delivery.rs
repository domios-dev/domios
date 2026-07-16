use sha2::{Digest, Sha256};

use crate::agent::provider::SystemPromptDelivery;
use crate::multiplexer::types::{
    DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
    PendingDeliveryTransitionError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchDeliveryMode {
    SystemPrompt,
    ConfirmedFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingDelivery {
    pub digest: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeliveryError {
    DeliveryUnconfirmed,
    RecordTransition(PendingDeliveryTransitionError),
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeliveryUnconfirmed => f.write_str("DeliveryUnconfirmed"),
            Self::RecordTransition(error) => write!(f, "pending delivery transition: {error:?}"),
        }
    }
}

impl std::error::Error for DeliveryError {}

pub(crate) fn mode_for_descriptor(descriptor: &SystemPromptDelivery) -> LaunchDeliveryMode {
    match descriptor {
        SystemPromptDelivery::CliFlag(_) | SystemPromptDelivery::SessionConfig => {
            LaunchDeliveryMode::SystemPrompt
        }
        SystemPromptDelivery::External => LaunchDeliveryMode::ConfirmedFallback,
    }
}

pub(crate) fn pending_delivery(text: &str) -> PendingDelivery {
    let digest = Sha256::digest(text.as_bytes());
    PendingDelivery {
        digest: format!("{digest:x}"),
        confirmed: false,
    }
}

pub(crate) fn pending_record_for(text: &str) -> PendingDeliveryRecord {
    PendingDeliveryRecord::new(
        pending_delivery(text).digest,
        PendingDeliveryChannel::InteractiveInput,
    )
}

pub(crate) fn next_attempt() -> DeliveryAttemptProvenance {
    let started_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    DeliveryAttemptProvenance {
        attempt_id: uuid::Uuid::new_v4().to_string(),
        started_at_ms,
    }
}

pub(crate) fn begin_fallback_attempt(
    pending: &mut PendingDeliveryRecord,
    attempt: DeliveryAttemptProvenance,
) -> Result<(), DeliveryError> {
    if pending.state() == PendingDeliveryState::Confirmed {
        return Ok(());
    }
    pending
        .replace_attempt(attempt)
        .map_err(DeliveryError::RecordTransition)
}

pub(crate) fn confirm_fallback(
    pending: &mut PendingDeliveryRecord,
    submission_observed: bool,
) -> Result<(), DeliveryError> {
    if pending.state() == PendingDeliveryState::Confirmed {
        return Ok(());
    }
    if !submission_observed {
        return Err(DeliveryError::DeliveryUnconfirmed);
    }
    pending.confirm().map_err(DeliveryError::RecordTransition)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_prefers_system_prompt_when_declared() {
        assert_eq!(
            mode_for_descriptor(&SystemPromptDelivery::CliFlag("--system".into())),
            LaunchDeliveryMode::SystemPrompt
        );
    }

    #[test]
    fn fallback_first_message_confirmed_exactly_once() {
        let mut pending = pending_record_for("task");
        begin_fallback_attempt(
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "first".into(),
                started_at_ms: 1,
            },
        )
        .unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::Pending);
        confirm_fallback(&mut pending, true).unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::Confirmed);
        confirm_fallback(&mut pending, false).unwrap();
    }

    #[test]
    fn fallback_deadline_yields_structured_error() {
        let mut pending = pending_record_for("task");
        begin_fallback_attempt(
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "first".into(),
                started_at_ms: 1,
            },
        )
        .unwrap();
        assert_eq!(
            confirm_fallback(&mut pending, false),
            Err(DeliveryError::DeliveryUnconfirmed)
        );
        assert_eq!(pending.state(), PendingDeliveryState::Pending);
        assert_eq!(pending.attempt_provenance().unwrap().attempt_id, "first");
    }

    #[test]
    fn fallback_reconnect_never_resends_confirmed() {
        let mut pending = pending_record_for("task");
        pending.confirm().unwrap();
        assert!(confirm_fallback(&mut pending, false).is_ok());
    }

    #[test]
    fn fallback_retry_after_failure_redelivers_once() {
        let mut pending = pending_record_for("task");
        begin_fallback_attempt(
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "first".into(),
                started_at_ms: 1,
            },
        )
        .unwrap();
        assert_eq!(
            confirm_fallback(&mut pending, false),
            Err(DeliveryError::DeliveryUnconfirmed)
        );
        begin_fallback_attempt(
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "retry".into(),
                started_at_ms: 2,
            },
        )
        .unwrap();
        assert_eq!(pending.attempt_provenance().unwrap().attempt_id, "retry");
        confirm_fallback(&mut pending, true).unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::Confirmed);
    }
}
