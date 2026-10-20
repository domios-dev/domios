use super::*;
use crate::agent::tab_state::TabStateSnapshot;
use crate::service::supporter_tab_state::{CandidateVerdict, PROVIDER_UPDATE_PROMPT};
use codeterm_types::pty::PromptKind;

#[path = "unattended_prompt.rs"]
mod unattended_prompt;

impl ServiceContext {
    pub(crate) fn is_scrape_candidate(&self, agent: &str, screen: &str) -> bool {
        manual_update_prompt_candidate(screen)
            || trust_candidate::may_be_directory_trust_prompt(screen)
            || crate::agent::prompt::parse_prompt_safely(|| {
                self.agent_registry.parse_interactive_prompt(agent, screen)
            })
            .is_some()
    }

    pub(crate) fn observe_output_scrape_candidate(&self, snapshot: TabStateSnapshot) {
        let fallback_snapshot = snapshot.clone();
        let unobserved = snapshot.clone();
        if !self.observe_scrape_candidate_from(
            snapshot,
            move |service| service.resolve_scrape_candidate_fallback(&fallback_snapshot),
            crate::service::supporter_tab_state::ObservationTrigger::OutputCandidate,
            None,
            None,
        ) {
            self.resolve_unobserved_launch_gate(&unobserved);
        }
    }

    pub(crate) fn resolve_scrape_candidate_heuristically(&self, snapshot: &TabStateSnapshot) {
        if self.preserve_owner_prompt(snapshot) {
            return;
        }
        let onboarding = {
            let mux = self.mux.read();
            mux.get_tab(&snapshot.tab_id)
                .filter(|tab| {
                    super::super::directory_trust::launch_onboarding_check_applies(
                        tab,
                        &snapshot.agent_type,
                    )
                })
                .and_then(|tab| tab.pty_id.clone())
        };
        if let Some(pty) = onboarding.filter(|_| {
            self.agent_registry
                .with_provider(&snapshot.agent_type, |provider| {
                    provider
                        .launch_onboarding_response(&snapshot.screen_text)
                        .is_some()
                })
                .unwrap_or(false)
        }) {
            self.enqueue_launch_onboarding_screen_check(
                &snapshot.tab_id,
                &pty,
                &snapshot.agent_type,
            );
            return;
        }
        let flags = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(&snapshot.tab_id) else {
                return;
            };
            (
                super::super::directory_trust::directory_trust_check_applies(tab),
                super::super::directory_trust::shared_directory_trust_check_applies(tab),
                tab.auto_approve,
                super::super::directory_trust::directory_trust_skip_reason(tab),
            )
        };
        let trust = trust_candidate::candidate_for_output(
            self,
            &snapshot.tab_id,
            &snapshot.agent_type,
            Some(&snapshot.screen_text),
            flags.0,
            flags.1,
            flags.2,
            flags.3,
        );
        if manual_update_prompt_candidate(&snapshot.screen_text) {
            if let Some((prompt, safe_choice)) = self
                .agent_registry
                .with_provider(&snapshot.agent_type, |provider| {
                    parse_manual_update_prompt_with_response(provider, snapshot)
                })
                .flatten()
            {
                resolve_classified_manual_update_prompt_fallback(
                    self,
                    snapshot,
                    prompt,
                    safe_choice,
                );
            }
            return;
        }
        let parsed = crate::agent::prompt::parse_prompt_safely(|| {
            self.agent_registry
                .parse_interactive_prompt(&snapshot.agent_type, &snapshot.screen_text)
        })
        .filter(|(prompt, kind)| {
            *kind == PromptKind::DirectoryTrust
                || !crate::agent::state_reactor::prompt_is_composer_text(
                    &snapshot.screen_text,
                    prompt,
                )
        });
        let kind = if trust {
            PromptKind::DirectoryTrust
        } else {
            parsed
                .as_ref()
                .map_or(PromptKind::Provider, |(_, kind)| *kind)
        };
        let decision = self.prompt_detector.lock().evaluate_pty_output_parsed(
            &snapshot.tab_id,
            true,
            true,
            parsed.map(|(prompt, _)| crate::agent::prompt::ResolvedPrompt {
                prompt,
                explicit: false,
            }),
        );
        self.apply_prompt_decision_with_kind(&decision, Some(&snapshot.screen_text), kind, false);
    }

    fn preserve_owner_prompt(&self, snapshot: &TabStateSnapshot) -> bool {
        let tab = self.mux.read().get_tab(&snapshot.tab_id).cloned();
        if !tab.is_some_and(|tab| !crate::service::unattended_launch::owns_prompt_answers(&tab)) {
            return false;
        }
        let Some(provider) = self.agent_registry.provider_arc(&snapshot.agent_type) else {
            return false;
        };
        let parsed = crate::agent::prompt::parse_prompt_safely(|| {
            provider.parse_prompt(&snapshot.screen_text)
        });
        let update = crate::service::output::manual_update_prompt_candidate(&snapshot.screen_text)
            && parsed
                .as_ref()
                .is_some_and(|prompt| provider.update_prompt_safe_choice(prompt).is_some());
        let trust = provider
            .parse_directory_trust_prompt(&snapshot.screen_text)
            .is_some();
        let permission = self
            .prompt_detector
            .lock()
            .pending_permission_request(&snapshot.tab_id)
            .is_some_and(|(_, signal)| {
                parsed.as_ref().is_some_and(|prompt| {
                    provider
                        .permission_prompt_response(
                            &signal.tool_name,
                            prompt,
                            &snapshot.screen_text,
                        )
                        .is_some()
                })
            });
        let Some(prompt) = parsed else {
            return false;
        };
        let kind = if update {
            PromptKind::ManualUpdate
        } else if trust {
            PromptKind::DirectoryTrust
        } else if permission {
            PromptKind::Provider
        } else {
            return false;
        };
        let decision = self.prompt_detector.lock().evaluate_pty_output_parsed(
            &snapshot.tab_id,
            true,
            true,
            Some(crate::agent::prompt::ResolvedPrompt {
                prompt,
                explicit: false,
            }),
        );
        self.apply_prompt_decision_with_kind(&decision, Some(&snapshot.screen_text), kind, true);
        true
    }

    pub(crate) fn publish_model_scrape_candidate(
        &self,
        snapshot: &TabStateSnapshot,
        verdict: &CandidateVerdict,
        valid: &dyn Fn() -> bool,
    ) {
        let parent_observation_id = verdict.observation_id();
        if !valid()
            || self
                .prompt_detector
                .lock()
                .active_prompt_explicit(&snapshot.tab_id)
                == Some(true)
        {
            return;
        }
        let launch_onboarding = {
            let mux = self.mux.read();
            mux.get_tab(&snapshot.tab_id).is_some_and(|tab| {
                super::super::directory_trust::launch_onboarding_check_applies(
                    tab,
                    &snapshot.agent_type,
                )
            })
        } && self
            .agent_registry
            .with_provider(&snapshot.agent_type, |provider| {
                provider
                    .launch_onboarding_response(&snapshot.screen_text)
                    .is_some()
            })
            .unwrap_or(false);
        if ((verdict.label == crate::service::supporter_tab_state::MODEL_UNAVAILABLE_OR_QUOTA)
            || (verdict.label == crate::service::supporter_tab_state::LOGIN_OR_AUTH_REQUIRED
                && !launch_onboarding))
            && self.handle_prompt_chain_verdict(&snapshot.tab_id, &verdict.label, false)
        {
            return;
        }
        if self.route_unattended_launch_prompt(snapshot, verdict) {
            return;
        }
        let parsed = if verdict.live_prompt() {
            crate::agent::prompt::parse_prompt_safely(|| {
                self.agent_registry
                    .parse_interactive_prompt(&snapshot.agent_type, &snapshot.screen_text)
            })
            .filter(|(prompt, _)| {
                crate::service::supporter_tab_state::candidate_allows_prompt(
                    verdict,
                    &snapshot.screen_text,
                    prompt,
                )
            })
        } else {
            None
        };
        if self.handle_prompt_chain_verdict(
            &snapshot.tab_id,
            &verdict.label,
            parsed.is_none() && verdict.live_prompt(),
        ) {
            return;
        }
        let kind = if verdict.label == PROVIDER_UPDATE_PROMPT {
            PromptKind::ManualUpdate
        } else {
            parsed
                .as_ref()
                .map_or(PromptKind::Provider, |(_, kind)| *kind)
        };
        if !valid() {
            return;
        }
        if parsed.as_ref().is_some_and(|_| {
            self.try_model_safe_response(snapshot, verdict)
                != crate::service::supporter_tab_state::SafeResponseProgress::NotHandled
        }) {
            return;
        }
        let decision = self.prompt_detector.lock().evaluate_pty_output_parsed(
            &snapshot.tab_id,
            true,
            true,
            parsed.map(|(prompt, _)| crate::agent::prompt::ResolvedPrompt {
                prompt,
                explicit: false,
            }),
        );
        let establishes_prompt = matches!(
            &decision,
            crate::agent::prompt::PromptDecision::NewPrompt { .. }
                | crate::agent::prompt::PromptDecision::PromptUpdated { .. }
        );
        self.apply_prompt_decision_immediately(
            &decision,
            Some(&snapshot.screen_text),
            kind,
            true,
            verdict.confidence >= 0.80,
        );
        if establishes_prompt {
            if let Some(parent_observation_id) = parent_observation_id {
                self.observe_model_prompt_established(snapshot.clone(), parent_observation_id);
            }
        }
    }

    pub(super) fn classify_manual_update_prompt_off_runtime(
        &self,
        pty_id: String,
        snapshot: TabStateSnapshot,
    ) {
        let svc = self.clone();
        std::thread::spawn(move || {
            if svc.preserve_owner_prompt(&snapshot) {
                return;
            }
            let classified = svc
                .agent_registry
                .with_provider(&snapshot.agent_type, |provider| {
                    parse_manual_update_prompt_with_response(provider, &snapshot)
                })
                .flatten();
            let Some((prompt, skip_value)) = classified else {
                return;
            };
            let screen = svc.screen_state.lock().get_screen_text(&pty_id);
            if screen.as_deref() != Some(snapshot.screen_text.as_str()) {
                return;
            }
            resolve_classified_manual_update_prompt(&svc, &snapshot, prompt, skip_value);
        });
    }
}
