use crate::service::launch_readiness::{attempt_owns_tab, LaunchAttempt};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchInputRefusal {
    DirectoryTrustPrompt,
    PendingPrompt,
    ProviderDialog(TabState),
    Deadline,
    AttemptSuperseded,
    OwnerPromptDeadline(String),
}

impl LaunchInputRefusal {
    pub(crate) fn reason(self) -> String {
        match self {
            Self::DirectoryTrustPrompt => "active provider directory-trust prompt".into(),
            Self::PendingPrompt => "active provider prompt".into(),
            Self::ProviderDialog(state) => format!("provider classified {state:?} dialog"),
            Self::Deadline => "provider composer readiness deadline elapsed".into(),
            Self::AttemptSuperseded => "launch attempt changed before input readiness".into(),
            Self::OwnerPromptDeadline(question) => {
                format!("owner prompt was not answered before the 30 minute deadline: {question}")
            }
        }
    }
}

struct LaunchInputObservation {
    session_bound: bool,
    composer_ready: bool,
    pending_prompt: bool,
    provider_process_alive: bool,
    provider_busy: bool,
    directory_trust_prompt: bool,
    active_interactive_prompt: bool,
    classification: Option<TabStateClassification>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LaunchInputMode {
    Standard,
    LazySessionStart,
}

fn decide_launch_input(
    observation: LaunchInputObservation,
    mode: LaunchInputMode,
) -> Result<Option<LaunchInputEvidence>, LaunchInputRefusal> {
    if mode == LaunchInputMode::LazySessionStart && observation.provider_busy {
        return Ok(None);
    }
    let refusal = if observation.directory_trust_prompt {
        Some(LaunchInputRefusal::DirectoryTrustPrompt)
    } else if mode == LaunchInputMode::LazySessionStart && observation.pending_prompt {
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
        let signal = self
            .screen_state
            .lock()
            .arm_screen_change_signal(&attempt.pty_id);
        let deadline = Instant::now() + timeout;
        let mut generation = signal.generation();
        let (provider_readiness, composer_cursor_position, provider_binary_name) = self
            .agent_registry
            .with_provider(agent_id, |provider| {
                (
                    provider.provider_readiness_strategy(),
                    provider.composer_cursor_position(),
                    provider.binary_name().to_string(),
                )
            })
            .unwrap_or((
                crate::agent::provider::ProviderReadinessStrategy::default(),
                crate::agent::provider::ComposerCursorPosition::default(),
                String::new(),
            ));
        let declares_structural_composer = provider_readiness.requires_composer();

        loop {
            let Some(current) = LaunchAttempt::current(&self.mux, &attempt.tab_id, &attempt.pty_id)
            else {
                return Err(LaunchInputRefusal::AttemptSuperseded);
            };
            if &current != attempt {
                return Err(LaunchInputRefusal::AttemptSuperseded);
            }

            let (session_bound, pending_prompt, session_id, cwd) = {
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
                )
            };
            let provider_process_alive = mode == LaunchInputMode::Standard
                || self.provider_process_alive_in_tab_tree(&attempt.pty_id, &provider_binary_name);
            let (screen, input) = {
                let screen_state = self.screen_state.lock();
                (
                    screen_state
                        .get_screen_text(&attempt.pty_id)
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
            let provider_observation = self.agent_registry.with_provider(agent_id, |provider| {
                let input_position = if mode == LaunchInputMode::LazySessionStart {
                    composer_cursor_position
                } else {
                    crate::agent::provider::ComposerCursorPosition::BottomRegion
                };
                let terminal_input_ready = input
                    .as_ref()
                    .is_some_and(|state| launch_composer_input_ready(state, input_position));
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
                let directory_trust_prompt =
                    provider.directory_trust_prompt_response(&screen).is_some();
                let classification = provider.classify_tab_state(&snapshot);
                let provider_busy =
                    provider_is_busy(mode, composer_cursor_position, classification.as_ref());
                let active_interactive_prompt = classification.as_ref().is_some_and(|value| {
                    value.state == TabState::ClarifyingQuestion
                        && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
                }) && provider.parse_prompt(&screen).is_some();
                (
                    composer_ready,
                    directory_trust_prompt,
                    active_interactive_prompt,
                    classification,
                    provider_busy,
                )
            });
            let (
                composer_ready,
                directory_trust_prompt,
                active_interactive_prompt,
                classification,
                provider_busy,
            ) = provider_observation.unwrap_or((false, false, false, None, false));
            match decide_launch_input(
                LaunchInputObservation {
                    session_bound,
                    composer_ready,
                    pending_prompt,
                    provider_process_alive,
                    provider_busy,
                    directory_trust_prompt,
                    active_interactive_prompt,
                    classification,
                },
                mode,
            ) {
                Err(refusal) => return Err(refusal),
                Ok(Some(readiness)) => return Ok(readiness),
                Ok(None) => {}
            }

            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(LaunchInputRefusal::Deadline)?;
            if remaining.is_zero() || !signal.wait_after(generation, remaining) {
                return Err(LaunchInputRefusal::Deadline);
            }
            generation = signal.generation();
        }
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

fn launch_composer_input_ready(
    input: &codeterm_pty::pty::screen_state::ComposerInputState,
    cursor_position: crate::agent::provider::ComposerCursorPosition,
) -> bool {
    if cursor_position == crate::agent::provider::ComposerCursorPosition::VisibleAnywhere {
        return input.alternate_screen && input.cursor_visible;
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
