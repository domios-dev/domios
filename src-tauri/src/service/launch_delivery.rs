use sha2::{Digest, Sha256};

use crate::agent::provider::SystemPromptDelivery;

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
    SubmitNeverConfirmed,
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SubmitNeverConfirmed => f.write_str("SubmitNeverConfirmed"),
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

pub(crate) fn confirm_fallback(
    pending: &mut PendingDelivery,
    submission_observed: bool,
) -> Result<(), DeliveryError> {
    if pending.confirmed || submission_observed {
        pending.confirmed = true;
        return Ok(());
    }
    Err(DeliveryError::SubmitNeverConfirmed)
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
        let mut pending = pending_delivery("task");
        confirm_fallback(&mut pending, true).unwrap();
        assert!(pending.confirmed);
        confirm_fallback(&mut pending, false).unwrap();
    }

    #[test]
    fn fallback_deadline_yields_structured_error() {
        let mut pending = pending_delivery("task");
        assert_eq!(
            confirm_fallback(&mut pending, false),
            Err(DeliveryError::SubmitNeverConfirmed)
        );
        assert!(!pending.confirmed);
    }

    #[test]
    fn fallback_reconnect_never_resends_confirmed() {
        let mut pending = pending_delivery("task");
        pending.confirmed = true;
        assert!(confirm_fallback(&mut pending, false).is_ok());
    }

    #[test]
    fn fallback_retry_after_failure_redelivers_once() {
        let mut pending = pending_delivery("task");
        assert_eq!(
            confirm_fallback(&mut pending, false),
            Err(DeliveryError::SubmitNeverConfirmed)
        );
        confirm_fallback(&mut pending, true).unwrap();
        assert!(pending.confirmed);
    }
}
