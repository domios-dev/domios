use sha2::{Digest, Sha256};

use crate::agent::provider::SystemPromptDelivery;
use crate::multiplexer::commands::MuxState;
#[cfg(test)]
use crate::multiplexer::types::PendingDeliveryState;
use crate::multiplexer::types::{
    DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord,
};
use crate::service::boot_stage::{BootStage, BootStageSink};
use crate::service::delivery_error::{DeliveryError, SendOrigin};
use crate::service::inject_verify::{InjectionError, InjectionResult};
use crate::service::launch_input_gate::LaunchInputEvidence;

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
pub(crate) struct LaunchCommandTask {
    pub text: Option<String>,
    pub system_prompt: Option<String>,
    pub task_is_first_turn: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingDelivery {
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FallbackDeliverySpec {
    pub channel: PendingDeliveryChannel,
    pub payload: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisibleSpawnTaskSelection<'a> {
    pub provider_launch: Option<&'a str>,
    pub provider_starter: Option<crate::agent::provider::StarterPrompt>,
    pub confirmed_first_turn: Option<VisibleSpawnFirstTurn<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisibleSpawnFirstTurn<'a> {
    AssignedTaskStarter,
    Task(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchConsumptionStage {
    PtyStarting,
    ShellReady,
    ProviderReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchConsumptionOutcome {
    NotReady,
    AlreadyConfirmed,
    Confirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchConsumptionReceipt {
    pub outcome: LaunchConsumptionOutcome,
    pub attempt_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchConsumptionError {
    pub delivery: DeliveryError,
    pub transaction: Option<String>,
}

impl std::fmt::Display for LaunchConsumptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.delivery)
    }
}

impl std::error::Error for LaunchConsumptionError {}

impl LaunchConsumptionError {
    pub(crate) fn surfaces_as_launch_failure(&self) -> bool {
        self.delivery.surfaces_as_failure(SendOrigin::Programmatic)
    }
}

pub(crate) struct LaunchDeliveryGate {
    stage: LaunchConsumptionStage,
}

impl LaunchDeliveryGate {
    pub(crate) fn new() -> Self {
        Self {
            stage: LaunchConsumptionStage::PtyStarting,
        }
    }

    pub(crate) fn mark_shell_ready(&mut self) {
        self.stage = LaunchConsumptionStage::ShellReady;
    }

    pub(crate) fn mark_provider_ready(&mut self) {
        self.stage = LaunchConsumptionStage::ProviderReady;
    }

    pub(crate) fn consume_injection<F>(
        &self,
        mux: &MuxState,
        tab_id: &str,
        sink: &impl BootStageSink,
        transaction: F,
    ) -> Result<LaunchConsumptionReceipt, LaunchConsumptionError>
    where
        F: FnOnce() -> Result<InjectionResult, InjectionError>,
    {
        let attempt = match self.prepare_consumption(mux, tab_id) {
            Ok(attempt) => attempt,
            Err(outcome) => {
                return Ok(LaunchConsumptionReceipt {
                    outcome,
                    attempt_id: None,
                })
            }
        };

        let result = dispatch_initial_delivery(
            LaunchDeliveryMode::ConfirmedFallback,
            sink,
            tab_id,
            Some(transaction),
        )
        .expect("fallback delivery dispatch executes the transaction");
        let transaction_error = result.as_ref().err().map(ToString::to_string);
        let evidence = result
            .map(|injection| injection.echo_observed && injection.submit_observed)
            .map_err(|error| DeliveryError::from_injection(tab_id, error));
        self.finish_consumption(mux, tab_id, &attempt, evidence, transaction_error)
            .map(|outcome| LaunchConsumptionReceipt {
                outcome,
                attempt_id: Some(attempt.attempt_id),
            })
    }

    pub(crate) fn consume_injection_with_readiness<F>(
        &self,
        mux: &MuxState,
        tab_id: &str,
        sink: &impl BootStageSink,
        readiness: Option<LaunchInputEvidence>,
        transaction: F,
    ) -> Result<LaunchConsumptionReceipt, LaunchConsumptionError>
    where
        F: FnOnce() -> Result<InjectionResult, InjectionError>,
    {
        if readiness.is_none() {
            return Ok(LaunchConsumptionReceipt {
                outcome: LaunchConsumptionOutcome::NotReady,
                attempt_id: None,
            });
        }
        self.consume_injection(mux, tab_id, sink, transaction)
    }

    #[cfg(test)]
    pub(crate) fn consume<F>(
        &self,
        mux: &MuxState,
        tab_id: &str,
        sink: &impl BootStageSink,
        transaction: F,
    ) -> Result<LaunchConsumptionOutcome, LaunchConsumptionError>
    where
        F: FnOnce() -> Result<bool, String>,
    {
        let attempt = match self.prepare_consumption(mux, tab_id) {
            Ok(attempt) => attempt,
            Err(outcome) => return Ok(outcome),
        };
        let result = dispatch_initial_delivery(
            LaunchDeliveryMode::ConfirmedFallback,
            sink,
            tab_id,
            Some(transaction),
        )
        .expect("fallback delivery dispatch executes the transaction");
        let submission_observed = result.as_ref().copied().unwrap_or(false);
        let delivery = {
            let mut mux = mux.write();
            let pending = mux
                .get_tab_mut(tab_id)
                .and_then(|tab| tab.pending_delivery.as_mut())
                .expect("fallback launch retains its pending delivery record");
            confirm_fallback_for_attempt(tab_id, pending, &attempt, submission_observed)
        };
        match delivery {
            Ok(()) => result
                .map(|_| LaunchConsumptionOutcome::Confirmed)
                .map_err(|transaction| LaunchConsumptionError {
                    delivery: DeliveryError::SubmitUnconfirmed {
                        tab: tab_id.to_string(),
                        detail: "launch delivery transaction failed".into(),
                    },
                    transaction: Some(transaction),
                }),
            Err(delivery) => Err(LaunchConsumptionError {
                delivery,
                transaction: result.err(),
            }),
        }
    }

    fn prepare_consumption(
        &self,
        mux: &MuxState,
        tab_id: &str,
    ) -> Result<DeliveryAttemptProvenance, LaunchConsumptionOutcome> {
        if self.stage != LaunchConsumptionStage::ProviderReady {
            return Err(LaunchConsumptionOutcome::NotReady);
        }
        let attempt = next_attempt();
        let should_submit = {
            let mut mux = mux.write();
            let pending = mux
                .get_tab_mut(tab_id)
                .and_then(|tab| tab.pending_delivery.as_mut())
                .expect("fallback launch owns a pending delivery record");
            !pending.state().is_accepted()
                && begin_fallback_attempt(tab_id, pending, attempt.clone()).is_ok()
        };
        should_submit
            .then_some(attempt)
            .ok_or(LaunchConsumptionOutcome::AlreadyConfirmed)
    }

    fn finish_consumption(
        &self,
        mux: &MuxState,
        tab_id: &str,
        attempt: &DeliveryAttemptProvenance,
        evidence: Result<bool, DeliveryError>,
        transaction: Option<String>,
    ) -> Result<LaunchConsumptionOutcome, LaunchConsumptionError> {
        let delivery = {
            let mut mux = mux.write();
            let pending = mux
                .get_tab_mut(tab_id)
                .and_then(|tab| tab.pending_delivery.as_mut())
                .expect("fallback launch retains its pending delivery record");
            reconcile_pending_delivery_for_attempt(tab_id, pending, attempt, evidence)
        };
        delivery
            .map(|_| LaunchConsumptionOutcome::Confirmed)
            .map_err(|delivery| LaunchConsumptionError {
                delivery,
                transaction,
            })
    }
}

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

pub(crate) fn command_task(
    provider: &dyn crate::agent::provider::AgentProvider,
    task: Option<&str>,
) -> LaunchCommandTask {
    let system_prompt =
        content_for_descriptor(&provider.system_prompt_delivery(), task).system_prompt;
    let inline_task = system_prompt.is_none()
        && matches!(
            provider.task_delivery(),
            crate::agent::provider::TaskDelivery::InlineFlag
        );
    let inline_task_text = inline_task
        .then(|| {
            task.filter(|text| !text.trim().is_empty())
                .map(str::to_string)
        })
        .flatten();
    LaunchCommandTask {
        text: system_prompt.clone().or(inline_task_text),
        system_prompt,
        task_is_first_turn: inline_task,
    }
}

pub(crate) fn select_visible_spawn_task<'a>(
    role: Option<&str>,
    task: Option<&'a str>,
    system_prompt_capable: bool,
    native_starter_capable: bool,
) -> VisibleSpawnTaskSelection<'a> {
    let task = task.filter(|text| !text.trim().is_empty());
    match (role, task, system_prompt_capable, native_starter_capable) {
        (None, Some(task), true, true) => VisibleSpawnTaskSelection {
            provider_launch: Some(task),
            provider_starter: Some(crate::agent::provider::StarterPrompt::AssignedTask),
            confirmed_first_turn: None,
        },
        (None, Some(task), true, false) => VisibleSpawnTaskSelection {
            provider_launch: Some(task),
            provider_starter: None,
            confirmed_first_turn: Some(VisibleSpawnFirstTurn::AssignedTaskStarter),
        },
        (None, Some(task), false, _) => VisibleSpawnTaskSelection {
            provider_launch: None,
            provider_starter: None,
            confirmed_first_turn: Some(VisibleSpawnFirstTurn::Task(task)),
        },
        _ => VisibleSpawnTaskSelection {
            provider_launch: task,
            provider_starter: None,
            confirmed_first_turn: None,
        },
    }
}

pub(crate) fn visible_spawn_capabilities(
    provider: &dyn crate::agent::provider::AgentProvider,
) -> (bool, bool) {
    (
        matches!(
            mode_for_descriptor(&provider.system_prompt_delivery()),
            LaunchDeliveryMode::SystemPrompt
        ),
        provider
            .starter_prompt_text(crate::agent::provider::StarterPrompt::AssignedTask)
            .is_some(),
    )
}

pub(crate) fn assigned_task_starter(provider_text: Option<&str>) -> String {
    provider_text
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("Begin the task assigned in your system instructions.")
        .to_string()
}

pub(crate) fn mark_starter_prompt(
    prompt: crate::agent::provider::StarterPrompt,
    text: String,
) -> String {
    match prompt.system_prefix() {
        Some(prefix) => format!(" {prefix}{text}"),
        None => text,
    }
}

/// A native starter turn already carries the handshake via the launch command;
/// only a provider without one still needs the PTY-confirmed fallback turn.
pub(crate) fn startup_handshake_confirmed_first_turn(
    native_starter_capable: bool,
) -> Option<crate::agent::provider::StarterPrompt> {
    (!native_starter_capable).then_some(crate::agent::provider::StarterPrompt::StartupHandshake)
}

pub(crate) fn startup_starter_prompt(
    supervised: bool,
    task_bound: bool,
) -> Option<crate::agent::provider::StarterPrompt> {
    (supervised && !task_bound).then_some(crate::agent::provider::StarterPrompt::StartupHandshake)
}

pub(crate) fn pending_delivery(text: &str) -> PendingDelivery {
    let digest = Sha256::digest(text.as_bytes());
    PendingDelivery {
        digest: format!("{digest:x}"),
    }
}

#[cfg(test)]
fn pending_record_for(text: &str) -> PendingDeliveryRecord {
    pending_record_for_spec(FallbackDeliverySpec {
        channel: PendingDeliveryChannel::InteractiveInput,
        payload: text.to_owned(),
    })
}

pub(crate) fn fallback_delivery_spec(
    fallback_bootstrap: Option<&str>,
    task: Option<&str>,
    starter_text: Option<&str>,
) -> Option<FallbackDeliverySpec> {
    if let (Some(bootstrap), Some(starter)) = (fallback_bootstrap, starter_text) {
        return Some(FallbackDeliverySpec {
            channel: PendingDeliveryChannel::FallbackBootstrap,
            payload: format!("{bootstrap}\n\n{starter}"),
        });
    }
    fallback_bootstrap
        .map(|payload| FallbackDeliverySpec {
            channel: PendingDeliveryChannel::FallbackBootstrap,
            payload: payload.to_owned(),
        })
        .or_else(|| {
            task.map(|payload| FallbackDeliverySpec {
                channel: PendingDeliveryChannel::InteractiveInput,
                payload: payload.to_owned(),
            })
        })
}

pub(crate) fn pending_record_for_spec(spec: FallbackDeliverySpec) -> PendingDeliveryRecord {
    PendingDeliveryRecord::new(pending_delivery(&spec.payload).digest, spec.channel)
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
    tab_id: &str,
    pending: &mut PendingDeliveryRecord,
    attempt: DeliveryAttemptProvenance,
) -> Result<(), DeliveryError> {
    if pending.state().is_accepted() {
        return Ok(());
    }
    pending
        .replace_attempt(attempt)
        .map_err(|error| DeliveryError::SubmitUnconfirmed {
            tab: tab_id.to_string(),
            detail: format!("pending delivery transition: {error:?}"),
        })
}

pub(crate) fn confirm_fallback(
    tab_id: &str,
    pending: &mut PendingDeliveryRecord,
    submission_observed: bool,
) -> Result<(), DeliveryError> {
    if pending.state().is_accepted() {
        return Ok(());
    }
    if !submission_observed {
        return Err(DeliveryError::SubmitUnconfirmed {
            tab: tab_id.to_string(),
            detail: "delivery acknowledgement not observed".into(),
        });
    }
    pending
        .confirm()
        .map_err(|error| DeliveryError::SubmitUnconfirmed {
            tab: tab_id.to_string(),
            detail: format!("pending delivery transition: {error:?}"),
        })
}

#[cfg(test)]
fn confirm_fallback_for_attempt(
    tab_id: &str,
    pending: &mut PendingDeliveryRecord,
    attempt: &DeliveryAttemptProvenance,
    submission_observed: bool,
) -> Result<(), DeliveryError> {
    if pending.attempt_provenance() != Some(attempt) {
        return Err(DeliveryError::SubmitUnconfirmed {
            tab: tab_id.to_string(),
            detail: "delivery attempt was superseded before acknowledgement".into(),
        });
    }
    confirm_fallback(tab_id, pending, submission_observed)
}

fn reconcile_pending_delivery_for_attempt(
    tab_id: &str,
    pending: &mut PendingDeliveryRecord,
    attempt: &DeliveryAttemptProvenance,
    evidence: Result<bool, DeliveryError>,
) -> Result<(), DeliveryError> {
    if pending.attempt_provenance() != Some(attempt) {
        return Err(DeliveryError::SubmitUnconfirmed {
            tab: tab_id.to_string(),
            detail: "delivery attempt was superseded before reconciliation".into(),
        });
    }
    reconcile_pending_delivery(tab_id, pending, evidence)
}

pub(crate) fn reconcile_pending_delivery(
    tab_id: &str,
    pending: &mut PendingDeliveryRecord,
    evidence: Result<bool, DeliveryError>,
) -> Result<(), DeliveryError> {
    if pending.state().is_accepted() {
        return Ok(());
    }
    match evidence {
        Ok(true) => {
            pending.mark_ingested();
            confirm_fallback(tab_id, pending, true)
        }
        Ok(false) => {
            pending.mark_unconfirmed();
            Err(DeliveryError::SubmitUnconfirmed {
                tab: tab_id.to_string(),
                detail: "delivery acknowledgement not observed".into(),
            })
        }
        Err(error) => {
            if error.bytes_written() {
                pending.mark_unconfirmed();
            }
            Err(error)
        }
    }
}

pub(crate) fn dispatch_initial_delivery<T, F>(
    mode: LaunchDeliveryMode,
    sink: &impl BootStageSink,
    tab_id: &str,
    transaction: Option<F>,
) -> Option<T>
where
    F: FnOnce() -> T,
{
    match mode {
        LaunchDeliveryMode::SystemPrompt => None,
        LaunchDeliveryMode::ConfirmedFallback => transaction.map(|transaction| {
            sink.record(tab_id, BootStage::DeliveringInstructions, None);
            transaction()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
    };
    use crate::agent::registry::AgentRegistry;
    use crate::service::orchestrator::{build_agent_command, SpawnAgentParams};
    use codeterm_util::lock_hold::TimedRwLock;
    use parking_lot::Mutex;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

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
    fn launch_delivery_rejects_missing_readiness_before_transaction() {
        let (state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let mux: MuxState = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
            "mux-test", state,
        ));
        let sink = RecordingSink::default();
        let mut gate = LaunchDeliveryGate::new();
        gate.mark_provider_ready();
        let mut transactions = 0;
        let receipt = gate
            .consume_injection_with_readiness(&mux, &tab_id, &sink, None, || {
                transactions += 1;
                panic!("the launch transaction must not run without readiness evidence");
            })
            .expect("readiness refusal is not a write error");

        assert_eq!(receipt.outcome, LaunchConsumptionOutcome::NotReady);
        assert_eq!(transactions, 0);
        assert!(sink.0.lock().is_empty());
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
    fn roleless_visible_task_uses_native_starter_with_system_delivery() {
        assert_eq!(
            select_visible_spawn_task(None, Some("task"), true, true),
            VisibleSpawnTaskSelection {
                provider_launch: Some("task"),
                provider_starter: Some(crate::agent::provider::StarterPrompt::AssignedTask),
                confirmed_first_turn: None,
            }
        );
    }

    #[test]
    fn roleless_visible_task_falls_back_when_native_starter_is_unavailable() {
        assert_eq!(
            select_visible_spawn_task(None, Some("task"), true, false),
            VisibleSpawnTaskSelection {
                provider_launch: Some("task"),
                provider_starter: None,
                confirmed_first_turn: Some(VisibleSpawnFirstTurn::AssignedTaskStarter),
            }
        );
    }

    #[test]
    fn native_starter_capable_startup_handshake_is_not_confirmed_a_second_time() {
        assert_eq!(startup_handshake_confirmed_first_turn(true), None);
    }

    #[test]
    fn startup_ack_starter_requires_a_supervised_non_task_launch() {
        assert_eq!(startup_starter_prompt(false, false), None);
        assert_eq!(
            startup_starter_prompt(true, false),
            Some(crate::agent::provider::StarterPrompt::StartupHandshake)
        );
        assert_eq!(startup_starter_prompt(true, true), None);
    }

    #[test]
    fn pty_only_startup_handshake_still_confirms_the_first_turn() {
        assert_eq!(
            startup_handshake_confirmed_first_turn(false),
            Some(crate::agent::provider::StarterPrompt::StartupHandshake)
        );
    }

    #[test]
    fn roleless_visible_task_uses_the_full_turn_without_system_delivery() {
        assert_eq!(
            select_visible_spawn_task(None, Some("task"), false, false),
            VisibleSpawnTaskSelection {
                provider_launch: None,
                provider_starter: None,
                confirmed_first_turn: Some(VisibleSpawnFirstTurn::Task("task")),
            }
        );
    }

    #[test]
    fn team_task_keeps_the_existing_provider_launch_route() {
        for role in ["Worker", "Planner", "Orchestrator"] {
            for (system_prompt_capable, native_starter_capable) in
                [(false, false), (true, false), (true, true)]
            {
                assert_eq!(
                    select_visible_spawn_task(
                        Some(role),
                        Some("task"),
                        system_prompt_capable,
                        native_starter_capable,
                    ),
                    VisibleSpawnTaskSelection {
                        provider_launch: Some("task"),
                        provider_starter: None,
                        confirmed_first_turn: None,
                    }
                );
            }
        }
    }

    #[test]
    fn assigned_starter_prefers_provider_text_and_marks_the_turn() {
        let provider_text = String::from("opaque provider starter");
        assert_eq!(assigned_task_starter(Some(&provider_text)), provider_text);
        let marked = mark_starter_prompt(
            crate::agent::provider::StarterPrompt::AssignedTask,
            provider_text.clone(),
        );
        assert!(marked.ends_with(&provider_text));
        assert_ne!(marked, provider_text);
    }

    #[test]
    fn fallback_first_message_confirmed_exactly_once() {
        let mut pending = pending_record_for("task");
        begin_fallback_attempt(
            "tab",
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "first".into(),
                started_at_ms: 1,
            },
        )
        .unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::Pending);
        confirm_fallback("tab", &mut pending, true).unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::AcceptedWeak);
        confirm_fallback("tab", &mut pending, false).unwrap();
    }

    /// A launch that stops at its output deadline leaves the bootstrap payload
    /// undelivered. Only the delivery's own evidence resolves that record: the
    /// deadline never clears it, an acknowledged write accepts it exactly once,
    /// and a write that produced nothing leaves it deliverable rather than lost.
    #[test]
    fn pending_delivery_reconciles_from_delivery_evidence() {
        let mut pending = pending_record_for("general bootstrap");
        assert_eq!(pending.state(), PendingDeliveryState::Pending);
        assert!(pending.state().needs_delivery());

        // Nothing was written, so the record is neither accepted nor discarded.
        let unwritten = reconcile_pending_delivery(
            "ga-tab",
            &mut pending,
            Err(DeliveryError::TabBusyRefused {
                tab: "ga-tab".into(),
                detail: "agent launch deadline elapsed".into(),
            }),
        );
        assert!(unwritten.is_err());
        assert_eq!(pending.state(), PendingDeliveryState::Pending);
        assert!(pending.state().needs_delivery());

        // An unacknowledged submit is uncertainty, not delivery: still owed.
        assert!(reconcile_pending_delivery("ga-tab", &mut pending, Ok(false)).is_err());
        assert_eq!(pending.state(), PendingDeliveryState::Unconfirmed);
        assert!(pending.state().needs_delivery());

        // Acknowledged delivery, and only that, retires the record.
        reconcile_pending_delivery("ga-tab", &mut pending, Ok(true)).unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::AcceptedWeak);
        assert!(!pending.state().needs_delivery());

        // A second reconciliation of the same record is a no-op, so an accepted
        // payload can never be replayed by a later evidence event.
        reconcile_pending_delivery("ga-tab", &mut pending, Ok(true)).unwrap();
        reconcile_pending_delivery("ga-tab", &mut pending, Ok(false)).unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::AcceptedWeak);
    }

    #[test]
    fn pair_fallback_bootstrap_submits_identity_once_without_work() {
        let bootstrap = crate::service::role_identity::ManagerBootstrapInputs {
            role: "Orchestrator".into(),
            role_kind: "orchestrator".into(),
            team_id: "not yet established".into(),
            team_name: "not yet established".into(),
            plan_id: "no plan bound yet".into(),
            project: "Manager Bootstrap Fixture".into(),
            coordination: None,
        }
        .compose();
        let delivery = content_for_descriptor(&SystemPromptDelivery::External, Some(&bootstrap));
        assert_eq!(delivery.fallback.as_deref(), Some(bootstrap.as_str()));
        let launch_content = format!("{bootstrap}\n\nUser work must not be submitted here.");
        assert_eq!(
            fallback_delivery_spec(Some(&bootstrap), Some(&launch_content), None),
            Some(FallbackDeliverySpec {
                channel: PendingDeliveryChannel::FallbackBootstrap,
                payload: bootstrap.clone(),
            })
        );

        let spec = fallback_delivery_spec(Some(&bootstrap), Some(&launch_content), None).unwrap();
        let mut pending = pending_record_for_spec(spec.clone());
        assert_eq!(
            pending.content_digest,
            pending_delivery(&spec.payload).digest
        );
        assert_eq!(pending.channel, PendingDeliveryChannel::FallbackBootstrap);
        assert_eq!(
            pending.provenance,
            crate::multiplexer::types::PendingDeliveryProvenance::FallbackBootstrap
        );
        begin_fallback_attempt(
            "pair-tab",
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
            "pair-tab",
            Some(|| submissions += 1),
        );
        confirm_fallback("pair-tab", &mut pending, true).unwrap();
        if !pending.state().is_accepted() {
            dispatch_initial_delivery(
                LaunchDeliveryMode::ConfirmedFallback,
                &sink,
                "pair-tab",
                Some(|| submissions += 1),
            );
        }
        assert_eq!(submissions, 1);
    }

    fn assert_recorded_fallback(bootstrap: &str, starter: &str, assigned: bool) {
        let spec = fallback_delivery_spec(Some(bootstrap), None, Some(starter)).unwrap();
        let pending = pending_record_for_spec(spec.clone());
        assert_eq!(pending.channel, PendingDeliveryChannel::FallbackBootstrap);
        assert_eq!(
            pending.content_digest,
            pending_delivery(&spec.payload).digest
        );
        assert!(spec.payload.contains(bootstrap));
        assert!(spec.payload.contains(starter));
        assert_eq!(
            spec.payload.contains("Execute the assigned task."),
            assigned
        );
        let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(pending);
        let mux = Arc::new(TimedRwLock::new("fallback-delivery-test", state));
        let sink = RecordingSink::default();
        let mut submitted = Vec::new();
        let mut gate = LaunchDeliveryGate::new();
        gate.mark_shell_ready();
        gate.mark_provider_ready();
        let receipt = gate
            .consume_injection(&mux, &tab_id, &sink, || {
                submitted.push(spec.payload.clone());
                Ok(InjectionResult {
                    bytes_written: spec.payload.len(),
                    echo_observed: true,
                    submit_observed: true,
                    submit_anomaly: false,
                    submit_key_writes: 1,
                    elapsed: Duration::ZERO,
                })
            })
            .unwrap();
        assert_eq!(receipt.outcome, LaunchConsumptionOutcome::Confirmed);
        assert!(receipt.attempt_id.is_some());
        let pending = mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_delivery
            .clone()
            .unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::AcceptedWeak);
        assert_eq!(submitted.len(), 1);
        assert_eq!(
            pending.content_digest,
            pending_delivery(&submitted[0]).digest
        );
    }

    #[test]
    fn fallback_no_task_manager_persists_and_submits_one_identical_idle_payload() {
        assert_recorded_fallback("manager identity", "You are a newly started agent. Wait for instructions from the user or your orchestration parent.", false);
    }

    #[test]
    fn fallback_task_manager_persists_and_submits_one_identical_assigned_payload() {
        assert_recorded_fallback("manager identity", "Execute the assigned task.", true);
    }

    struct RecordingInlineProvider;

    impl AgentProvider for RecordingInlineProvider {
        fn id(&self) -> &str {
            "recording-inline"
        }

        fn display_name(&self) -> &str {
            "Recording Inline"
        }

        fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
            let marker = match params.starter_prompt {
                crate::agent::provider::StarterPrompt::NoStarter => "INLINE-NONE",
                crate::agent::provider::StarterPrompt::AssignedTask => "INLINE-ASSIGNED",
                crate::agent::provider::StarterPrompt::IdleWakeup => "INLINE-IDLE",
                crate::agent::provider::StarterPrompt::TeamBootstrap => "INLINE-TEAM",
                crate::agent::provider::StarterPrompt::StartupHandshake => "INLINE-STARTUP",
            };
            format!(
                "recording-inline --system {} {marker}",
                params.task.as_deref().unwrap_or_default()
            )
        }

        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay {
                duration: Duration::ZERO,
            }
        }

        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::InlineFlag
        }

        fn system_prompt_delivery(&self) -> SystemPromptDelivery {
            SystemPromptDelivery::CliFlag("--system".into())
        }

        fn detect_from_title(&self, _: &str) -> bool {
            false
        }

        fn detect_from_output(&self, _: &str) -> bool {
            false
        }

        fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
            None
        }

        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
            None
        }

        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
    }

    #[test]
    fn system_manager_uses_inline_idle_and_zero_fallback_record() {
        let registry = AgentRegistry::new();
        registry.register_provider(Box::new(RecordingInlineProvider));
        let params = SpawnAgentParams {
            starter_prompt: crate::agent::provider::StarterPrompt::IdleWakeup,
            ..Default::default()
        };
        let command = build_agent_command(
            "recording-inline",
            &params,
            Some("manager identity"),
            None,
            &registry,
        );
        assert!(command.contains("--system manager identity"));
        assert!(command.contains("INLINE-IDLE"));
        let mode = mode_for_descriptor(&SystemPromptDelivery::CliFlag("--system".into()));
        assert_eq!(mode, LaunchDeliveryMode::SystemPrompt);
        let pending_spec = (mode == LaunchDeliveryMode::ConfirmedFallback)
            .then(|| fallback_delivery_spec(None, None, None))
            .flatten();
        assert!(pending_spec.is_none());
    }

    #[test]
    fn fallback_deadline_yields_structured_error() {
        let mut pending = pending_record_for("task");
        begin_fallback_attempt(
            "tab",
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "first".into(),
                started_at_ms: 1,
            },
        )
        .unwrap();
        assert_eq!(
            confirm_fallback("tab", &mut pending, false)
                .unwrap_err()
                .code(),
            "submit_unconfirmed"
        );
        assert_eq!(pending.state(), PendingDeliveryState::Pending);
        assert_eq!(pending.attempt_provenance().unwrap().attempt_id, "first");
    }

    #[test]
    fn fallback_reconnect_never_resends_confirmed() {
        let mut pending = pending_record_for("task");
        pending.confirm().unwrap();
        assert!(confirm_fallback("tab", &mut pending, false).is_ok());
    }

    #[test]
    fn fallback_retry_after_failure_redelivers_once() {
        let mut pending = pending_record_for("task");
        begin_fallback_attempt(
            "tab",
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "first".into(),
                started_at_ms: 1,
            },
        )
        .unwrap();
        assert_eq!(
            confirm_fallback("tab", &mut pending, false)
                .unwrap_err()
                .code(),
            "submit_unconfirmed"
        );
        begin_fallback_attempt(
            "tab",
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "retry".into(),
                started_at_ms: 2,
            },
        )
        .unwrap();
        assert_eq!(pending.attempt_provenance().unwrap().attempt_id, "retry");
        confirm_fallback("tab", &mut pending, true).unwrap();
        assert_eq!(pending.state(), PendingDeliveryState::AcceptedWeak);
    }

    #[test]
    fn superseded_attempt_cannot_confirm_the_replacement_record() {
        let mut pending = pending_record_for("task");
        let first = DeliveryAttemptProvenance {
            attempt_id: "first".into(),
            started_at_ms: 1,
        };
        begin_fallback_attempt("tab", &mut pending, first.clone()).unwrap();
        begin_fallback_attempt(
            "tab",
            &mut pending,
            DeliveryAttemptProvenance {
                attempt_id: "replacement".into(),
                started_at_ms: 2,
            },
        )
        .unwrap();

        let error = confirm_fallback_for_attempt("tab", &mut pending, &first, true).unwrap_err();
        assert_eq!(error.code(), "submit_unconfirmed");
        assert_eq!(pending.state(), PendingDeliveryState::Pending);
        assert_eq!(
            pending.attempt_provenance().unwrap().attempt_id,
            "replacement"
        );
    }

    #[test]
    fn fallback_emits_delivering_after_tui_ready() {
        let sink = RecordingSink::default();
        sink.record("tab-1", BootStage::AgentLaunching, None);
        dispatch_initial_delivery(
            LaunchDeliveryMode::ConfirmedFallback,
            &sink,
            "tab-1",
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
        sink.record("tab-1", BootStage::AgentLaunching, None);

        let result = dispatch_initial_delivery(
            mode_for_descriptor(&SystemPromptDelivery::SessionConfig),
            &sink,
            "tab-1",
            Some(|| transactions += 1),
        );

        assert_eq!(result, None);
        assert_eq!(transactions, 0);
        assert_eq!(sink.0.lock().as_slice(), ["agent_launching"]);
    }
}
