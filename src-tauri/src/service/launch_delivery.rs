use sha2::{Digest, Sha256};

use crate::agent::provider::SystemPromptDelivery;
use crate::multiplexer::types::{
    DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
    PendingDeliveryTransitionError,
};
use crate::service::boot_stage::{BootStage, BootStageSink};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchDeliveryMode {
    SystemPrompt,
    ConfirmedFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchContent {
    pub system_prompt: Option<String>,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingDelivery {
    pub digest: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FallbackDeliverySpec<'a> {
    pub channel: PendingDeliveryChannel,
    pub payload: &'a str,
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

pub(crate) fn content_for_descriptor(
    descriptor: &SystemPromptDelivery,
    content: Option<&str>,
) -> LaunchContent {
    let content = content
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string);
    match mode_for_descriptor(descriptor) {
        LaunchDeliveryMode::SystemPrompt => LaunchContent {
            system_prompt: content,
            fallback: None,
        },
        LaunchDeliveryMode::ConfirmedFallback => LaunchContent {
            system_prompt: None,
            fallback: content,
        },
    }
}

pub(crate) fn pending_delivery(text: &str) -> PendingDelivery {
    let digest = Sha256::digest(text.as_bytes());
    PendingDelivery {
        digest: format!("{digest:x}"),
        confirmed: false,
    }
}

#[cfg(test)]
fn pending_record_for(text: &str) -> PendingDeliveryRecord {
    pending_record_for_spec(FallbackDeliverySpec {
        channel: PendingDeliveryChannel::InteractiveInput,
        payload: text,
    })
}

pub(crate) fn fallback_delivery_spec<'a>(
    fallback_bootstrap: Option<&'a str>,
    task: Option<&'a str>,
) -> Option<FallbackDeliverySpec<'a>> {
    fallback_bootstrap
        .map(|payload| FallbackDeliverySpec {
            channel: PendingDeliveryChannel::FallbackBootstrap,
            payload,
        })
        .or_else(|| {
            task.map(|payload| FallbackDeliverySpec {
                channel: PendingDeliveryChannel::InteractiveInput,
                payload,
            })
        })
}

pub(crate) fn pending_record_for_spec(spec: FallbackDeliverySpec<'_>) -> PendingDeliveryRecord {
    PendingDeliveryRecord::new(pending_delivery(spec.payload).digest, spec.channel)
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

pub(crate) fn dispatch_initial_delivery<T, F>(
    mode: LaunchDeliveryMode,
    sink: &impl BootStageSink,
    pane_id: &str,
    transaction: Option<F>,
) -> Option<T>
where
    F: FnOnce() -> T,
{
    match mode {
        LaunchDeliveryMode::SystemPrompt => None,
        LaunchDeliveryMode::ConfirmedFallback => transaction.map(|transaction| {
            sink.record(pane_id, BootStage::DeliveringInstructions, None);
            transaction()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<&'static str>>);

    impl BootStageSink for RecordingSink {
        fn record(&self, _: &str, stage: BootStage, _: Option<&str>) {
            self.0.lock().push(stage.as_str());
        }
    }

    impl RecordingSink {
        fn record_transaction(&self) {
            self.0.lock().push("transaction");
        }
    }

    #[test]
    fn delivery_prefers_system_prompt_when_declared() {
        assert_eq!(
            mode_for_descriptor(&SystemPromptDelivery::CliFlag("--system".into())),
            LaunchDeliveryMode::SystemPrompt
        );
    }

    #[test]
    fn system_prompt_channel_consumes_launch_content_without_a_fallback_turn() {
        let content = content_for_descriptor(
            &SystemPromptDelivery::CliFlag("--system".into()),
            Some("manager identity"),
        );
        assert_eq!(content.system_prompt.as_deref(), Some("manager identity"));
        assert_eq!(content.fallback, None);
    }

    #[test]
    fn external_provider_defers_identical_content_to_confirmed_fallback() {
        let content =
            content_for_descriptor(&SystemPromptDelivery::External, Some("manager identity"));
        assert_eq!(content.system_prompt, None);
        assert_eq!(content.fallback.as_deref(), Some("manager identity"));
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
    fn pair_fallback_bootstrap_submits_identity_once_without_work() {
        let bootstrap = crate::service::role_identity::ManagerBootstrapInputs {
            role: "Orchestrator".into(),
            team_id: "not yet established".into(),
            team_name: "not yet established".into(),
            plan_id: "no plan bound yet".into(),
            project: "Manager Bootstrap Fixture".into(),
        }
        .compose();
        let delivery = content_for_descriptor(&SystemPromptDelivery::External, Some(&bootstrap));
        assert_eq!(delivery.fallback.as_deref(), Some(bootstrap.as_str()));
        let launch_content = format!("{bootstrap}\n\nUser work must not be submitted here.");
        assert_eq!(
            fallback_delivery_spec(Some(&bootstrap), Some(&launch_content)),
            Some(FallbackDeliverySpec {
                channel: PendingDeliveryChannel::FallbackBootstrap,
                payload: bootstrap.as_str(),
            })
        );

        let spec = fallback_delivery_spec(Some(&bootstrap), Some(&launch_content)).unwrap();
        let mut pending = pending_record_for_spec(spec.clone());
        assert_eq!(
            pending.content_digest,
            pending_delivery(spec.payload).digest
        );
        assert_eq!(pending.channel, PendingDeliveryChannel::FallbackBootstrap);
        assert_eq!(
            pending.provenance,
            crate::multiplexer::types::PendingDeliveryProvenance::FallbackBootstrap
        );
        begin_fallback_attempt(
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "bootstrap".into(),
                started_at_ms: 1,
            },
        )
        .unwrap();

        let sink = RecordingSink::default();
        let mut submissions = 0;
        dispatch_initial_delivery(
            LaunchDeliveryMode::ConfirmedFallback,
            &sink,
            "pair-pane",
            Some(|| submissions += 1),
        );
        confirm_fallback(&mut pending, true).unwrap();
        if pending.state() != PendingDeliveryState::Confirmed {
            dispatch_initial_delivery(
                LaunchDeliveryMode::ConfirmedFallback,
                &sink,
                "pair-pane",
                Some(|| submissions += 1),
            );
        }
        assert_eq!(submissions, 1);
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

    #[test]
    fn fallback_emits_delivering_after_tui_ready() {
        let sink = RecordingSink::default();
        sink.record("pane-1", BootStage::AgentLaunching, None);
        dispatch_initial_delivery(
            LaunchDeliveryMode::ConfirmedFallback,
            &sink,
            "pane-1",
            Some(|| sink.record_transaction()),
        );

        assert_eq!(
            sink.0.lock().as_slice(),
            ["agent_launching", "delivering_instructions", "transaction"]
        );
    }

    #[test]
    fn system_prompt_channel_never_emits_delivering() {
        let sink = RecordingSink::default();
        let mut transactions = 0;
        sink.record("pane-1", BootStage::AgentLaunching, None);

        let result = dispatch_initial_delivery(
            mode_for_descriptor(&SystemPromptDelivery::SessionConfig),
            &sink,
            "pane-1",
            Some(|| transactions += 1),
        );

        assert_eq!(result, None);
        assert_eq!(transactions, 0);
        assert_eq!(sink.0.lock().as_slice(), ["agent_launching"]);
    }
}
