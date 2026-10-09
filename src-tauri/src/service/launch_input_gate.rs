use crate::service::launch_readiness::{attempt_owns_tab, LaunchAttempt, OWNER_PROMPT_DEADLINE};
use crate::service::ServiceContext;
use codeterm_plugin::tab_state::{TabState, TabStateClassification, TabStateSnapshot};
use std::time::Instant;

#[cfg(test)]
#[path = "launch_input_gate/tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchInputEvidence {
    SessionBinding,
    ProviderComposer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchInputRefusal {
    PendingPrompt,
    ProviderDialog(TabState),
    Deadline,
    ComposerNotRecognised,
    AttemptSuperseded,
    OwnerPromptDeadline(String),
}

impl LaunchInputRefusal {
    pub(crate) fn reason(self) -> String {
        match self {
            Self::PendingPrompt => "active provider prompt".into(),
            Self::ProviderDialog(state) => format!("provider classified {state:?} dialog"),
            Self::Deadline => "provider composer readiness deadline elapsed".into(),
            Self::ComposerNotRecognised => "provider composer layout was not recognised".into(),
            Self::AttemptSuperseded => "launch attempt changed before input readiness".into(),
            Self::OwnerPromptDeadline(question) => {
                let minutes = OWNER_PROMPT_DEADLINE.as_secs().div_ceil(60);
                format!("owner prompt was not answered before the {minutes} minute deadline: {question}")
            }
        }
    }
}

fn launch_readiness_deadline_refusal(
    requires_structural_composer: bool,
    observed_tui_without_composer: bool,
) -> LaunchInputRefusal {
    if requires_structural_composer && observed_tui_without_composer {
        LaunchInputRefusal::ComposerNotRecognised
    } else {
        LaunchInputRefusal::Deadline
    }
}

struct LaunchInputObservation {
    session_bound: bool,
    composer_ready: bool,
    pending_prompt: bool,
    provider_process_alive: bool,
    provider_busy: bool,
    directory_trust_prompt: bool,
    directory_trust_pending: bool,
    active_interactive_prompt: bool,
    classification: Option<TabStateClassification>,
}

fn delegated_role(
    role: Option<crate::multiplexer::types::AgentRole>,
    role_profile: Option<&str>,
) -> bool {
    role == Some(codeterm_types::agent::roles::AgentRole::Worker)
        || (role == Some(codeterm_types::agent::roles::AgentRole::Manager)
            && role_profile == Some("planner"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum LaunchInputMode {
    Standard,
    LazySessionStart,
}

#[derive(Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "evaluation", content = "value")]
enum GateFact<T> {
    NotEvaluated,
    Observed(T),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
struct GateClassification {
    state: TabState,
    confidence: f32,
}

#[derive(Debug, serde::Serialize)]
struct LaunchInputDecision {
    session_bound: bool,
    composer_ready: bool,
    pending_prompt: bool,
    provider_process_alive: bool,
    provider_busy: bool,
    directory_trust_prompt: bool,
    directory_trust_pending: bool,
    active_interactive_prompt: bool,
    classification: Option<GateClassification>,
}

#[derive(Debug, serde::Serialize)]
struct LaunchInputDiagnostic {
    tab_id: String,
    pty_id: String,
    binding_nonce: String,
    mode: LaunchInputMode,
    event_generation: u64,
    elapsed_ms: u128,
    deadline_ms: u128,
    outcome: &'static str,
    session_bound: GateFact<bool>,
    pending_prompt: GateFact<bool>,
    provider_process_alive: GateFact<bool>,
    process_gate_allows: GateFact<bool>,
    composer_ready: GateFact<bool>,
    provider_tui: GateFact<bool>,
    tui_evidence: GateFact<bool>,
    terminal_input_ready: GateFact<bool>,
    provider_busy: GateFact<bool>,
    directory_trust_prompt: GateFact<bool>,
    directory_trust_pending: GateFact<bool>,
    directory_trust_state: GateFact<codeterm_plugin::provider::DirectoryTrustState>,
    active_interactive_prompt: GateFact<bool>,
    classification: GateFact<Option<GateClassification>>,
    provider_evaluated: bool,
    onboarding_wait: bool,
    decision_inputs: GateFact<LaunchInputDecision>,
    declares_structural_composer: bool,
    observed_tui_without_composer: bool,
}

impl LaunchInputDiagnostic {
    fn new(attempt: &LaunchAttempt, mode: LaunchInputMode, timeout: std::time::Duration) -> Self {
        Self {
            tab_id: attempt.tab_id.clone(),
            pty_id: attempt.pty_id.clone(),
            binding_nonce: attempt.binding_nonce.clone(),
            mode,
            event_generation: 0,
            elapsed_ms: 0,
            deadline_ms: timeout.as_millis(),
            outcome: "not_settled",
            session_bound: GateFact::NotEvaluated,
            pending_prompt: GateFact::NotEvaluated,
            provider_process_alive: GateFact::NotEvaluated,
            process_gate_allows: GateFact::NotEvaluated,
            composer_ready: GateFact::NotEvaluated,
            provider_tui: GateFact::NotEvaluated,
            tui_evidence: GateFact::NotEvaluated,
            terminal_input_ready: GateFact::NotEvaluated,
            provider_busy: GateFact::NotEvaluated,
            directory_trust_prompt: GateFact::NotEvaluated,
            directory_trust_pending: GateFact::NotEvaluated,
            directory_trust_state: GateFact::NotEvaluated,
            active_interactive_prompt: GateFact::NotEvaluated,
            classification: GateFact::NotEvaluated,
            provider_evaluated: false,
            onboarding_wait: false,
            decision_inputs: GateFact::NotEvaluated,
            declares_structural_composer: false,
            observed_tui_without_composer: false,
        }
    }

    fn begin_frame(
        &mut self,
        attempt: &LaunchAttempt,
        timeout: std::time::Duration,
        generation: u64,
    ) {
        *self = Self::new(attempt, self.mode, timeout);
        self.event_generation = generation;
    }

    fn observe_decision(
        &mut self,
        observation: &LaunchInputObservation,
        provider_tui: bool,
        tui_evidence: bool,
        terminal_input_ready: bool,
        trust_state: codeterm_plugin::provider::DirectoryTrustState,
        provider_evaluated: bool,
    ) {
        self.session_bound = GateFact::Observed(observation.session_bound);
        self.pending_prompt = GateFact::Observed(observation.pending_prompt);
        self.process_gate_allows = GateFact::Observed(observation.provider_process_alive);
        let classification = observation
            .classification
            .as_ref()
            .map(|value| GateClassification {
                state: value.state,
                confidence: value.confidence,
            });
        self.decision_inputs = GateFact::Observed(LaunchInputDecision {
            session_bound: observation.session_bound,
            composer_ready: observation.composer_ready,
            pending_prompt: observation.pending_prompt,
            provider_process_alive: observation.provider_process_alive,
            provider_busy: observation.provider_busy,
            directory_trust_prompt: observation.directory_trust_prompt,
            directory_trust_pending: observation.directory_trust_pending,
            active_interactive_prompt: observation.active_interactive_prompt,
            classification: classification.clone(),
        });
        self.provider_evaluated = provider_evaluated;
        if !provider_evaluated {
            return;
        }
        self.composer_ready = GateFact::Observed(observation.composer_ready);
        self.provider_tui = GateFact::Observed(provider_tui);
        self.tui_evidence = GateFact::Observed(tui_evidence);
        self.terminal_input_ready = GateFact::Observed(terminal_input_ready);
        self.provider_busy = GateFact::Observed(observation.provider_busy);
        self.directory_trust_prompt = GateFact::Observed(observation.directory_trust_prompt);
        self.directory_trust_pending = GateFact::Observed(observation.directory_trust_pending);
        self.directory_trust_state = GateFact::Observed(trust_state);
        self.active_interactive_prompt = GateFact::Observed(observation.active_interactive_prompt);
        self.classification = GateFact::Observed(classification);
    }

    fn settle(
        &mut self,
        result: &Result<LaunchInputEvidence, LaunchInputRefusal>,
        elapsed: std::time::Duration,
    ) {
        self.elapsed_ms = elapsed.as_millis();
        self.outcome = match result {
            Ok(LaunchInputEvidence::SessionBinding) => "session_binding",
            Ok(LaunchInputEvidence::ProviderComposer) => "provider_composer",
            Err(LaunchInputRefusal::PendingPrompt) => "pending_prompt",
            Err(LaunchInputRefusal::ProviderDialog(_)) => "provider_dialog",
            Err(LaunchInputRefusal::Deadline) => "deadline",
            Err(LaunchInputRefusal::ComposerNotRecognised) => "composer_not_recognised",
            Err(LaunchInputRefusal::AttemptSuperseded) => "attempt_superseded",
            Err(LaunchInputRefusal::OwnerPromptDeadline(_)) => "owner_prompt_deadline",
        };
    }
}

fn decide_launch_input(
    observation: LaunchInputObservation,
    mode: LaunchInputMode,
) -> Result<Option<LaunchInputEvidence>, LaunchInputRefusal> {
    if mode == LaunchInputMode::LazySessionStart && observation.provider_busy {
        return Ok(None);
    }
    if observation.directory_trust_prompt || observation.directory_trust_pending {
        return Ok(None);
    }
    let refusal = if mode == LaunchInputMode::LazySessionStart && observation.pending_prompt {
        Some(LaunchInputRefusal::PendingPrompt)
    } else {
        observation
            .classification
            .filter(|classification| {
                classification.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
                    && match classification.state {
                        TabState::WaitingOnAuth
                        | TabState::UpdatePrompt
                        | TabState::RateLimited => true,
                        TabState::ClarifyingQuestion => observation.active_interactive_prompt,
                        TabState::Working | TabState::IdleStuck | TabState::Unknown => false,
                    }
            })
            .map(|classification| LaunchInputRefusal::ProviderDialog(classification.state))
    };
    if let Some(refusal) = refusal {
        return match mode {
            LaunchInputMode::Standard => Err(refusal),
            LaunchInputMode::LazySessionStart => Ok(None),
        };
    }
    if mode == LaunchInputMode::LazySessionStart && !observation.provider_process_alive {
        return Ok(None);
    }
    if mode == LaunchInputMode::LazySessionStart {
        return Ok(observation
            .composer_ready
            .then_some(LaunchInputEvidence::ProviderComposer));
    }
    if observation.session_bound {
        Ok(Some(LaunchInputEvidence::SessionBinding))
    } else {
        Ok(observation
            .composer_ready
            .then_some(LaunchInputEvidence::ProviderComposer))
    }
}

fn decide_launch_input_at_deadline(
    observation: LaunchInputObservation,
    mode: LaunchInputMode,
    expired: bool,
    structural_composer: bool,
    observed_tui_without_composer: bool,
) -> Result<Option<LaunchInputEvidence>, LaunchInputRefusal> {
    match decide_launch_input(observation, mode)? {
        Some(evidence) => Ok(Some(evidence)),
        None if expired => Err(launch_readiness_deadline_refusal(
            structural_composer,
            observed_tui_without_composer,
        )),
        None => Ok(None),
    }
}

fn provider_composer_is_ready(
    mode: LaunchInputMode,
    declares_structural_composer: bool,
    terminal_input_ready: bool,
    tui_evidence: bool,
) -> bool {
    terminal_input_ready
        && match mode {
            LaunchInputMode::Standard => tui_evidence,
            LaunchInputMode::LazySessionStart => declares_structural_composer && tui_evidence,
        }
}

fn provider_is_busy(
    mode: LaunchInputMode,
    cursor_position: crate::agent::provider::ComposerCursorPosition,
    classification: Option<&TabStateClassification>,
) -> bool {
    mode == LaunchInputMode::LazySessionStart
        && cursor_position == crate::agent::provider::ComposerCursorPosition::VisibleAnywhere
        && classification.is_some_and(|value| {
            value.state == TabState::Working
                && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
        })
}

fn directory_trust_blocks_input(
    trust_state: codeterm_plugin::provider::DirectoryTrustState,
    prompt_visible: bool,
    prompt_observed: bool,
    checkpoint_reached: bool,
) -> bool {
    prompt_visible
        || (trust_state == codeterm_plugin::provider::DirectoryTrustState::Untrusted
            && !prompt_observed
            && !checkpoint_reached)
}

fn directory_trust_checkpoint_reached(
    mode: LaunchInputMode,
    declares_structural_composer: bool,
    composer_ready: bool,
    pending_prompt: bool,
    active_interactive_prompt: bool,
) -> bool {
    mode == LaunchInputMode::LazySessionStart
        && declares_structural_composer
        && composer_ready
        && !pending_prompt
        && !active_interactive_prompt
}

impl ServiceContext {
    pub(crate) fn await_launch_input_readiness(
        &self,
        attempt: &LaunchAttempt,
        agent_id: &str,
        timeout: std::time::Duration,
    ) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
        self.await_launch_input_readiness_with_mode(
            attempt,
            agent_id,
            timeout,
            LaunchInputMode::Standard,
        )
    }

    pub(crate) fn await_lazy_session_input_readiness(
        &self,
        attempt: &LaunchAttempt,
        agent_id: &str,
        timeout: std::time::Duration,
    ) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
        self.await_launch_input_readiness_with_mode(
            attempt,
            agent_id,
            timeout,
            LaunchInputMode::LazySessionStart,
        )
    }

    fn await_launch_input_readiness_with_mode(
        &self,
        attempt: &LaunchAttempt,
        agent_id: &str,
        timeout: std::time::Duration,
        mode: LaunchInputMode,
    ) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
        self.await_launch_input_readiness_observed(attempt, agent_id, timeout, mode, |diagnostic| {
            if let Ok(snapshot) = serde_json::to_string(diagnostic) {
                log::info!("[launch-input-gate] final={snapshot}");
            }
        })
    }

    fn await_launch_input_readiness_observed(
        &self,
        attempt: &LaunchAttempt,
        agent_id: &str,
        timeout: std::time::Duration,
        mode: LaunchInputMode,
        record_final: impl FnOnce(&LaunchInputDiagnostic),
    ) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
        let mut diagnostic = LaunchInputDiagnostic::new(attempt, mode, timeout);
        let signal = self
            .screen_state
            .lock()
            .arm_screen_change_signal(&attempt.pty_id);
        let _input_wake = (mode == LaunchInputMode::LazySessionStart).then(|| {
            crate::service::launch_readiness::register_input_wake(attempt, signal.clone())
        });
        let started = Instant::now();
        let deadline = started + timeout;
        let mut generation = signal.generation();
        let (provider_readiness, composer_cursor_position, provider_binary_name, status_bar_rows) =
            self.agent_registry
                .with_provider(agent_id, |provider| {
                    (
                        provider.provider_readiness_strategy(),
                        provider.composer_cursor_position(),
                        provider.binary_name().to_string(),
                        provider.status_bar_rows(),
                    )
                })
                .unwrap_or((
                    crate::agent::provider::ProviderReadinessStrategy::default(),
                    crate::agent::provider::ComposerCursorPosition::default(),
                    String::new(),
                    0,
                ));
        let declares_structural_composer = provider_readiness.requires_composer();
        let uses_prompt_glyph_fingerprint = declares_structural_composer
            && composer_cursor_position
                == crate::agent::provider::ComposerCursorPosition::BottomRegion;
        let mut observed_tui_without_composer = false;
        let mut onboarding_choice_submitted = false;
        let mut directory_trust_prompt_observed = false;
        let mut final_observation = false;
        let mut composer_logged = false;

        let result = (|| loop {
            diagnostic.begin_frame(attempt, timeout, generation);
            let Some(current) = LaunchAttempt::current(&self.mux, &attempt.tab_id, &attempt.pty_id)
            else {
                return Err(LaunchInputRefusal::AttemptSuperseded);
            };
            if &current != attempt {
                return Err(LaunchInputRefusal::AttemptSuperseded);
            }

            let (
                session_bound,
                pending_prompt,
                session_id,
                cwd,
                trust_lookup_path,
                skip_permissions,
                delegated_role,
            ) = {
                let mux = self.mux.read();
                let Some(tab) = mux.get_tab(&attempt.tab_id) else {
                    return Err(LaunchInputRefusal::AttemptSuperseded);
                };
                if !attempt_owns_tab(tab, attempt) {
                    return Err(LaunchInputRefusal::AttemptSuperseded);
                }
                (
                    tab.launch_session_authenticated(),
                    tab.pending_prompt.is_some(),
                    tab.agent_session_id.clone(),
                    tab.cwd.clone().or_else(|| tab.provisioned_cwd.clone()),
                    crate::service::directory_trust::directory_trust_check_applies(tab)
                        .then(|| tab.cwd.clone())
                        .flatten(),
                    tab.auto_approve,
                    delegated_role(tab.role, tab.role_profile.as_deref()),
                )
            };
            let provider_process_alive = mode == LaunchInputMode::Standard
                || self.provider_process_alive_in_tab_tree(&attempt.pty_id, &provider_binary_name);
            diagnostic.session_bound = GateFact::Observed(session_bound);
            diagnostic.pending_prompt = GateFact::Observed(pending_prompt);
            diagnostic.process_gate_allows = GateFact::Observed(provider_process_alive);
            if mode == LaunchInputMode::LazySessionStart {
                diagnostic.provider_process_alive = GateFact::Observed(provider_process_alive);
            }
            let (screen, screen_rows, input) = {
                let screen_state = self.screen_state.lock();
                (
                    screen_state
                        .get_screen_text(&attempt.pty_id)
                        .unwrap_or_default(),
                    screen_state
                        .get_screen_rows(&attempt.pty_id)
                        .unwrap_or_default(),
                    screen_state.composer_input_state(&attempt.pty_id),
                )
            };
            let snapshot = TabStateSnapshot {
                tab_id: attempt.tab_id.clone(),
                agent_type: agent_id.to_string(),
                session_id,
                cwd,
                screen_text: screen.clone(),
            };
            directory_trust_prompt_observed |= self
                .agent_registry
                .with_provider(agent_id, |provider| {
                    provider.directory_trust_prompt_response(&screen).is_some()
                })
                .unwrap_or(false);
            let onboarding_response = self
                .agent_registry
                .with_provider(agent_id, |provider| {
                    provider.launch_onboarding_response(&screen)
                })
                .flatten();
            let onboarding_auto_answer = self
                .agent_registry
                .with_provider(agent_id, |provider| provider.onboarding_auto_answer())
                .unwrap_or_default();
            if delegated_role
                || (onboarding_response.is_some()
                    && onboarding_auto_answer.answers_for_owner_launch())
            {
                let decision = self.respond_to_launch_onboarding(
                    &attempt.tab_id,
                    agent_id,
                    &screen,
                    signal.clone(),
                );
                let wait_for_response = match decision {
                    crate::service::directory_trust::TrustDecision::Answer { step } => {
                        onboarding_choice_submitted |= step.as_slice() == b"\r";
                        true
                    }
                    crate::service::directory_trust::TrustDecision::Waiting => true,
                    crate::service::directory_trust::TrustDecision::NoDialog
                        if onboarding_choice_submitted =>
                    {
                        self.notify_launch_onboarding_applied_once(&attempt.tab_id);
                        false
                    }
                    _ => false,
                };
                if wait_for_response {
                    diagnostic.onboarding_wait = true;
                    let remaining =
                        deadline
                            .checked_duration_since(Instant::now())
                            .ok_or_else(|| {
                                launch_readiness_deadline_refusal(
                                    declares_structural_composer,
                                    observed_tui_without_composer,
                                )
                            })?;
                    if remaining.is_zero() || !signal.wait_after(generation, remaining) {
                        return Err(launch_readiness_deadline_refusal(
                            declares_structural_composer,
                            observed_tui_without_composer,
                        ));
                    }
                    generation = signal.generation();
                    continue;
                }
            }
            let provider_observation = self.agent_registry.with_provider(agent_id, |provider| {
                let input_position = if mode == LaunchInputMode::LazySessionStart {
                    composer_cursor_position
                } else {
                    crate::agent::provider::ComposerCursorPosition::BottomRegion
                };
                let classification = provider.classify_tab_state(&snapshot);
                let classified_modal = classification.as_ref().is_some_and(|value| {
                    matches!(
                        value.state,
                        codeterm_plugin::tab_state::TabState::WaitingOnAuth
                            | codeterm_plugin::tab_state::TabState::UpdatePrompt
                            | codeterm_plugin::tab_state::TabState::ClarifyingQuestion
                    ) && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
                });
                let directory_trust_prompt =
                    provider.directory_trust_prompt_response(&screen).is_some();
                let terminal_input_ready = input.as_ref().is_some_and(|state| {
                    launch_composer_input_ready_for_frame(
                        state,
                        input_position,
                        uses_prompt_glyph_fingerprint.then_some(screen_rows.as_slice()),
                        status_bar_rows,
                        directory_trust_prompt || classified_modal,
                    )
                });
                let provider_tui = provider.screen_has_tui(&screen);
                let tui_evidence = if mode == LaunchInputMode::LazySessionStart
                    && composer_cursor_position
                        == crate::agent::provider::ComposerCursorPosition::VisibleAnywhere
                {
                    provider_tui
                } else {
                    provider_tui || input.as_ref().is_some_and(|state| state.alternate_screen)
                };
                let composer_ready = provider_composer_is_ready(
                    mode,
                    declares_structural_composer,
                    terminal_input_ready,
                    tui_evidence,
                );
                let directory_trust_state = trust_lookup_path.as_deref().map_or(
                    codeterm_plugin::provider::DirectoryTrustState::Unknown,
                    |path| {
                        provider
                            .directory_trust_state(
                                &codeterm_paths::strip_verbatim_prefix(path),
                                skip_permissions,
                            )
                            .unwrap_or(codeterm_plugin::provider::DirectoryTrustState::Unknown)
                    },
                );
                let provider_busy =
                    provider_is_busy(mode, composer_cursor_position, classification.as_ref());
                let active_interactive_prompt = classification.as_ref().is_some_and(|value| {
                    value.state == TabState::ClarifyingQuestion
                        && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
                }) && provider.parse_prompt(&screen).is_some();
                (
                    composer_ready,
                    provider_tui,
                    tui_evidence,
                    terminal_input_ready,
                    directory_trust_prompt,
                    directory_trust_state,
                    active_interactive_prompt,
                    classification,
                    provider_busy,
                )
            });
            let provider_evaluated = provider_observation.is_some();
            let (
                composer_ready,
                provider_tui,
                tui_evidence,
                terminal_input_ready,
                directory_trust_prompt,
                directory_trust_state,
                active_interactive_prompt,
                classification,
                provider_busy,
            ) = provider_observation.unwrap_or((
                false,
                false,
                false,
                false,
                false,
                codeterm_plugin::provider::DirectoryTrustState::Unknown,
                false,
                None,
                false,
            ));
            observed_tui_without_composer |=
                uses_prompt_glyph_fingerprint && provider_tui && !terminal_input_ready;
            directory_trust_prompt_observed |= directory_trust_prompt;
            let directory_trust_pending = directory_trust_blocks_input(
                directory_trust_state,
                directory_trust_prompt,
                directory_trust_prompt_observed,
                directory_trust_checkpoint_reached(
                    mode,
                    declares_structural_composer,
                    composer_ready,
                    pending_prompt,
                    active_interactive_prompt,
                ),
            );
            let observation = LaunchInputObservation {
                session_bound,
                composer_ready,
                pending_prompt,
                provider_process_alive,
                provider_busy,
                directory_trust_prompt,
                directory_trust_pending,
                active_interactive_prompt,
                classification,
            };
            if composer_ready && !composer_logged {
                composer_logged = true;
                crate::service::agent_restart::timing::stage(
                    &attempt.tab_id,
                    &attempt.pty_id,
                    "first-composer",
                );
            }
            diagnostic.observe_decision(
                &observation,
                provider_tui,
                tui_evidence,
                terminal_input_ready,
                directory_trust_state,
                provider_evaluated,
            );
            match decide_launch_input_at_deadline(
                observation,
                mode,
                final_observation,
                declares_structural_composer,
                observed_tui_without_composer,
            ) {
                Err(refusal) => return Err(refusal),
                Ok(Some(readiness)) => return Ok(readiness),
                Ok(None) => {}
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || !signal.wait_after(generation, remaining) {
                final_observation = true;
            }
            generation = signal.generation();
        })();
        diagnostic.declares_structural_composer = declares_structural_composer;
        diagnostic.observed_tui_without_composer = observed_tui_without_composer;
        diagnostic.settle(&result, started.elapsed());
        record_final(&diagnostic);
        result
    }

    fn provider_process_alive_in_tab_tree(&self, pty_id: &str, binary_name: &str) -> bool {
        if binary_name.is_empty() {
            return false;
        }
        let root_pid = {
            let pty = self.pty.lock();
            let Some(session) = pty.get_session(pty_id) else {
                return false;
            };
            let Ok(child) = session.child.lock() else {
                return false;
            };
            child.process_id()
        };
        root_pid
            .is_some_and(|root| crate::service::process_tree::has_named_process(root, binary_name))
    }
}

#[cfg(test)]
fn launch_composer_input_ready(
    input: &codeterm_pty::pty::screen_state::ComposerInputState,
    cursor_position: crate::agent::provider::ComposerCursorPosition,
    screen_rows: Option<&[String]>,
    status_bar_rows: u16,
) -> bool {
    launch_composer_input_ready_for_frame(
        input,
        cursor_position,
        screen_rows,
        status_bar_rows,
        false,
    )
}

fn launch_composer_input_ready_for_frame(
    input: &codeterm_pty::pty::screen_state::ComposerInputState,
    cursor_position: crate::agent::provider::ComposerCursorPosition,
    screen_rows: Option<&[String]>,
    status_bar_rows: u16,
    classified_modal_or_trust_frame: bool,
) -> bool {
    if crate::agent::offline_state_classifier::composer_evidence_excluded(
        classified_modal_or_trust_frame,
    ) {
        return false;
    }
    if cursor_position == crate::agent::provider::ComposerCursorPosition::VisibleAnywhere {
        return input.alternate_screen && input.cursor_visible;
    }
    if cursor_position == crate::agent::provider::ComposerCursorPosition::PromptLine {
        return crate::agent::offline_state_classifier::prompt_line_cursor_ready(
            input.cursor_visible,
            input.cursor_row,
            input.last_nonblank_row,
        );
    }
    if let Some(screen_rows) = screen_rows {
        return crate::agent::offline_state_classifier::prompt_glyph_cursor_ready_for_frame(
            screen_rows,
            input.cursor_visible,
            input.cursor_row,
            status_bar_rows,
            classified_modal_or_trust_frame,
        );
    }
    crate::agent::offline_state_classifier::composer_cursor_ready(
        input.cursor_visible,
        input.cursor_row,
        input.rows,
    ) || input
        .inverse_caret_row
        .zip(input.last_nonblank_row)
        .is_some_and(|(row, bottom)| {
            crate::agent::offline_state_classifier::composer_cursor_ready(
                true,
                row,
                bottom.saturating_add(1),
            )
        })
}
