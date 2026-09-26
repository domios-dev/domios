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
    ProviderDialog(TabState),
    Deadline,
    AttemptSuperseded,
}

impl LaunchInputRefusal {
    pub(crate) fn reason(self) -> String {
        match self {
            Self::DirectoryTrustPrompt => "active provider directory-trust prompt".into(),
            Self::ProviderDialog(state) => format!("provider classified {state:?} dialog"),
            Self::Deadline => {
                "provider composer or session binding was not observed before the readiness deadline"
                    .into()
            }
            Self::AttemptSuperseded => "launch attempt changed before input readiness".into(),
        }
    }
}

struct LaunchInputObservation {
    session_bound: bool,
    composer_ready: bool,
    directory_trust_prompt: bool,
    active_interactive_prompt: bool,
    classification: Option<TabStateClassification>,
}

fn decide_launch_input(
    observation: LaunchInputObservation,
) -> Result<Option<LaunchInputEvidence>, LaunchInputRefusal> {
    if observation.directory_trust_prompt {
        return Err(LaunchInputRefusal::DirectoryTrustPrompt);
    }
    if let Some(classification) = observation.classification {
        let active_dialog = match classification.state {
            TabState::WaitingOnAuth | TabState::UpdatePrompt | TabState::RateLimited => true,
            TabState::ClarifyingQuestion => observation.active_interactive_prompt,
            _ => false,
        };
        if classification.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
            && active_dialog
        {
            return Err(LaunchInputRefusal::ProviderDialog(classification.state));
        }
    }
    if observation.session_bound {
        Ok(Some(LaunchInputEvidence::SessionBinding))
    } else {
        Ok(observation
            .composer_ready
            .then_some(LaunchInputEvidence::ProviderComposer))
    }
}

impl ServiceContext {
    pub(crate) fn await_launch_input_readiness(
        &self,
        attempt: &LaunchAttempt,
        agent_id: &str,
        timeout: std::time::Duration,
    ) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
        let signal = self
            .screen_state
            .lock()
            .arm_screen_change_signal(&attempt.pty_id);
        let deadline = Instant::now() + timeout;
        let mut generation = signal.generation();

        loop {
            let Some(current) = LaunchAttempt::current(&self.mux, &attempt.tab_id, &attempt.pty_id)
            else {
                return Err(LaunchInputRefusal::AttemptSuperseded);
            };
            if &current != attempt {
                return Err(LaunchInputRefusal::AttemptSuperseded);
            }

            let (session_bound, session_id, cwd) = {
                let mux = self.mux.read();
                let Some(tab) = mux.get_tab(&attempt.tab_id) else {
                    return Err(LaunchInputRefusal::AttemptSuperseded);
                };
                if !attempt_owns_tab(tab, attempt) {
                    return Err(LaunchInputRefusal::AttemptSuperseded);
                }
                (
                    tab.launch_session_authenticated(),
                    tab.agent_session_id.clone(),
                    tab.cwd.clone().or_else(|| tab.provisioned_cwd.clone()),
                )
            };
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
                let provider_tui = provider.screen_has_tui(&screen);
                let composer_ready = input.is_some_and(|state| {
                    launch_composer_input_ready(state) && (provider_tui || state.alternate_screen)
                });
                let directory_trust_prompt =
                    provider.directory_trust_prompt_response(&screen).is_some();
                let classification = provider.classify_tab_state(&snapshot);
                let active_interactive_prompt = classification.as_ref().is_some_and(|value| {
                    value.state == TabState::ClarifyingQuestion
                        && value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
                }) && provider.parse_prompt(&screen).is_some();
                (
                    composer_ready,
                    directory_trust_prompt,
                    active_interactive_prompt,
                    classification,
                )
            });
            let (composer_ready, directory_trust_prompt, active_interactive_prompt, classification) =
                provider_observation.unwrap_or((false, false, false, None));
            match decide_launch_input(LaunchInputObservation {
                session_bound,
                composer_ready,
                directory_trust_prompt,
                active_interactive_prompt,
                classification,
            }) {
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
}

fn launch_composer_input_ready(input: codeterm_pty::pty::screen_state::ComposerInputState) -> bool {
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
